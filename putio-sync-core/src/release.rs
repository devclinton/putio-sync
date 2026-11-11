//! A rule-based reader for release names such as
//! `Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv` (feature 050).
//!
//! It reads the same fields from a video's file name and from an OpenSubtitles result's
//! `release`, so the two can be compared field by field. [`Episode`] reads the same tokens
//! with their letter case kept, for the names organising writes on Put.io (feature 023). A dozen token rules, std only; there
//! is no model and no regex crate. Anything it does not recognize is left out, never guessed.

/// Video height, normalised
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// 480p
    P480,
    /// 576p
    P576,
    /// 720p
    P720,
    /// 1080p or 1080i
    P1080,
    /// 2160p, 4K, UHD
    P2160,
}

/// Where the video came from, normalised so `WEB-DL`, `web.dl` and `WEBDL` are equal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Untouched disc copy
    Remux,
    /// Blu-ray encode (`BluRay`, `BDRip`, `BRRip`)
    Bluray,
    /// Streaming download (`WEB-DL`, `WEB`)
    WebDl,
    /// Streaming re-encode
    WebRip,
    /// Television capture
    Hdtv,
    /// DVD rip
    Dvd,
    /// HD rip
    HdRip,
    /// Camera copy
    Cam,
}

/// Video codec, normalised
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// x264, h264, AVC
    H264,
    /// x265, h265, HEVC
    H265,
    /// AV1
    Av1,
    /// XviD, DivX
    Xvid,
}

/// What a release name says
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Title words, lowercase, space separated; empty when the name starts with a marker
    pub title: String,
    /// Release year of a movie (or of a series, when the name has one)
    pub year: Option<u16>,
    /// Season number
    pub season: Option<u16>,
    /// First episode number (`S01E01E02` gives 1)
    pub episode: Option<u16>,
    /// Video height
    pub resolution: Option<Resolution>,
    /// Source
    pub source: Option<Source>,
    /// Video codec
    pub codec: Option<Codec>,
    /// Release group, lowercase
    pub group: Option<String>,
}

const EXTENSIONS: [&str; 14] = [
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "ts", "m2ts", "webm", "mpg", "mpeg", "srt", "ass",
    "sub",
];

/// Suffixes after the last `-` that belong to a source or codec word, not to a group
const NOT_GROUPS: [&str; 6] = ["dl", "rip", "ray", "hd", "dts", "ma"];

impl ReleaseInfo {
    /// Read a file name, a file stem or a release string
    pub fn parse(name: &str) -> Self {
        let name = strip_extension(name.trim());
        let (name, bracket_group) = take_leading_group(name);
        let (name, dash_group) = take_trailing_group(&name);
        let group = bracket_group.or(dash_group).map(|g| g.to_lowercase());

        let lower = name.to_lowercase();
        let tokens: Vec<&str> = lower
            .split(['.', '_', ' ', '-', '[', ']', '(', ')'])
            .filter(|t| !t.is_empty())
            .collect();

        let mut info = ReleaseInfo {
            group,
            ..Default::default()
        };
        // The index of the first marker that ends the title: season, resolution, source, codec
        let mut marker_at = tokens.len();
        let mut years: Vec<(usize, u16)> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let t = tokens[i];
            let next = tokens.get(i + 1).copied();
            let mut used = 0;
            if let Some((s, e)) = season_episode(t) {
                used = 1;
                if info.season.is_none() {
                    info.season = Some(s);
                    info.episode = e;
                }
            } else if let Some(r) = resolution(t) {
                used = 1;
                info.resolution.get_or_insert(r);
            } else if let Some((src, n)) = source(t, next) {
                used = n;
                info.source.get_or_insert(src);
            } else if let Some((c, n)) = codec(t, next) {
                used = n;
                info.codec.get_or_insert(c);
            } else if let Some(y) = year(t).filter(|_| i > 0) {
                years.push((i, y));
            }
            if used > 0 && marker_at == tokens.len() {
                marker_at = i;
            }
            i += used.max(1);
        }
        // The last year before the first marker is the release year; an earlier year-like
        // token belongs to the title (`Blade Runner 2049 2017`). A year after a marker counts
        // as the year but does not end the title.
        let mut title_end = marker_at;
        if let Some(&(at, y)) = years.iter().rev().find(|(at, _)| *at < marker_at) {
            info.year = Some(y);
            title_end = at;
        } else if let Some(&(_, y)) = years.first() {
            info.year = Some(y);
        }
        info.title = tokens[..title_end].join(" ");
        info
    }
}

