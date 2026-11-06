//! How sure we are that an OpenSubtitles result fits this video (feature 050), from 0 to 100.
//!
//! A fingerprint match is 100. An ID match is 60 on its own and gains points for each part of
//! the release name that agrees (group, source, resolution, codec). A name match earns its
//! basis from the title and the year or episode, and any disagreement there scores 0. All
//! weights are constants here; the one tunable is `min_confidence` (default 65). A result that
//! scores under it is never saved. Download count only breaks ties and never changes a score.

use crate::release::ReleaseInfo;

/// An ID match alone
pub const IDS_BASE: u8 = 60;
/// Same release group
pub const GROUP: u8 = 15;
/// Same source (`WEB-DL`, `BluRay` ...)
pub const SOURCE: u8 = 10;
/// Same resolution
pub const RESOLUTION: u8 = 10;
/// Same codec
pub const CODEC: u8 = 5;
/// Points for a perfect title match at the name level
pub const TITLE_WEIGHT: f64 = 40.0;
/// Points when year (movie) or season and episode (episode) agree at the name level
pub const AGREEMENT: u8 = 10;
/// Titles less alike than this score 0 at the name level
pub const MIN_TITLE_SIMILARITY: f64 = 0.6;
/// The most the download count adds, as a tie-break only
pub const MAX_TIEBREAK: u8 = 2;

/// Which search found the result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// File fingerprint
    Hash,
    /// IMDb or TMDb id
    Ids,
    /// Title, year, season and episode parsed from the file name
    Name,
}

impl Level {
    /// The saved-subtitle label (also the metrics `source`)
    pub fn label(self) -> &'static str {
        match self {
            Level::Hash => "os_hash",
            Level::Ids => "os_ids",
            Level::Name => "os_name",
        }
    }
}

/// A part of the match that earned points
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The service says the file fingerprint matches
    Fingerprint,
    /// IMDb or TMDb id (and season and episode) agree
    Ids,
    /// Title is alike
    Title,
    /// Year agrees
    Year,
    /// Season and episode agree
    Episode,
    /// Release group
    Group,
    /// Source
    Source,
    /// Resolution
    Resolution,
    /// Codec
    Codec,
}

impl Reason {
    /// The word shown on the step message
    pub fn word(self) -> &'static str {
        match self {
            Reason::Fingerprint => "fingerprint",
            Reason::Ids => "ids",
            Reason::Title => "title",
            Reason::Year => "year",
            Reason::Episode => "episode",
            Reason::Group => "group",
            Reason::Source => "source",
            Reason::Resolution => "resolution",
            Reason::Codec => "codec",
        }
    }
}

/// A result's score
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Score {
    /// 0 to 100
    pub total: u8,
    /// Download-count tie-break, 0 to [`MAX_TIEBREAK`]; never part of `total`
    pub tiebreak: u8,
    /// How it was found
    pub level: Level,
    /// What earned the points
    pub reasons: Vec<Reason>,
}

impl Score {
    fn zero(level: Level) -> Self {
        Self {
            total: 0,
            tiebreak: 0,
            level,
            reasons: Vec::new(),
        }
    }

    /// At or above the threshold; the tie-break is not counted
    pub fn accepts(&self, min_confidence: u8) -> bool {
        self.total >= min_confidence
    }

    /// Sort key, best last: the score, then the tie-break
    pub fn rank(&self) -> (u8, u8) {
        (self.total, self.tiebreak)
    }

    /// `80 (group, source)` for the step message and the log
    pub fn describe(&self) -> String {
        let words: Vec<&str> = self
            .reasons
            .iter()
            .filter(|r| !matches!(r, Reason::Fingerprint | Reason::Ids))
            .map(|r| r.word())
            .collect();
        if words.is_empty() {
            self.total.to_string()
        } else {
            format!("{} ({})", self.total, words.join(", "))
        }
    }
}

/// What a result says about itself, for comparing with the video
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Series or movie titles to compare with (several: the series title, the feature title
    /// and the title read from the release string)
    pub titles: Vec<String>,
    /// Feature year
    pub year: Option<u16>,
    /// Season of an episode
    pub season: Option<u16>,
    /// Episode number
    pub episode: Option<u16>,
    /// The `release` string, read
    pub release: ReleaseInfo,
    /// Times downloaded
    pub downloads: u64,
}

/// The service matched the exact file
pub fn score_hash(facts: &Facts) -> Score {
    Score {
        total: 100,
        tiebreak: tiebreak(facts.downloads),
        level: Level::Hash,
        reasons: vec![Reason::Fingerprint],
    }
}

/// The result is for the ids we searched with (and season and episode). Release parts add
/// points. A result for another season or episode scores 0.
pub fn score_ids(video: &ReleaseInfo, facts: &Facts) -> Score {
    if episode_disagrees(video, facts) {
        return Score::zero(Level::Ids);
    }
    let mut score = Score {
        total: IDS_BASE,
        tiebreak: tiebreak(facts.downloads),
        level: Level::Ids,
        reasons: vec![Reason::Ids],
    };
    add_release_parts(&mut score, video, &facts.release);
    score
}

