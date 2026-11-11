//! Organising files stream mode keeps on Put.io (feature 023): the `[organise]` settings and
//! the folder and name each file should have. Pure: the download crate's organiser moves the
//! files on Put.io.
//!
//! Each kind of media has a reader and a layout: TV episodes (023) and movies (024).

use crate::error::{CoreError, CoreResult};
use crate::lang;
use crate::release::{self, Episode, Movie};
use crate::rules::sanitize_name;
use crate::sync_hooks::MAX_NAME_BYTES;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// When planned moves are made
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganiseApply {
    /// Moves wait until the operator applies them
    #[default]
    Preview,
    /// Moves are made in the sync pass that plans them
    Auto,
}

/// `[organise]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrganiseConfig {
    /// Move TV episodes into `Show/Season NN` folders
    pub tv: bool,
    /// Put.io folder for shows, from the synced folder; `""` is the synced folder itself
    pub tv_folder: String,
    /// Move movies into `Movie (Year)` folders (feature 024)
    pub movies: bool,
    /// Put.io folder for movies, from the synced folder
    pub movies_folder: String,
    /// Wait for the operator, or move at once
    pub apply: OrganiseApply,
    /// Let plugins' folder and rename rules (feature 028) set the target
    pub use_plugins: bool,
}

impl Default for OrganiseConfig {
    fn default() -> Self {
        Self {
            tv: false,
            tv_folder: "TV Shows".to_string(),
            movies: false,
            movies_folder: "Movies".to_string(),
            apply: OrganiseApply::Preview,
            use_plugins: true,
        }
    }
}

impl OrganiseConfig {
    /// Whether nothing differs from the defaults, so the section is left out of the file
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether any kind is organised
    pub fn any(&self) -> bool {
        self.tv || self.movies
    }

    /// Check `tv_folder` and `movies_folder` stay inside the synced folder
    pub fn validate(&self) -> CoreResult<()> {
        for (key, folder) in [
            ("tv_folder", &self.tv_folder),
            ("movies_folder", &self.movies_folder),
        ] {
            let folder = folder.trim();
            if folder.starts_with(['/', '\\']) {
                return Err(CoreError::ConfigError(format!(
                    "organise.{key} must be relative to the synced Put.io folder"
                )));
            }
            if folder.split(['/', '\\']).any(|p| p.trim() == "..") {
                return Err(CoreError::ConfigError(format!(
                    "organise.{key} must not contain .."
                )));
            }
        }
        Ok(())
    }
}

/// What kind of media a move is for
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrganiseKind {
    /// A TV episode: `Show/Season NN/Show - SNNEEE - Title.ext`
    Tv,
    /// A movie: `Movie (Year)/Movie (Year).ext`, extras in a subfolder
    Movie,
}

impl OrganiseKind {
    /// The stored value and the metric label
    pub fn as_str(&self) -> &'static str {
        match self {
            OrganiseKind::Tv => "tv",
            OrganiseKind::Movie => "movie",
        }
    }
}

impl fmt::Display for OrganiseKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OrganiseKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tv" => Ok(OrganiseKind::Tv),
            "movie" => Ok(OrganiseKind::Movie),
            other => Err(format!("unknown organise kind \"{other}\"")),
        }
    }
}

/// Where a file belongs: a Put.io folder from the synced folder, and a name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The kind that read the name
    pub kind: OrganiseKind,
    /// Put.io folder from the synced folder
    pub folder: String,
    /// File name in that folder
    pub name: String,
}

/// Where `name`, in Put.io folder `folder` (from the synced folder), belongs under the
/// kinds `config` turns on; `None` when no kind reads it or it is already there
pub fn plan_target(config: &OrganiseConfig, name: &str, folder: &str) -> Option<Target> {
    let tv = config
        .tv
        .then(|| tv_target(&config.tv_folder, name, folder))
        .flatten();
    // An episode is never a film, nor is anything under the shows' folder
    let target = match tv {
        Some(t) => t,
        None if config.movies && !inside(folder, &config.tv_folder) => {
            movie_target(&config.movies_folder, name, folder)?
        }
        None => return None,
    };
    (target.folder != normalize_folder(folder) || target.name != name).then_some(target)
}

/// Whether `folder` is `root` or below it; never for an empty `root`
fn inside(folder: &str, root: &str) -> bool {
    let (folder, root) = (normalize_folder(folder), normalize_folder(root));
    !root.is_empty() && (folder == root || folder.starts_with(&format!("{root}/")))
}