fn strip_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if EXTENSIONS.contains(&ext.to_lowercase().as_str()) => stem,
        _ => name,
    }
}

/// `[Group] Title ...` gives the group and the rest
fn take_leading_group(name: &str) -> (String, Option<String>) {
    if let Some(rest) = name.strip_prefix('[') {
        if let Some((group, tail)) = rest.split_once(']') {
            let g = group.trim();
            if !g.is_empty() {
                return (tail.trim().to_string(), Some(g.to_string()));
            }
        }
    }
    (name.to_string(), None)
}

/// `... x264-GRP` (optionally followed by `[tag]`) gives the group and the rest
fn take_trailing_group(name: &str) -> (String, Option<String>) {
    let mut body = name.trim_end();
    // Trailing site tags such as `[rarbg]` or `[eztv]`
    while body.ends_with(']') {
        match body.rfind('[') {
            Some(at) if at > 0 => body = body[..at].trim_end(),
            _ => break,
        }
    }
    let Some((head, tail)) = body.rsplit_once('-') else {
        return (name.to_string(), None);
    };
    let tail = tail.trim();
    let plausible = tail.len() >= 2
        && tail.chars().all(|c| c.is_ascii_alphanumeric())
        && !NOT_GROUPS.contains(&tail.to_lowercase().as_str())
        && resolution(&tail.to_lowercase()).is_none()
        && !head.trim().is_empty()
        && !tail.contains(' ');
    if plausible {
        (head.trim().to_string(), Some(tail.to_string()))
    } else {
        (name.to_string(), None)
    }
}

/// `s01e02`, `s01e02e03`, `s01e02-e03`, `1x02`, or `s01` alone
fn season_episode(t: &str) -> Option<(u16, Option<u16>)> {
    if let Some(rest) = t.strip_prefix('s') {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if (1..=2).contains(&digits) {
            let season: u16 = rest[..digits].parse().ok()?;
            let tail = &rest[digits..];
            if tail.is_empty() {
                return Some((season, None));
            }
            if let Some(ep) = tail.strip_prefix('e') {
                let n = ep.chars().take_while(|c| c.is_ascii_digit()).count();
                if (1..=3).contains(&n) {
                    let more = &ep[n..];
                    // only further `e<digits>` groups may follow
                    if more.is_empty() || more.starts_with('e') {
                        return Some((season, ep[..n].parse().ok()));
                    }
                }
            }
        }
        return None;
    }
    let (a, b) = t.split_once('x')?;
    if (1..=2).contains(&a.len())
        && (2..=3).contains(&b.len())
        && a.chars().all(|c| c.is_ascii_digit())
        && b.chars().all(|c| c.is_ascii_digit())
    {
        return Some((a.parse().ok()?, b.parse().ok()));
    }
    None
}

fn year(t: &str) -> Option<u16> {
    if t.len() == 4 && t.chars().all(|c| c.is_ascii_digit()) {
        let y: u16 = t.parse().ok()?;
        (1900..=2099).contains(&y).then_some(y)
    } else {
        None
    }
}

fn resolution(t: &str) -> Option<Resolution> {
    Some(match t {
        "2160p" | "4k" | "uhd" => Resolution::P2160,
        "1080p" | "1080i" => Resolution::P1080,
        "720p" => Resolution::P720,
        "576p" => Resolution::P576,
        "480p" => Resolution::P480,
        _ => return None,
    })
}

/// The source and how many tokens it used (`web dl` is two)
fn source(t: &str, next: Option<&str>) -> Option<(Source, usize)> {
    Some(match t {
        "remux" => (Source::Remux, 1),
        "bluray" | "blu" | "bdrip" | "brrip" | "bd" => (Source::Bluray, 1),
        "webdl" => (Source::WebDl, 1),
        "web" if next == Some("dl") => (Source::WebDl, 2),
        "web" => (Source::WebDl, 1),
        "webrip" => (Source::WebRip, 1),
        "hdtv" | "pdtv" => (Source::Hdtv, 1),
        "dvdrip" | "dvd" => (Source::Dvd, 1),
        "hdrip" => (Source::HdRip, 1),
        "cam" | "camrip" | "hdcam" => (Source::Cam, 1),
        _ => return None,
    })
}

