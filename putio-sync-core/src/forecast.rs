//! When a disk fills at the rate it has been growing (feature 064).
//!
//! The daemon records how full the download disk and the Put.io account are about once
//! an hour. [`fill`] fits a line through the last week of those readings and says when
//! it reaches the size. It is pure, and it says so when there is too little to go on or
//! the use is not growing, instead of guessing a date.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

/// Readings older than this are not looked at
pub const WINDOW_SECS: i64 = 7 * 86_400;
/// The readings must span at least this long
pub const MIN_SPAN_SECS: i64 = 86_400;
/// Fewest readings a forecast is made from
pub const MIN_SAMPLES: usize = 3;
/// A date further away than this is "not growing"
pub const MAX_DAYS: f64 = 3650.0;

/// How full something was at a time
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// Unix seconds
    pub at: i64,
    /// Bytes used
    pub used: u64,
}

/// What a forecast can say
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// It fills at `full_at`
    Filling,
    /// Too few readings, or over too short a time
    NotEnoughData,
    /// Use is flat or falling, or the date is more than ten years away
    NotGrowing,
    /// It is full now
    Full,
}

/// When something fills, or why that can't be said
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Forecast {
    /// What can be said
    pub state: State,
    /// Growth over the readings, bytes a day; negative when shrinking. `None` without
    /// enough data.
    pub rate_bytes_per_day: Option<i64>,
    /// Days until full, when `state` is `filling`
    pub days_left: Option<f64>,
    /// When it is full, RFC 3339 UTC, when `state` is `filling`
    pub full_at: Option<String>,
    /// Readings used
    pub samples: usize,
    /// The time the readings span, in hours
    pub span_hours: u32,
}

