//! Choosing the subtitle to translate from (research R10).
//!
//! Candidates are a video's main-slot sidecars in a language other than the target. The
//! pick, in order:
//!
//! 1. the first wanted language (config order) whose file was not made by translation;
//! 2. a file that was transcribed from the audio;
//! 3. any other file not made by translation, in language-code order;
//! 4. a file made by translation, only when nothing else exists.
//!
//! A file with no provenance row, or whose size differs from the recorded one, counts as
//! "not translated, not transcribed", so errors never compound through a chain of
//! translations when an original exists.

use std::path::PathBuf;

/// How much is known about where a sidecar came from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Made by this feature
    Translated,
    /// Made by the transcription server
    Transcribed,
    /// Anything else, or nothing recorded
    Other,
}

/// One sidecar that could be the source
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Canonical language code
    pub language: String,
    /// Where it is
    pub path: PathBuf,
    /// Where it came from
    pub origin: Origin,
}

/// Pick the source for `target`. `None` means "no source subtitle".
pub fn pick(candidates: &[Candidate], wanted: &[String], target: &str) -> Option<Candidate> {
    let mut usable: Vec<&Candidate> = candidates.iter().filter(|c| c.language != target).collect();
    usable.sort_by(|a, b| a.language.cmp(&b.language));

    wanted
        .iter()
        .filter(|w| w.as_str() != target)
        .find_map(|w| {
            usable
                .iter()
                .find(|c| &c.language == w && c.origin != Origin::Translated)
        })
        .or_else(|| usable.iter().find(|c| c.origin == Origin::Transcribed))
        .or_else(|| usable.iter().find(|c| c.origin != Origin::Translated))
        .or_else(|| usable.first())
        .map(|c| (*c).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(language: &str, origin: Origin) -> Candidate {
        Candidate {
            language: language.to_string(),
            path: PathBuf::from(format!("/m/v.{language}.srt")),
            origin,
        }
    }

    fn w(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|s| s.to_string()).collect()
    }

    fn lang(pick: Option<Candidate>) -> Option<String> {
        pick.map(|p| p.language)
    }

    #[test]
    fn the_first_wanted_language_present_wins() {
        let cands = [
            c("de", Origin::Other),
            c("fr", Origin::Other),
            c("en", Origin::Other),
        ];
        assert_eq!(
            lang(pick(&cands, &w(&["en", "fr", "es"]), "es")),
            Some("en".into())
        );
        assert_eq!(
            lang(pick(&cands, &w(&["fr", "en", "es"]), "es")),
            Some("fr".into())
        );
    }

    #[test]
    fn a_transcribed_file_comes_next() {
        let cands = [c("de", Origin::Other), c("ja", Origin::Transcribed)];
        assert_eq!(
            lang(pick(&cands, &w(&["en", "es"]), "es")),
            Some("ja".into())
        );
        // but a wanted language still beats it
        let cands = [c("ja", Origin::Transcribed), c("en", Origin::Other)];
        assert_eq!(
            lang(pick(&cands, &w(&["en", "es"]), "es")),
            Some("en".into())
        );
        // a transcript in a wanted language is a wanted-language file
        let cands = [c("en", Origin::Transcribed), c("fr", Origin::Other)];
        assert_eq!(
            lang(pick(&cands, &w(&["fr", "en", "es"]), "es")),
            Some("fr".into())
        );
    }

    #[test]
    fn then_any_other_file_in_language_code_order() {
        let cands = [
            c("pt", Origin::Other),
            c("de", Origin::Other),
            c("it", Origin::Other),
        ];
        assert_eq!(
            lang(pick(&cands, &w(&["en", "es"]), "es")),
            Some("de".into())
        );
    }

    #[test]
    fn a_translation_is_the_last_resort() {
        let cands = [c("de", Origin::Translated), c("fr", Origin::Translated)];
        assert_eq!(
            lang(pick(&cands, &w(&["de", "es"]), "es")),
            Some("de".into())
        );
        let cands = [c("en", Origin::Translated), c("zh", Origin::Other)];
        assert_eq!(
            lang(pick(&cands, &w(&["en", "es"]), "es")),
            Some("zh".into())
        );
    }

    #[test]
    fn the_target_language_is_never_the_source() {
        let cands = [c("es", Origin::Other)];
        assert_eq!(pick(&cands, &w(&["es"]), "es"), None);
        let cands = [c("es", Origin::Other), c("en", Origin::Other)];
        assert_eq!(
            lang(pick(&cands, &w(&["es", "en"]), "es")),
            Some("en".into())
        );
    }

    #[test]
    fn no_candidates_give_none() {
        assert_eq!(pick(&[], &w(&["en"]), "es"), None);
    }

    #[test]
    fn the_path_and_origin_come_with_the_pick() {
        let cands = [c("en", Origin::Transcribed)];
        let p = pick(&cands, &w(&["en"]), "es").unwrap();
        assert_eq!(p.path, PathBuf::from("/m/v.en.srt"));
        assert_eq!(p.origin, Origin::Transcribed);
    }
}