/// The codec and how many tokens it used (`h 264` is two)
fn codec(t: &str, next: Option<&str>) -> Option<(Codec, usize)> {
    Some(match t {
        "x264" | "h264" | "avc" => (Codec::H264, 1),
        "x265" | "h265" | "hevc" => (Codec::H265, 1),
        "h" if next == Some("264") => (Codec::H264, 2),
        "h" if next == Some("265") => (Codec::H265, 2),
        "av1" => (Codec::Av1, 1),
        "xvid" | "divx" => (Codec::Xvid, 1),
        _ => return None,
    })
}

/// What an episode's release name says, with the letter case kept (feature 023)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// The show's name as words (`Show Name`); empty when the name starts with the number
    pub show: String,
    /// The year after the show's name (`Doctor.Who.2005.S03E04`)
    pub year: Option<u16>,
    /// Season number
    pub season: u16,
    /// First episode number
    pub episode: u16,
    /// Last episode of a multi-episode file (`S01E01E02` gives 2); `episode` otherwise
    pub last_episode: u16,
    /// Episode title words before the first quality or release tag; empty when none
    pub title: String,
}

/// Release words that end an episode title and are not a resolution, source or codec
const TAGS: &[&str] = &[
    "proper",
    "repack",
    "rerip",
    "real",
    "internal",
    "extended",
    "uncut",
    "unrated",
    "multi",
    "dual",
    "subbed",
    "dubbed",
    "hdr",
    "hdr10",
    "dv",
    "dovi",
    "sdr",
    "10bit",
    "8bit",
    "imax",
    "amzn",
    "nf",
    "dsnp",
    "hmax",
    "atvp",
    "hulu",
    "pcok",
    "itunes",
    "ddp",
    "ddp5",
    "ddp2",
    "dd5",
    "dd",
    "aac",
    "aac2",
    "ac3",
    "eac3",
    "dts",
    "truehd",
    "atmos",
    "flac",
    "opus",
    "mp3",
    "web",
    "complete",
    "hybrid",
    "danish",
    "swedish",
    "norwegian",
    "finnish",
    "nordic",
    "german",
    "french",
    "truefrench",
    "spanish",
    "italian",
    "dutch",
    "polish",
    "russian",
    "japanese",
    "korean",
    "vostfr",
    "subs",
];

impl Episode {
    /// Read an episode's file name; `None` when it has no season and episode number
    pub fn parse(name: &str) -> Option<Self> {
        let words = words(name);
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        // `S01E02`, or `S01 E02` with the season and episode apart
        let (at, season, episode, mut next) = match lower
            .iter()
            .position(|t| matches!(season_episode(t), Some((_, Some(_)))))
        {
            Some(at) => {
                let (season, episode) = season_episode(&lower[at])?;
                (at, season, episode?, at + 1)
            }
            None => (0..lower.len().saturating_sub(1)).find_map(|at| {
                let (season, None) = season_episode(&lower[at])? else {
                    return None;
                };
                Some((at, season, extra_episode(&lower[at + 1])?, at + 2))
            })?,
        };
        // `s01e01e02` names more than one; so do `e02` words right after it
        let mut last = lower[at]
            .split('e')
            .skip(1)
            .filter_map(|n| n.parse::<u16>().ok())
            .last()
            .unwrap_or(episode);
        while let Some(n) = lower
            .get(next)
            .and_then(|t| extra_episode(t).or_else(|| range_end(t, episode)))
        {
            last = last.max(n);
            next += 1;
        }
        let (show, year) = show_words(&words[..at], &lower[..at]);
        let end = (next..lower.len())
            .find(|&j| is_marker(&lower[j], lower.get(j + 1).map(String::as_str)))
            .unwrap_or(lower.len());
        Some(Episode {
            show,
            year,
            season,
            episode,
            last_episode: last.max(episode),
            title: display(&words[next..end]),
        })
    }