/// The result was found by the parsed title. The basis is the title similarity and the year or
/// episode agreement; any disagreement of year, season or episode is 0.
pub fn score_name(video: &ReleaseInfo, facts: &Facts) -> Score {
    if video.title.is_empty() || episode_disagrees(video, facts) {
        return Score::zero(Level::Name);
    }
    let is_episode = video.season.is_some() && video.episode.is_some();
    if !is_episode {
        if let (Some(a), Some(b)) = (video.year, facts.year) {
            if a != b {
                return Score::zero(Level::Name);
            }
        }
    }
    let sim = facts
        .titles
        .iter()
        .map(|t| similarity(&video.title, t))
        .fold(0.0, f64::max);
    if sim < MIN_TITLE_SIMILARITY {
        return Score::zero(Level::Name);
    }
    let mut score = Score {
        total: (sim * TITLE_WEIGHT).round() as u8,
        tiebreak: tiebreak(facts.downloads),
        level: Level::Name,
        reasons: vec![Reason::Title],
    };
    if is_episode {
        if facts.season == video.season && facts.episode == video.episode {
            score.total += AGREEMENT;
            score.reasons.push(Reason::Episode);
        }
    } else if video.year.is_some() && video.year == facts.year {
        score.total += AGREEMENT;
        score.reasons.push(Reason::Year);
    }
    add_release_parts(&mut score, video, &facts.release);
    score
}

/// Season or episode known on both sides and different
fn episode_disagrees(video: &ReleaseInfo, facts: &Facts) -> bool {
    let differs = |a: Option<u16>, b: Option<u16>| matches!((a, b), (Some(a), Some(b)) if a != b);
    differs(video.season, facts.season) || differs(video.episode, facts.episode)
}

fn add_release_parts(score: &mut Score, video: &ReleaseInfo, cand: &ReleaseInfo) {
    let mut add = |same: bool, points: u8, reason: Reason| {
        if same {
            score.total = score.total.saturating_add(points).min(100);
            score.reasons.push(reason);
        }
    };
    add(
        video.group.is_some() && video.group == cand.group,
        GROUP,
        Reason::Group,
    );
    add(
        video.source.is_some() && video.source == cand.source,
        SOURCE,
        Reason::Source,
    );
    add(
        video.resolution.is_some() && video.resolution == cand.resolution,
        RESOLUTION,
        Reason::Resolution,
    );
    add(
        video.codec.is_some() && video.codec == cand.codec,
        CODEC,
        Reason::Codec,
    );
}

/// 0 to [`MAX_TIEBREAK`], one point per two orders of magnitude of downloads
fn tiebreak(downloads: u64) -> u8 {
    let magnitude = (downloads as f64 + 1.0).log10();
    ((magnitude / 2.0).floor() as u8).min(MAX_TIEBREAK)
}