/// When `total` bytes are used, going by the `samples` of the week before `now` (Unix
/// seconds). The samples need not be sorted.
pub fn fill(samples: &[Sample], total: u64, now: i64) -> Forecast {
    let mut recent: Vec<Sample> = samples
        .iter()
        .copied()
        .filter(|s| s.at <= now && now - s.at <= WINDOW_SECS)
        .collect();
    recent.sort_by_key(|s| s.at);
    let span = match (recent.first(), recent.last()) {
        (Some(first), Some(last)) => last.at - first.at,
        _ => 0,
    };
    let mut out = Forecast {
        state: State::NotEnoughData,
        rate_bytes_per_day: None,
        days_left: None,
        full_at: None,
        samples: recent.len(),
        span_hours: u32::try_from(span / 3600).unwrap_or(u32::MAX),
    };
    let Some(latest) = recent.last().copied() else {
        return out;
    };
    if total > 0 && latest.used >= total {
        out.state = State::Full;
        return out;
    }
    if total == 0 || recent.len() < MIN_SAMPLES || span < MIN_SPAN_SECS {
        return out;
    }

    // Least squares, with times and sizes taken from their means to keep the sums small
    let n = recent.len() as f64;
    let mean_t = recent.iter().map(|s| s.at as f64).sum::<f64>() / n;
    let mean_u = recent.iter().map(|s| s.used as f64).sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for s in &recent {
        let dt = s.at as f64 - mean_t;
        num += dt * (s.used as f64 - mean_u);
        den += dt * dt;
    }
    let per_day = if den > 0.0 { num / den * 86_400.0 } else { 0.0 };
    out.rate_bytes_per_day = Some(per_day.round() as i64);
    let days = if per_day >= 1.0 {
        (total - latest.used) as f64 / per_day
    } else {
        f64::INFINITY
    };
    if days > MAX_DAYS {
        out.state = State::NotGrowing;
        return out;
    }
    out.state = State::Filling;
    out.days_left = Some((days * 10.0).round() / 10.0);
    out.full_at = DateTime::<Utc>::from_timestamp(now + (days * 86_400.0) as i64, 0)
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;
    const HOUR: i64 = 3600;
    /// 2026-11-27T00:00:00Z
    const NOW: i64 = 1_795_737_600;

    /// One reading an hour for `hours`, ending at `NOW`, growing `per_hour` bytes
    fn line(hours: i64, start: u64, per_hour: i64) -> Vec<Sample> {
        (0..=hours)
            .map(|h| Sample {
                at: NOW - (hours - h) * HOUR,
                used: (start as i64 + h * per_hour) as u64,
            })
            .collect()
    }

    #[test]
    fn steady_growth_gives_a_date() {
        // 1 GB a day from 40 GB over two days, 100 GB disk: at 42 GB, 58 days to go
        let f = fill(&line(48, 40 * GB, (GB / 24) as i64), 100 * GB, NOW);
        assert_eq!(f.state, State::Filling);
        assert_eq!(f.samples, 49);
        assert_eq!(f.span_hours, 48);
        let rate = f.rate_bytes_per_day.unwrap();
        assert!((rate - GB as i64).abs() < 1024, "{rate}");
        assert_eq!(f.days_left, Some(58.0));
        assert_eq!(f.full_at.as_deref(), Some("2027-01-24T00:00:00Z"));
    }

    #[test]
    fn too_few_or_too_close_readings_are_not_enough() {
        let none = fill(&[], 100 * GB, NOW);
        assert_eq!(none.state, State::NotEnoughData);
        assert_eq!((none.samples, none.rate_bytes_per_day), (0, None));
        // 23 hours of readings
        let short = fill(&line(23, 40 * GB, 1 << 20), 100 * GB, NOW);
        assert_eq!(short.state, State::NotEnoughData);
        assert_eq!(short.full_at, None);
        // Two readings a day apart
        let two = [
            Sample {
                at: NOW - 86_400,
                used: GB,
            },
            Sample {
                at: NOW,
                used: 2 * GB,
            },
        ];
        assert_eq!(fill(&two, 100 * GB, NOW).state, State::NotEnoughData);
        // An unknown size
        assert_eq!(
            fill(&line(48, GB, 1 << 20), 0, NOW).state,
            State::NotEnoughData
        );
    }

    #[test]
    fn flat_or_shrinking_use_is_not_growing() {
        let flat = fill(&line(48, 40 * GB, 0), 100 * GB, NOW);
        assert_eq!(flat.state, State::NotGrowing);
        assert_eq!(flat.rate_bytes_per_day, Some(0));
        assert_eq!(flat.full_at, None);
        let shrinking = fill(&line(48, 40 * GB, -(1 << 20)), 100 * GB, NOW);
        assert_eq!(shrinking.state, State::NotGrowing);
        assert!(shrinking.rate_bytes_per_day.unwrap() < 0);
        // 1 KB an hour into 60 GB free is thousands of years
        assert_eq!(
            fill(&line(48, 40 * GB, 1024), 100 * GB, NOW).state,
            State::NotGrowing
        );
    }

    #[test]
    fn full_is_said_even_with_one_reading() {
        let f = fill(
            &[Sample {
                at: NOW,
                used: 100 * GB,
            }],
            100 * GB,
            NOW,
        );
        assert_eq!(f.state, State::Full);
    }

    #[test]
    fn old_and_future_readings_are_left_out() {
        let mut samples = line(48, 40 * GB, (GB / 24) as i64);
        // A reading from a month ago at a very different level, and one from the future
        samples.push(Sample {
            at: NOW - 30 * 86_400,
            used: 99 * GB,
        });
        samples.push(Sample {
            at: NOW + HOUR,
            used: 0,
        });
        let f = fill(&samples, 100 * GB, NOW);
        assert_eq!(f.samples, 49);
        assert_eq!(f.days_left, Some(58.0));
    }

    /// A budget that deletes keeps the account level: up and down around one number
    #[test]
    fn a_sawtooth_is_not_growing() {
        let samples: Vec<Sample> = (0..=72)
            .map(|h| Sample {
                at: NOW - (72 - h) * HOUR,
                used: 80 * GB + ((h % 24) as u64) * (GB / 24),
            })
            .collect();
        let f = fill(&samples, 100 * GB, NOW);
        assert!(
            f.rate_bytes_per_day.unwrap().abs() < (GB / 8) as i64,
            "{f:?}"
        );
    }
}
