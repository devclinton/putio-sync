//! Keeping the source's timings (research R9).
//!
//! Whatever translates, the saved file has the source's cue count and exactly the source's
//! start and end times. A result from outside with the same count but other times takes the
//! source's times (position by position); a different count is refused.

use putio_sync_subtitles::srt::Cue;

/// Why a result was refused
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AlignError {
    /// The result holds no cues
    #[error("the result has no subtitle cues")]
    NoCues,
    /// The result has another number of cues than the source
    #[error("result has {got} cues, the source has {expected}")]
    CountMismatch {
        /// Cues in the source
        expected: usize,
        /// Cues in the result
        got: usize,
    },
    /// A cue that has text in the source came back empty
    #[error("cue {number} is empty")]
    EmptyCue {
        /// 1-based position
        number: usize,
    },
    /// The invariant failed: a bug, never a user error
    #[error("internal: timing mismatch at cue {number}")]
    TimingMismatch {
        /// 1-based position
        number: usize,
    },
}

/// A result with the source's timings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aligned {
    /// The cues to save
    pub cues: Vec<Cue>,
    /// How many cues had other times in the result than in the source
    pub realigned: usize,
}

/// Combine the source's timings with the result's text, cue by cue
pub fn realign(source: &[Cue], result: &[Cue]) -> Result<Aligned, AlignError> {
    if result.is_empty() {
        return Err(AlignError::NoCues);
    }
    if source.len() != result.len() {
        return Err(AlignError::CountMismatch {
            expected: source.len(),
            got: result.len(),
        });
    }
    let mut realigned = 0;
    let mut cues = Vec::with_capacity(source.len());
    for (i, (s, r)) in source.iter().zip(result).enumerate() {
        if r.text.trim().is_empty() && !s.text.trim().is_empty() {
            return Err(AlignError::EmptyCue { number: i + 1 });
        }
        if (s.start_ms, s.end_ms) != (r.start_ms, r.end_ms) {
            realigned += 1;
        }
        cues.push(Cue {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: r.text.clone(),
        });
    }
    Ok(Aligned { cues, realigned })
}

/// The invariant checked before any write: same length, identical start and end per cue
pub fn verify_same_timing(source: &[Cue], output: &[Cue]) -> Result<(), AlignError> {
    if source.len() != output.len() {
        return Err(AlignError::CountMismatch {
            expected: source.len(),
            got: output.len(),
        });
    }
    for (i, (s, o)) in source.iter().zip(output).enumerate() {
        if (s.start_ms, s.end_ms) != (o.start_ms, o.end_ms) {
            return Err(AlignError::TimingMismatch { number: i + 1 });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: u64, end: u64, text: &str) -> Cue {
        Cue {
            start_ms: start,
            end_ms: end,
            text: text.to_string(),
        }
    }

    fn source() -> Vec<Cue> {
        vec![cue(1000, 2000, "Hello"), cue(3000, 4500, "Bye\nnow")]
    }

    #[test]
    fn equal_times_are_accepted_unchanged() {
        let result = vec![cue(1000, 2000, "Hola"), cue(3000, 4500, "Adios\nya")];
        let a = realign(&source(), &result).unwrap();
        assert_eq!(a.cues, result);
        assert_eq!(a.realigned, 0);
    }

    #[test]
    fn a_count_mismatch_is_refused_with_both_counts() {
        let e = realign(&source(), &[cue(1000, 2000, "Hola")]).unwrap_err();
        assert_eq!(
            e,
            AlignError::CountMismatch {
                expected: 2,
                got: 1
            }
        );
        assert_eq!(e.to_string(), "result has 1 cues, the source has 2");
        assert_eq!(realign(&source(), &[]).unwrap_err(), AlignError::NoCues);
    }

    #[test]
    fn equal_count_with_other_times_takes_the_source_times() {
        let result = vec![cue(1100, 2100, "Hola"), cue(3000, 4500, "Adios")];
        let a = realign(&source(), &result).unwrap();
        assert_eq!(a.realigned, 1);
        assert_eq!((a.cues[0].start_ms, a.cues[0].end_ms), (1000, 2000));
        assert_eq!(a.cues[0].text, "Hola");
        assert_eq!(a.cues[1].text, "Adios");
        verify_same_timing(&source(), &a.cues).unwrap();
    }

    #[test]
    fn an_empty_text_for_a_non_empty_cue_is_refused() {
        let result = vec![cue(1000, 2000, "Hola"), cue(3000, 4500, "  ")];
        assert_eq!(
            realign(&source(), &result).unwrap_err(),
            AlignError::EmptyCue { number: 2 }
        );
    }

    #[test]
    fn verify_catches_any_difference() {
        let s = source();
        assert!(verify_same_timing(&s, &s).is_ok());
        let mut o = s.clone();
        o[1].end_ms += 1;
        assert_eq!(
            verify_same_timing(&s, &o).unwrap_err(),
            AlignError::TimingMismatch { number: 2 }
        );
        o.pop();
        assert!(matches!(
            verify_same_timing(&s, &o),
            Err(AlignError::CountMismatch { .. })
        ));
    }

    /// A small deterministic generator, so the property test needs no dependency
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    #[test]
    fn random_results_always_end_with_the_source_timings() {
        let mut rng = Lcg(42);
        for _ in 0..200 {
            let n = 1 + (rng.next() % 60) as usize;
            let mut t = 0;
            let src: Vec<Cue> = (0..n)
                .map(|i| {
                    t += rng.next() % 3000;
                    let len = 1 + rng.next() % 4000;
                    cue(t, t + len, &format!("line {i}"))
                })
                .collect();
            // a result with the same count and arbitrary times
            let res: Vec<Cue> = src
                .iter()
                .map(|c| {
                    cue(
                        rng.next() % 100_000,
                        rng.next() % 100_000,
                        &format!("t {}", c.text),
                    )
                })
                .collect();
            let aligned = realign(&src, &res).unwrap();
            verify_same_timing(&src, &aligned.cues).unwrap();
            // and the numbering the writer gives is 1..n in order
            let rendered = putio_sync_subtitles::srt::render(&aligned.cues);
            let again = putio_sync_subtitles::srt::parse(&rendered);
            assert_eq!(again.len(), src.len());
            verify_same_timing(&src, &again).unwrap();
        }
    }
}