/// Video extensions a film may have; subtitles ([`SUBTITLES`]) go beside it
const VIDEOS: [&str; 11] = [
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "ts", "m2ts", "webm", "mpg", "mpeg",
];

/// Subtitle extensions whose language word (`.en.srt`) is kept
const SUBTITLES: [&str; 6] = ["srt", "ass", "ssa", "sub", "vtt", "idx"];

/// An episode's target under `root`: `root/Show/Season NN/Show - SNNEEE[-EEEE] - Title.ext`
pub fn tv_target(root: &str, name: &str, folder: &str) -> Option<Target> {
    let (stem, ext) = split_extension(name);
    let (stem, lang) = split_language(stem, ext);
    let episode = Episode::parse(stem)?;
    // A season-pack folder of the same season names the show best; then the file's own
    // name; then, for `S01E03.mkv`, any release folder above it
    let pack = folder
        .rsplit('/')
        .find_map(Episode::season_pack)
        .filter(|(_, _, season)| *season == episode.season)
        .map(|(show, year, _)| (show, year));
    let (show, year) = match pack {
        Some(found) => found,
        None if episode.show.is_empty() => folder.rsplit('/').find_map(Episode::show_of)?,
        None => (episode.show.clone(), episode.year),
    };
    let show = sanitize_name(&match year {
        Some(y) => format!("{show} ({y})"),
        None => show,
    });
    let mut number = format!("S{:02}E{:02}", episode.season, episode.episode);
    if episode.last_episode > episode.episode {
        number.push_str(&format!("-E{:02}", episode.last_episode));
    }
    let tail = format!("{lang}{ext}");
    let mut file = format!("{show} - {number}");
    let title = sanitize_name(&episode.title);
    if !title.is_empty() && file.len() + title.len() + 3 + tail.len() <= MAX_NAME_BYTES {
        file.push_str(" - ");
        file.push_str(&title);
    }
    file.push_str(&tail);
    Some(Target {
        kind: OrganiseKind::Tv,
        folder: join_folder(&[
            &normalize_folder(root),
            &show,
            &format!("Season {:02}", episode.season),
        ]),
        name: file,
    })
}

/// A film's target under `root`: `root/Movie (Year)/Movie (Year)[ - partN][.lang].ext`.
/// Extras keep their name in the film's extras folder (`Trailers`, `Featurettes`…); samples
/// and files in a show's folders give `None`.
pub fn movie_target(root: &str, name: &str, folder: &str) -> Option<Target> {
    let (stem, ext) = split_extension(name);
    let bare = ext.trim_start_matches('.').to_ascii_lowercase();
    if !VIDEOS.contains(&bare.as_str()) && !SUBTITLES.contains(&bare.as_str()) {
        return None;
    }
    let (stem, lang) = split_language(stem, ext);
    if folder.split('/').any(release::tv_folder_word) {
        return None;
    }
    let sample_folder = folder.split('/').any(|p| {
        let p = p.trim().to_ascii_lowercase();
        p == "sample" || p == "samples"
    });
    let movie = Movie::parse(stem)?;
    if movie.sample || sample_folder {
        return None;
    }
    // The file's own name and year; else the nearest release folder's, as for `abc123.mkv`
    let (title, year) = match movie.year {
        Some(y) if !movie.title.is_empty() => (movie.title.clone(), y),
        _ => folder.rsplit('/').find_map(Movie::of_folder)?,
    };
    let film = sanitize_name(&format!("{title} ({year})"));
    let base = join_folder(&[&normalize_folder(root), &film]);
    // The folder a file sits in names its extras kind before its own words do
    let extra = folder
        .rsplit('/')
        .next()
        .and_then(release::extras_folder_of)
        .or(movie.extra);
    let (folder, name) = match extra {
        Some(extra) => (join_folder(&[&base, extra]), sanitize_name(name)),
        None => {
            let part = movie
                .part
                .map(|n| format!(" - part{n}"))
                .unwrap_or_default();
            (base, format!("{film}{part}{lang}{ext}"))
        }
    };
    Some(Target {
        kind: OrganiseKind::Movie,
        folder,
        name,
    })
}

/// `name.ext` gives `("name", ".ext")`; a name without a short extension gives `(name, "")`
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at)
            if at > 0
                && (2..=6).contains(&(name.len() - at))
                && name[at + 1..].chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            (&name[..at], &name[at..])
        }
        _ => (name, ""),
    }
}