    /// The show's name and year from a release folder's name (`Show.Name.S01.1080p.BluRay`),
    /// for files named only by number. `None` unless the name has a season or quality word
    /// after the show, so a plain `Season 1` or `Breaking Bad` folder is not read as a release.
    pub fn show_of(folder: &str) -> Option<(String, Option<u16>)> {
        let words = words(folder);
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let end = (0..lower.len()).find(|&j| {
            season_episode(&lower[j]).is_some()
                || is_marker(&lower[j], lower.get(j + 1).map(String::as_str))
        })?;
        let (show, year) = show_words(&words[..end], &lower[..end]);
        (!show.is_empty()).then_some((show, year))
    }
}

impl Episode {
    /// The show, year and season of a season-pack folder (`Show.Name.S01.1080p.BluRay-GRP`).
    /// Its show is the better name when a file inside is obfuscated or misnamed, so the
    /// organiser takes it for files of the same season.
    pub fn season_pack(folder: &str) -> Option<(String, Option<u16>, u16)> {
        let words = words(folder);
        let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let at = lower.iter().position(|t| season_episode(t).is_some())?;
        let (season, _) = season_episode(&lower[at])?;
        // `Show (2022) Season 1 S01`: the show ends before `Season 1`
        let end = lower[..at].iter().position(|t| t == "season").unwrap_or(at);
        let (show, year) = show_words(&words[..end], &lower[..end]);
        (!show.is_empty()).then_some((show, year, season))
    }
}

/// `22` in `S02E21-22`: a bare number a little after the first episode
fn range_end(t: &str, episode: u16) -> Option<u16> {
    let n: u16 = ((1..=3).contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit()))
        .then(|| t.parse().ok())
        .flatten()?;
    (n > episode && n <= episode + 9).then_some(n)
}

/// Separators between release words
const SEPARATORS: [char; 8] = ['.', '_', ' ', '-', '[', ']', '(', ')'];

