//! What the Radarr/Sonarr side knows about a downloaded video (feature 050): enough IDs for
//! the subtitle matcher to search OpenSubtitles by IMDb or TMDb id.
//!
//! `core` owns the trait because the subtitles crate asks and the arr crate answers, and
//! neither may depend on the other. The daemon joins them.

use std::future::Future;
use std::pin::Pin;

/// A boxed future, so traits of this kind can be trait objects without a macro crate
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A movie or a TV episode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// Radarr's
    Movie,
    /// Sonarr's
    Episode,
}

/// What to look up
#[derive(Debug, Clone, Copy, Default)]
pub struct MediaRequest<'a> {
    /// The Put.io file id of the video
    pub putio_file_id: Option<i64>,
    /// The item's category, which names the *arr
    pub category: Option<&'a str>,
}

/// What the *arr recorded for a download
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaInfo {
    /// Movie or episode
    pub kind: Option<MediaKind>,
    /// Title, when known
    pub title: Option<String>,
    /// Release year, when known
    pub year: Option<u16>,
    /// IMDb id, digits only; the series' for an episode
    pub imdb_id: Option<String>,
    /// TMDb id; the series' for an episode
    pub tmdb_id: Option<u64>,
    /// `(season, episode)` of every episode of the download; empty for a movie
    pub episodes: Vec<(u16, u16)>,
}

impl MediaInfo {
    /// Whether there is an id to search by
    pub fn has_ids(&self) -> bool {
        self.imdb_id.is_some() || self.tmdb_id.is_some()
    }
}

/// The number of an IMDb id: `tt0903747` and `0903747` both give `903747` (the form the
/// OpenSubtitles API takes)
pub fn imdb_digits(raw: &str) -> Option<String> {
    let digits = raw.trim().trim_start_matches("tt");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let trimmed = digits.trim_start_matches('0');
    // OpenSubtitles takes the number; an all-zero id means "none"
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The IDs the *arr recorded for a download. A trait so the engine is tested with a fake.
pub trait MediaLookup: Send + Sync {
    /// `None` is "unknown" (no file id, not an *arr download, unreachable, no history); it is
    /// never an error
    fn lookup<'a>(&'a self, req: MediaRequest<'a>) -> BoxFut<'a, Option<MediaInfo>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imdb_ids_lose_the_prefix_and_leading_zeros() {
        assert_eq!(imdb_digits("tt0903747").as_deref(), Some("903747"));
        assert_eq!(imdb_digits("903747").as_deref(), Some("903747"));
        assert_eq!(imdb_digits(" tt1 ").as_deref(), Some("1"));
        for none in ["", "tt", "tt0", "0", "abc", "tt12x"] {
            assert_eq!(imdb_digits(none), None, "{none}");
        }
    }

    #[test]
    fn ids_are_detected() {
        assert!(!MediaInfo::default().has_ids());
        let m = MediaInfo {
            tmdb_id: Some(5),
            ..Default::default()
        };
        assert!(m.has_ids());
    }
}