/// A subtitle's `Show.S01E02.en` gives `("Show.S01E02", ".en")`, so players still pair it
fn split_language<'a>(stem: &'a str, ext: &str) -> (&'a str, &'a str) {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    if !SUBTITLES.contains(&ext.as_str()) {
        return (stem, "");
    }
    match stem.rfind('.') {
        Some(at) => {
            let word = &stem[at + 1..];
            let short =
                (2..=3).contains(&word.len()) && word.chars().all(|c| c.is_ascii_alphabetic());
            if short && lang::lookup(word).is_some() {
                (&stem[..at], &stem[at..])
            } else {
                (stem, "")
            }
        }
        None => (stem, ""),
    }
}

/// A Put.io path with each part trimmed and made safe; no leading or trailing `/`
pub fn normalize_folder(folder: &str) -> String {
    folder
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != ".")
        .map(sanitize_name)
        .collect::<Vec<_>>()
        .join("/")
}

/// Join path parts, skipping empty ones
pub fn join_folder(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv(name: &str, folder: &str) -> Option<(String, String)> {
        let config = OrganiseConfig {
            tv: true,
            ..OrganiseConfig::default()
        };
        plan_target(&config, name, folder).map(|t| (t.folder, t.name))
    }

    fn is(folder: &str, name: &str) -> Option<(String, String)> {
        Some((folder.to_string(), name.to_string()))
    }

    #[test]
    fn the_naming_contract() {
        assert_eq!(
            tv(
                "Show.Name.S02E05.Title.Here.1080p.WEB-DL.x264-GRP.mkv",
                "Show.Name.S02E05.1080p.WEB-DL.x264-GRP"
            ),
            is(
                "TV Shows/Show Name/Season 02",
                "Show Name - S02E05 - Title Here.mkv"
            )
        );
        assert_eq!(
            tv("the.office.us.s01e02.720p.hdtv.mkv", ""),
            is(
                "TV Shows/The Office Us/Season 01",
                "The Office Us - S01E02.mkv"
            )
        );
        assert_eq!(
            tv("Doctor.Who.2005.S03E04.720p.HDTV.x264-GRP.mkv", ""),
            is(
                "TV Shows/Doctor Who (2005)/Season 03",
                "Doctor Who (2005) - S03E04.mkv"
            )
        );
        assert_eq!(
            tv("Show.S01E01E02.Pilot.1080p.mkv", ""),
            is("TV Shows/Show/Season 01", "Show - S01E01-E02 - Pilot.mkv")
        );
        assert_eq!(
            tv("Show.1x05.mkv", ""),
            is("TV Shows/Show/Season 01", "Show - S01E05.mkv")
        );
        assert_eq!(
            tv("S01E03.mkv", "Downloads/Show.Name.S01.1080p.BluRay-GRP"),
            is("TV Shows/Show Name/Season 01", "Show Name - S01E03.mkv")
        );
        assert_eq!(
            tv("[SubsPlease] Show S02E01 (1080p).mkv", ""),
            is("TV Shows/Show/Season 02", "Show - S02E01.mkv")
        );
        assert_eq!(
            tv("Show.S01E02.en.srt", ""),
            is("TV Shows/Show/Season 01", "Show - S01E02.en.srt")
        );
        assert_eq!(
            tv("Show.S00E01.Special.mkv", ""),
            is("TV Shows/Show/Season 00", "Show - S00E01 - Special.mkv")
        );
        assert_eq!(tv("Movie.2019.1080p.mkv", ""), None);
        assert_eq!(tv("README.txt", ""), None);
        assert_eq!(tv("S01E03.mkv", "Season 1"), None);
    }

    /// Names Sonarr and Radarr imported on a real library (feature 023 test data)
    #[test]
    fn real_library_names() {
        let cases = [
            (
                "Batman.The.Brave.and.the.Bold.S01.1080p.BluRay.DDP.2.0.x265-EDGE2020",
                "Batman.The.Brave.and.the.Bold.S01E08.Fall.of.the.Blue.Beetle.1080p.BluRay.DDP.2.0.H.265.-EDGE2020.mkv",
                "TV Shows/Batman The Brave and the Bold/Season 01",
                "Batman The Brave and the Bold - S01E08 - Fall of the Blue Beetle.mkv",
            ),
            (
                "",
                "the.pitt.s01e05.1080p.web.h264-successfulcrab[EZTVx.to].mkv",
                "TV Shows/The Pitt/Season 01",
                "The Pitt - S01E05.mkv",
            ),
            (
                "Phineas.and.Ferb.S03.1080p.DSNP.WEBRip.AAC2.0.x264-PHOENiX[rartv]",
                "Phineas.and.Ferb.S03E45E46.The.Doonkelberry.Imperative.and.Buford.Confidential.1080p.DSNP.WEB-DL.AAC.2.0.H.264-PHOENiX.mkv",
                "TV Shows/Phineas and Ferb/Season 03",
                "Phineas and Ferb - S03E45-E46 - The Doonkelberry Imperative and Buford Confidential.mkv",
            ),
            (
                "Season 06",
                "Shaun the Sheep - S06E01-02 - Baa-gherita + Get Your Goat.mkv",
                "TV Shows/Shaun the Sheep/Season 06",
                "Shaun the Sheep - S06E01-E02 - Baa gherita + Get Your Goat.mkv",
            ),
            (
                // an obfuscated file in a season pack takes the pack's show
                "The.Avengers.Earths.Mightiest.Heroes.S01.1080p.BluRay.x264-PFa[rartv]",
                "pfa-taemh.s01e17.1080p.mkv",
                "TV Shows/The Avengers Earths Mightiest Heroes/Season 01",
                "The Avengers Earths Mightiest Heroes - S01E17.mkv",
            ),
            (
                "Animal.Control.2023.S03.1080p.WEBRip.10bit.EAC3.5.1.x265-iVy",
                "Animal.Control.S03E07.Ducks.and.Penguins.1080p.WEBRip.10bit.EAC3.5.1.x265-iVy.mkv",
                "TV Shows/Animal Control (2023)/Season 03",
                "Animal Control (2023) - S03E07 - Ducks and Penguins.mkv",
            ),
            (
                "Tulsa King (2022) Season 1 S01 (1080p BluRay x265 HEVC 10bit EAC3 5.1 t3nzin)",
                "Tulsa King (2022) - S01E07 - Warr Acres (1080p BluRay x265 t3nzin).mkv",
                "TV Shows/Tulsa King (2022)/Season 01",
                "Tulsa King (2022) - S01E07 - Warr Acres.mkv",
            ),
            (
                "Justice League - Season 1",
                "Justice League ACTION - S01 E22 - The Trouble with Truth (1080p - Web-DL).mp4",
                "TV Shows/Justice League ACTION/Season 01",
                "Justice League ACTION - S01E22 - The Trouble with Truth.mp4",
            ),
            (
                "",
                "F.for.Faar.S07E19.DANiSH.1080p.WEB-DL.AAC2.0.H.264-ADDICTION.mkv",
                "TV Shows/F for Faar/Season 07",
                "F for Faar - S07E19.mkv",
            ),
            (
                "",
                "Dark Winds S03E08 Beesh Lii Iron Horse 1080p AMZN WEB-DL DDP5 1 H 264-RAWR[EZTVx.to].mkv",
                "TV Shows/Dark Winds/Season 03",
                "Dark Winds - S03E08 - Beesh Lii Iron Horse.mkv",
            ),
        ];
        for (folder, name, want_folder, want_name) in cases {
            assert_eq!(tv(name, folder), is(want_folder, want_name), "{name}");
        }
        // anime absolute numbers and bare `E33` are left alone, as are movies
        assert_eq!(
            tv(
                "[Moozzi2] Overlord S4 - 03 (BD 1920x1080 x265-10Bit Flac).mkv",
                ""
            ),
            None
        );
        assert_eq!(tv("Astro Boy (2003) E33.mkv", ""), None);
        assert_eq!(
            tv(
                "Dune.Part.Two.2024.2160p.WEB-DL.DDP5.1.Atmos.DV.HDR.H.265-FLUX.mkv",
                ""
            ),
            None
        );
    }

    fn movie(name: &str, folder: &str) -> Option<(String, String)> {
        let config = OrganiseConfig {
            movies: true,
            ..OrganiseConfig::default()
        };
        plan_target(&config, name, folder).map(|t| (t.folder, t.name))
    }

    /// contracts/naming.md of feature 024
    #[test]
    fn the_movie_naming_contract() {
        let cases = [
            (
                "",
                "Movie.Name.2019.1080p.BluRay.x264-GRP.mkv",
                "Movies/Movie Name (2019)",
                "Movie Name (2019).mkv",
            ),
            (
                "Movie.Name.2019.1080p.BluRay-GRP",
                "abc123.mkv",
                "Movies/Movie Name (2019)",
                "Movie Name (2019).mkv",
            ),
            (
                "",
                "Blade.Runner.2049.2017.2160p.mkv",
                "Movies/Blade Runner 2049 (2017)",
                "Blade Runner 2049 (2017).mkv",
            ),
            (
                "",
                "1917.2019.1080p.mkv",
                "Movies/1917 (2019)",
                "1917 (2019).mkv",
            ),
            (
                "",
                "Charlotte's.Web.2006.720p.mkv",
                "Movies/Charlotte's Web (2006)",
                "Charlotte's Web (2006).mkv",
            ),
            (
                "",
                "Movie.2019.CD2.avi",
                "Movies/Movie (2019)",
                "Movie (2019) - part2.avi",
            ),
            (
                "",
                "Movie.2019.1080p.en.srt",
                "Movies/Movie (2019)",
                "Movie (2019).en.srt",
            ),
            (
                "Movie.2019.1080p-GRP/Featurettes",
                "Making Of.mkv",
                "Movies/Movie (2019)/Featurettes",
                "Making Of.mkv",
            ),
            (
                "",
                "Movie.2019.Trailer.1080p.mkv",
                "Movies/Movie (2019)/Trailers",
                "Movie.2019.Trailer.1080p.mkv",
            ),
            (
                "Movie.2019.1080p-GRP",
                "Movie.2019.Deleted.Scenes.mkv",
                "Movies/Movie (2019)/Deleted Scenes",
                "Movie.2019.Deleted.Scenes.mkv",
            ),
            (
                "",
                "The.Interview.2014.1080p.mkv",
                "Movies/The Interview (2014)",
                "The Interview (2014).mkv",
            ),
            (
                "The.Interview.2014.1080p.BluRay-GRP",
                "abc.mkv",
                "Movies/The Interview (2014)",
                "The Interview (2014).mkv",
            ),
            (
                "Downloads/Old Name (1999)",
                "film.mkv",
                "Movies/Old Name (1999)",
                "Old Name (1999).mkv",
            ),
            (
                "",
                "Movie: Part?.2019.mkv",
                "Movies/Movie_ Part_ (2019)",
                "Movie_ Part_ (2019).mkv",
            ),
        ];
        for (folder, name, want_folder, want_name) in cases {
            assert_eq!(movie(name, folder), is(want_folder, want_name), "{name}");
        }
        for (folder, name) in [
            ("Movie.2019.1080p-GRP/Sample", "movie-sample.mkv"),
            ("", "Movie.2019.SAMPLE.mkv"),
            ("", "Show.S01E02.mkv"),
            ("", "Home Video.mp4"),
            ("Movie.2019.1080p-GRP", "movie.nfo"),
            ("Best of 2019", "clip.mp4"),
            ("Movies/Movie (2019)", "Movie (2019).mkv"),
            (
                "Movies/Movie (2019)/Trailers",
                "Movie.2019.Trailer.1080p.mkv",
            ),
            ("Show (2005)/Season 01", "featurette.mkv"),
            (
                "TV Shows/Doctor Who (2005)",
                "Doctor Who 2005 Christmas Special.mkv",
            ),
        ] {
            assert_eq!(movie(name, folder), None, "{folder}/{name}");
        }
    }

    /// Names Radarr imported on a real library (feature 024 test data). Of 400, 349 give
    /// Radarr's title and year; the rest differ by metadata (TMDB titles, years one off),
    /// none is read as an episode, and samples Radarr imported as films are left out.
    #[test]
    fn real_movie_names() {
        let cases = [
            (
                "Color.Out.of.Space.2019.BluRay.1080p.DTS.x264-PRoDJi",
                "Color.Out.of.Space.2019.BluRay.1080p.DTS.x264-PRoDJi.mkv",
                "Color Out of Space (2019)",
            ),
            (
                "Dumbo.60th.Anniversary.Edition.1941.MULTi.1080p.BluRay.x264.DTS-C4N4B1S",
                "c4n4b1s-dumbo.1080p.mkv",
                "Dumbo 60th Anniversary Edition (1941)",
            ),
            (
                "",
                "Running Man 2025 1080p WEB-DL HEVC x265 5.1 BONE.mkv",
                "Running Man (2025)",
            ),
            (
                "Ready.Or.Not.2.Here.I.Come(2026)[DUAL ESP-ENG][HDR10 HEVC][Bluray 2160p]-Mang0z4",
                "Ready.Or.Not.2.Here.I.Come(2026)[DUAL ESP-ENG][HDR10 HEVC][Bluray 2160p]-Mang0z4.mkv",
                "Ready Or Not 2 Here I Come (2026)",
            ),
            (
                "",
                "Under.the.Skin.2013.HDRip.XViD.juggs[ETRG].avi",
                "Under the Skin (2013)",
            ),
            (
                "Live Free or Die Hard (2007) 1080p AVCHD",
                "Live Free or Die Hard (2007) 1080p AVCHD.m2ts",
                "Live Free or Die Hard (2007)",
            ),
        ];
        for (folder, name, film) in cases {
            let ext = name.rsplit('.').next().unwrap();
            assert_eq!(
                movie(name, folder),
                is(&format!("Movies/{film}"), &format!("{film}.{ext}")),
                "{name}"
            );
        }
        for name in [
            "The.Prophecy.1995.1080p.BRRip.x264.AAC-m2g.(Sample).mkv",
            "The.Incredibles.2004.1080p.BluRay.AC3.sample.mkv",
            "sample.avi",
            "The Red Turtle [neo1024][1080p][85217D65].mkv",
            "Transformers 2 Revenge Of The Fallen DVDRip XviD-MAXSPEED www.torentz.3xforum.ro.avi",
        ] {
            assert_eq!(movie(name, ""), None, "{name}");
        }
    }

    #[test]
    fn with_both_kinds_an_episode_stays_tv() {
        let config = OrganiseConfig {
            tv: true,
            movies: true,
            ..OrganiseConfig::default()
        };
        let t = plan_target(&config, "Doctor.Who.2005.S03E04.720p.mkv", "").unwrap();
        assert_eq!(t.kind, OrganiseKind::Tv);
        let t = plan_target(&config, "Movie.2019.1080p.mkv", "").unwrap();
        assert_eq!(t.kind, OrganiseKind::Movie);
    }

    #[test]
    fn a_file_already_in_place_and_off_give_nothing() {
        assert_eq!(
            tv("Show - S01E02 - Pilot.mkv", "TV Shows/Show/Season 01"),
            None
        );
        let off = OrganiseConfig::default();
        assert_eq!(plan_target(&off, "Show.S01E02.mkv", ""), None);
    }

    #[test]
    fn names_are_made_safe_and_the_root_is_normalised() {
        let t = tv_target("/Media//TV/", "Show: Part.S01E02.Who?.mkv", "").unwrap();
        assert_eq!(t.folder, "Media/TV/Show_ Part/Season 01");
        assert_eq!(t.name, "Show_ Part - S01E02 - Who_.mkv");
        let t = tv_target("", "Show.S01E02.mkv", "").unwrap();
        assert_eq!(t.folder, "Show/Season 01");
    }

    #[test]
    fn settings_defaults_and_checks() {
        let c = OrganiseConfig::default();
        assert!(c.is_default() && !c.any());
        assert_eq!(c.tv_folder, "TV Shows");
        assert!(c.validate().is_ok());
        for bad in ["/TV", "TV/../x", ".."] {
            let c = OrganiseConfig {
                tv_folder: bad.to_string(),
                ..OrganiseConfig::default()
            };
            assert!(c.validate().is_err(), "{bad}");
        }
        let parsed: OrganiseConfig = toml::from_str("tv = true\napply = \"auto\"").unwrap();
        assert!(parsed.tv);
        assert_eq!(parsed.apply, OrganiseApply::Auto);
        assert_eq!(parsed.tv_folder, "TV Shows");
        assert!(toml::from_str::<OrganiseConfig>("apply = \"later\"").is_err());
        assert_eq!("tv".parse::<OrganiseKind>(), Ok(OrganiseKind::Tv));
        assert_eq!("movie".parse::<OrganiseKind>(), Ok(OrganiseKind::Movie));
        assert_eq!(c.movies_folder, "Movies");
        let c = OrganiseConfig {
            movies_folder: "../Films".to_string(),
            ..OrganiseConfig::default()
        };
        assert!(c.validate().is_err());
        let c = OrganiseConfig {
            movies: true,
            ..OrganiseConfig::default()
        };
        assert!(c.any());
    }
}