/// A name's words with their case kept, without the extension and release groups. A
/// spaced ` - Title` at the end is a title, not a group.
fn words(name: &str) -> Vec<String> {
    let name = strip_extension(name.trim());
    let (name, _) = take_leading_group(name);
    let spaced = name
        .rsplit_once('-')
        .is_some_and(|(head, _)| head.ends_with(' '));
    let name = if spaced {
        name
    } else {
        take_trailing_group(&name).0
    };
    name.split(SEPARATORS)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// `e02` after an episode word
fn extra_episode(t: &str) -> Option<u16> {
    let digits = t.strip_prefix('e')?;
    ((1..=3).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// A word that ends a title: resolution, source, codec or release tag
fn is_marker(t: &str, next: Option<&str>) -> bool {
    resolution(t).is_some()
        || source(t, next).is_some()
        || codec(t, next).is_some()
        || TAGS.contains(&t)
}

/// The show's words and the year at their end, when there is more than the year
fn show_words(words: &[String], lower: &[String]) -> (String, Option<u16>) {
    match lower.last().and_then(|t| year(t)) {
        Some(y) if words.len() > 1 => (display(&words[..words.len() - 1]), Some(y)),
        _ => (display(words), None),
    }
}

/// Words joined by spaces. A name written all in lowercase gets capitals on every word;
/// otherwise each word keeps its case (`Bly Manor of`, `TNG`).
fn display(words: &[String]) -> String {
    let lowercase = !words.iter().any(|w| w.chars().any(char::is_uppercase));
    words
        .iter()
        .map(|w| {
            if !lowercase {
                return w.clone();
            }
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> ReleaseInfo {
        ReleaseInfo::parse(name)
    }

    #[test]
    fn an_episode_release_gives_every_field() {
        let r = p("Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv");
        assert_eq!(r.title, "show name");
        assert_eq!((r.season, r.episode), (Some(2), Some(5)));
        assert_eq!(r.resolution, Some(Resolution::P1080));
        assert_eq!(r.source, Some(Source::WebDl));
        assert_eq!(r.codec, Some(Codec::H264));
        assert_eq!(r.group.as_deref(), Some("grp"));
        assert_eq!(r.year, None);
    }

    #[test]
    fn a_movie_release_gives_the_year() {
        let r = p("The.Movie.Title.2019.2160p.BluRay.HEVC.DTS-HD.MA-FraMeSToR");
        assert_eq!(r.title, "the movie title");
        assert_eq!(r.year, Some(2019));
        assert_eq!(r.resolution, Some(Resolution::P2160));
        assert_eq!(r.source, Some(Source::Bluray));
        assert_eq!(r.codec, Some(Codec::H265));
        assert_eq!(r.group.as_deref(), Some("framestor"));
        assert_eq!((r.season, r.episode), (None, None));
    }

    #[test]
    fn web_dl_spellings_are_equal() {
        for n in [
            "A.Show.S01E01.WEB-DL.x264-G",
            "A Show S01E01 web dl x264-G",
            "a.show.s01e01.webdl.x264-g",
            "A.Show.S01E01.WEB.H264-G",
        ] {
            assert_eq!(p(n).source, Some(Source::WebDl), "{n}");
        }
        assert_eq!(
            p("A.Show.S01E01.WEBRip.x264-G").source,
            Some(Source::WebRip)
        );
        assert_ne!(Source::WebDl, Source::WebRip);
    }

    #[test]
    fn episode_numbering_forms() {
        assert_eq!(
            (p("Show.1x05.720p").season, p("Show.1x05.720p").episode),
            (Some(1), Some(5))
        );
        let multi = p("Show.S01E01E02.1080p.WEB-DL-GRP");
        assert_eq!((multi.season, multi.episode), (Some(1), Some(1)));
        let dash = p("Show.S01E01-E03.HDTV");
        assert_eq!((dash.season, dash.episode), (Some(1), Some(1)));
        let pack = p("Show.Name.S03.1080p.BluRay.x265-GRP");
        assert_eq!((pack.season, pack.episode), (Some(3), None));
        assert_eq!(pack.title, "show name");
        let big = p("Anime.S01E105.720p");
        assert_eq!(big.episode, Some(105));
        // `s` words and resolutions are not episodes
        assert_eq!(p("Sherlock.Holmes.1080p").season, None);
        assert_eq!(p("Show.1920x1080.mkv").season, None);
    }

    #[test]
    fn a_title_that_contains_a_year() {
        let r = p("Blade.Runner.2049.2017.1080p.BluRay.x264-GRP");
        assert_eq!(r.title, "blade runner 2049");
        assert_eq!(r.year, Some(2017));
        let r = p("2012.2009.1080p.BluRay.x264-GRP");
        assert_eq!(r.title, "2012");
        assert_eq!(r.year, Some(2009));
        let r = p("2012.mkv");
        assert_eq!(r.title, "2012");
        assert_eq!(r.year, None);
    }

    #[test]
    fn a_series_year_stays_out_of_the_title() {
        let r = p("Doctor.Who.2005.S03E04.720p.HDTV.x264-GRP");
        assert_eq!(r.title, "doctor who");
        assert_eq!(r.year, Some(2005));
        assert_eq!((r.season, r.episode), (Some(3), Some(4)));
    }

    #[test]
    fn groups_come_from_the_suffix_or_leading_brackets() {
        assert_eq!(
            p("[SubsPlease] Show - 05 (1080p).mkv").group.as_deref(),
            Some("subsplease")
        );
        assert_eq!(
            p("Show.S01E01.720p.HDTV.x264-DIMENSION[rarbg]")
                .group
                .as_deref(),
            Some("dimension")
        );
        // no group: the last hyphen belongs to a source word, or a resolution, or is too short
        assert_eq!(p("Movie.2020.1080p.WEB-DL").group, None);
        assert_eq!(p("Movie.2020.BD-Rip").group, None);
        assert_eq!(p("Movie.2020.x264-1080p").group, None);
        assert_eq!(p("Movie.Name").group, None);
    }

    #[test]
    fn codecs_and_resolutions() {
        assert_eq!(p("M.2020.H.264").codec, Some(Codec::H264));
        assert_eq!(p("M.2020.H.265").codec, Some(Codec::H265));
        assert_eq!(p("M.2020.AV1").codec, Some(Codec::Av1));
        assert_eq!(p("M.2020.XviD").codec, Some(Codec::Xvid));
        assert_eq!(p("M.2020.4K.REMUX").resolution, Some(Resolution::P2160));
        assert_eq!(p("M.2020.4K.REMUX").source, Some(Source::Remux));
        assert_eq!(p("M.2020.480p").resolution, Some(Resolution::P480));
    }

    #[test]
    fn junk_names_give_little_and_never_panic() {
        for junk in [
            "", "   ", "...", "-", "[]", "[x]", "a-b-c", "1080p", "movie", "S01E01",
        ] {
            let _ = p(junk);
        }
        assert_eq!(p("").title, "");
        assert_eq!(p("movie").title, "movie");
        assert_eq!(p("1080p").title, "");
        assert_eq!(p("S01E01").title, "");
        assert_eq!(p("S01E01").season, Some(1));
    }

    #[test]
    fn the_same_release_reads_the_same_with_other_separators() {
        let a = p("Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv");
        let b = p("Show Name S02E05 1080p WEB-DL x264-GRP");
        let c = p("show_name_s02e05_1080p_web-dl_x264-grp.srt");
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    fn ep(name: &str) -> Episode {
        Episode::parse(name).unwrap_or_else(|| panic!("{name} is an episode"))
    }

    #[test]
    fn an_episode_keeps_its_case_and_title() {
        let e = ep("Show.Name.S02E05.Title.Here.1080p.WEB-DL.x264-GRP.mkv");
        assert_eq!(e.show, "Show Name");
        assert_eq!((e.season, e.episode, e.last_episode), (2, 5, 5));
        assert_eq!(e.title, "Title Here");
        assert_eq!(e.year, None);
        let e = ep("the.office.us.s01e02.720p.hdtv.mkv");
        assert_eq!((e.show.as_str(), e.title.as_str()), ("The Office Us", ""));
        let e = ep("Show.S01E02.PROPER.AMZN.WEB-DL.DDP5.1.H.264-GRP.mkv");
        assert_eq!(e.title, "");
    }

    #[test]
    fn episode_forms_years_and_groups() {
        let e = ep("Doctor.Who.2005.S03E04.720p.HDTV.x264-GRP.mkv");
        assert_eq!((e.show.as_str(), e.year), ("Doctor Who", Some(2005)));
        let e = ep("Show.S01E01E02.Pilot.1080p.mkv");
        assert_eq!(
            (e.episode, e.last_episode, e.title.as_str()),
            (1, 2, "Pilot")
        );
        let e = ep("Show.S01E01-E03.HDTV");
        assert_eq!(e.last_episode, 3);
        let e = ep("Show.1x05.mkv");
        assert_eq!((e.season, e.episode), (1, 5));
        let e = ep("[SubsPlease] Show S02E01 (1080p).mkv");
        assert_eq!(
            (e.show.as_str(), e.season, e.title.as_str()),
            ("Show", 2, "")
        );
        let e = ep("S01E03.mkv");
        assert_eq!((e.show.as_str(), e.episode), ("", 3));
        // an organised name reads back the same
        let e = ep("Show Name - S01E02 - The Title.mkv");
        assert_eq!(
            (e.show.as_str(), e.title.as_str()),
            ("Show Name", "The Title")
        );
        // a show named by a year alone keeps it
        assert_eq!(ep("1923.S01E01.mkv").show, "1923");
    }

    #[test]
    fn non_episodes_and_release_folders() {
        assert_eq!(Episode::parse("Movie.2019.1080p.mkv"), None);
        assert_eq!(Episode::parse("Show.Name.S03.1080p"), None);
        assert_eq!(Episode::parse(""), None);
        assert_eq!(
            Episode::show_of("Show.Name.S01.1080p.BluRay.x264-GRP"),
            Some(("Show Name".to_string(), None))
        );
        assert_eq!(
            Episode::show_of("Doctor.Who.2005.S01.720p"),
            Some(("Doctor Who".to_string(), Some(2005)))
        );
        assert_eq!(Episode::show_of("Season 1"), None);
        assert_eq!(Episode::show_of("Breaking Bad"), None);
        assert_eq!(Episode::show_of("S01.1080p"), None);
    }
}