fn tokens(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Dice coefficient of the title words, 0.0 to 1.0
pub fn similarity(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|t| b.contains(t)).count();
    2.0 * shared as f64 / (a.len() + b.len()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(name: &str) -> ReleaseInfo {
        ReleaseInfo::parse(name)
    }

    fn facts(release: &str) -> Facts {
        let r = ReleaseInfo::parse(release);
        Facts {
            titles: vec![r.title.clone()],
            year: r.year,
            season: r.season,
            episode: r.episode,
            release: r,
            downloads: 0,
        }
    }

    const V: &str = "Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv";

    #[test]
    fn a_fingerprint_match_is_100_whatever_the_names_say() {
        let f = facts("Totally.Different.Name");
        let s = score_hash(&f);
        assert_eq!(s.total, 100);
        assert_eq!(s.level, Level::Hash);
        assert!(s.accepts(100));
    }

    #[test]
    fn ids_alone_are_60_and_release_parts_add_up() {
        let v = video(V);
        let none = score_ids(&v, &facts("Show Name S02E05"));
        assert_eq!(none.total, 60);
        assert_eq!(none.describe(), "60");
        for (release, total) in [
            ("Show.Name.S02E05.1080p.HDTV.x265-OTHER", 70), // resolution
            ("Show.Name.S02E05.720p.WEB-DL.x265-OTHER", 70), // source
            ("Show.Name.S02E05.720p.HDTV.x264-OTHER", 65),  // codec
            ("Show.Name.S02E05.720p.HDTV.x265-GRP", 75),    // group
            ("Show.Name.S02E05.1080p.WEB-DL.x264-GRP", 100), // all four
        ] {
            assert_eq!(score_ids(&v, &facts(release)).total, total, "{release}");
        }
        assert_eq!(
            score_ids(&v, &facts("Show.Name.S02E05.1080p.HDTV.x265-GRP")).describe(),
            "85 (group, resolution)"
        );
    }

    #[test]
    fn the_default_threshold_accepts_70_and_rejects_60() {
        let v = video(V);
        let at_60 = score_ids(&v, &facts("Show.Name.S02E05.720p.HDTV.x265-OTHER"));
        let at_70 = score_ids(&v, &facts("Show.Name.S02E05.1080p.HDTV.x265-OTHER"));
        assert_eq!((at_60.total, at_70.total), (60, 70));
        assert!(!at_60.accepts(65));
        assert!(at_70.accepts(65));
        assert!(at_60.accepts(60) && at_60.accepts(0) && !at_60.accepts(61));
    }

    #[test]
    fn another_episode_scores_zero_even_with_the_same_release_parts() {
        let v = video(V);
        assert_eq!(
            score_ids(&v, &facts("Show.Name.S02E06.1080p.WEB-DL.x264-GRP")).total,
            0
        );
        assert_eq!(
            score_ids(&v, &facts("Show.Name.S03E05.1080p.WEB-DL.x264-GRP")).total,
            0
        );
    }

    #[test]
    fn the_download_count_breaks_ties_and_never_crosses_the_threshold() {
        let v = video(V);
        let mut popular = facts("Show.Name.S02E05.720p.HDTV.x265-OTHER");
        popular.downloads = 5_000_000;
        let mut unknown = popular.clone();
        unknown.downloads = 0;
        let (a, b) = (score_ids(&v, &popular), score_ids(&v, &unknown));
        assert_eq!((a.total, b.total), (60, 60));
        assert!(a.rank() > b.rank());
        assert!(a.tiebreak <= MAX_TIEBREAK);
        assert!(!a.accepts(65), "a tie-break never lifts a 60 over 65");
        assert_eq!(tiebreak(0), 0);
        assert_eq!(tiebreak(99), 1);
        assert_eq!(tiebreak(u64::MAX), MAX_TIEBREAK);
    }

    #[test]
    fn titles_are_compared_by_normalised_words() {
        assert_eq!(similarity("Show Name", "show.name"), 1.0);
        assert_eq!(similarity("show name", "name show"), 1.0);
        assert!((similarity("show name", "show name 2019") - 0.8).abs() < 1e-9);
        assert_eq!(similarity("show", "other"), 0.0);
        assert_eq!(similarity("", "x"), 0.0);
        assert_eq!(similarity("The Office!", "the office"), 1.0);
    }

    #[test]
    fn the_name_level_needs_title_agreement_and_release_parts() {
        let v = video(V);
        // exact title and episode, same source and resolution: 40 + 10 + 10 + 10 = 70
        let s = score_name(&v, &facts("Show.Name.S02E05.1080p.WEB-DL.x265-OTHER"));
        assert_eq!(s.total, 70);
        assert_eq!(s.describe(), "70 (title, episode, source, resolution)");
        assert!(s.accepts(65));
        // exact title and episode with nothing else agreeing: 50, under the threshold
        assert_eq!(score_name(&v, &facts("Show.Name.S02E05.HDTV")).total, 50);
        // a different title never passes, even with a perfect release
        let other = score_name(&v, &facts("Another.Series.S02E05.1080p.WEB-DL.x264-GRP"));
        assert_eq!(other.total, 0);
    }

    #[test]
    fn the_name_level_zeroes_on_any_disagreement() {
        let v = video(V);
        assert_eq!(
            score_name(&v, &facts("Show.Name.S02E06.1080p.WEB-DL.x264-GRP")).total,
            0
        );
        assert_eq!(
            score_name(&v, &facts("Show.Name.S01E05.1080p.WEB-DL.x264-GRP")).total,
            0
        );
        let movie = video("The.Movie.2019.1080p.BluRay.x264-GRP");
        assert!(score_name(&movie, &facts("The.Movie.2019.1080p.BluRay.x264-GRP")).total >= 90);
        assert_eq!(
            score_name(&movie, &facts("The.Movie.2018.1080p.BluRay.x264-GRP")).total,
            0
        );
        // a year missing on one side is not a disagreement, it just earns nothing
        let s = score_name(&movie, &facts("The.Movie.1080p.BluRay.x264-GRP"));
        assert_eq!(s.total, 40 + 10 + 10 + 15 + 5);
    }

    #[test]
    fn an_unparsable_name_scores_nothing_at_the_name_level() {
        assert_eq!(score_name(&video("1080p.mkv"), &facts("Anything")).total, 0);
    }

    #[test]
    fn episode_titles_can_come_from_the_series_name() {
        let v = video(V);
        let mut f = facts("Pilot.S02E05.1080p.WEB-DL.x264-GRP");
        f.titles = vec!["Pilot".into(), "Show Name".into()];
        assert!(score_name(&v, &f).accepts(65));
    }

    #[test]
    fn the_best_possible_scores() {
        let v = video("Show.Name.S02E05.1080p.WEB-DL.x264-GRP");
        let same = facts("Show.Name.S02E05.1080p.WEB-DL.x264-GRP");
        assert_eq!(score_ids(&v, &same).total, 100);
        // the name level tops out lower: 40 title + 10 episode + 40 release
        assert_eq!(score_name(&v, &same).total, 90);
    }
}
