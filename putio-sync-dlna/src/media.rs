//! Which files players see, and how they are described (feature 022).
//!
//! The extension decides the class and the mime type. No `DLNA.ORG_PN` profile is set:
//! a wrong profile makes some TVs refuse a file they could play (research R5).

/// What a media file is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Audio,
    Image,
}

impl MediaKind {
    /// The UPnP class of an item of this kind
    pub fn upnp_class(self) -> &'static str {
        match self {
            MediaKind::Video => "object.item.videoItem",
            MediaKind::Audio => "object.item.audioItem.musicTrack",
            MediaKind::Image => "object.item.imageItem.photo",
        }
    }
}

/// A recognised media file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaType {
    pub kind: MediaKind,
    pub mime: String,
}

const VIDEO: &[&str] = &[
    "mp4", "m4v", "mkv", "avi", "mov", "wmv", "mpg", "mpeg", "ts", "m2ts", "webm", "flv", "3gp",
];
const AUDIO: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav", "wma",
];
const IMAGE: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp"];

/// Suffix of a partial download (`.<name>.downloading`)
const PARTIAL: &str = ".downloading";

/// Whether a name is hidden or a partial download
pub fn is_hidden(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(PARTIAL)
}

fn mime_for(ext: &str, kind: MediaKind) -> String {
    match ext {
        "mkv" => return "video/x-matroska".into(),
        "ts" | "m2ts" => return "video/mp2t".into(),
        "m4v" => return "video/mp4".into(),
        "avi" => return "video/x-msvideo".into(),
        "flac" => return "audio/flac".into(),
        "m4a" => return "audio/mp4".into(),
        "opus" => return "audio/ogg".into(),
        _ => {}
    }
    let prefix = match kind {
        MediaKind::Video => "video/",
        MediaKind::Audio => "audio/",
        MediaKind::Image => "image/",
    };
    mime_guess::from_ext(ext)
        .iter()
        .map(|m| m.essence_str().to_string())
        .find(|m| m.starts_with(prefix))
        .unwrap_or_else(|| format!("{prefix}{ext}"))
}

/// The media class and mime of a file name, or `None` for anything players don't get
/// (hidden files, partial downloads, other extensions).
pub fn classify(name: &str) -> Option<MediaType> {
    if is_hidden(name) {
        return None;
    }
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let kind = if VIDEO.contains(&ext.as_str()) {
        MediaKind::Video
    } else if AUDIO.contains(&ext.as_str()) {
        MediaKind::Audio
    } else if IMAGE.contains(&ext.as_str()) {
        MediaKind::Image
    } else {
        return None;
    };
    Some(MediaType {
        kind,
        mime: mime_for(&ext, kind),
    })
}

/// The 4th field of a `protocolInfo`: byte-range seeking, streaming transfer
pub fn content_features() -> &'static str {
    "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000"
}

/// `http-get:*:<mime>:<features>`
pub fn protocol_info(mime: &str) -> String {
    format!("http-get:*:{mime}:{}", content_features())
}

/// The protocolInfo of every mime type served, sorted and without repeats
pub fn all_protocol_info() -> Vec<String> {
    let mut mimes: Vec<String> = Vec::new();
    for (list, kind) in [
        (VIDEO, MediaKind::Video),
        (AUDIO, MediaKind::Audio),
        (IMAGE, MediaKind::Image),
    ] {
        for ext in list {
            mimes.push(mime_for(ext, kind));
        }
    }
    mimes.sort();
    mimes.dedup();
    mimes.iter().map(|m| protocol_info(m)).collect()
}

/// Names in the order people expect on a TV: case ignored and runs of digits compared as
/// numbers, so `Episode 2` comes before `Episode 10`
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut run = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        run.push(c);
                    }
                    run
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m_trim, n_trim) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let by_value = m_trim
                    .len()
                    .cmp(&n_trim.len())
                    .then_with(|| m_trim.cmp(n_trim))
                    // `01` and `1` are equal in value; the shorter goes first
                    .then_with(|| m.len().cmp(&n.len()));
                if by_value != Ordering::Equal {
                    return by_value;
                }
            }
            (Some(c), Some(d)) => {
                if c != d {
                    return c.cmp(&d);
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension() {
        let m = classify("Movie.MKV").unwrap();
        assert_eq!(m.kind, MediaKind::Video);
        assert_eq!(m.mime, "video/x-matroska");
        assert_eq!(classify("a.mp4").unwrap().mime, "video/mp4");
        assert_eq!(classify("a.m4v").unwrap().mime, "video/mp4");
        assert_eq!(classify("a.ts").unwrap().mime, "video/mp2t");
        assert_eq!(classify("song.mp3").unwrap().kind, MediaKind::Audio);
        assert_eq!(classify("song.flac").unwrap().mime, "audio/flac");
        assert_eq!(classify("p.jpeg").unwrap().kind, MediaKind::Image);
    }

    #[test]
    fn other_hidden_and_partial_names_are_left_out() {
        for name in [
            "a.nfo",
            "b.torrent",
            "noext",
            ".hidden.mp4",
            ".x.mkv.downloading",
            "x.mkv.downloading",
            "x.srt",
        ] {
            assert!(classify(name).is_none(), "{name}");
        }
    }

    #[test]
    fn protocol_info_has_range_features_and_no_profile() {
        let p = protocol_info("video/mp4");
        assert!(p.starts_with("http-get:*:video/mp4:"));
        assert!(p.contains("DLNA.ORG_OP=01"));
        assert!(!p.contains("DLNA.ORG_PN"));
    }

    #[test]
    fn every_class_has_a_protocol_info() {
        let all = all_protocol_info();
        assert!(all.iter().any(|p| p.contains("video/x-matroska")));
        assert!(all.iter().any(|p| p.contains("audio/flac")));
        assert!(all.iter().any(|p| p.contains("image/png")));
        let mut sorted = all.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len());
    }

    #[test]
    fn natural_order_compares_numbers_by_value() {
        let mut names = vec![
            "Episode 10.mkv",
            "episode 2.mkv",
            "Episode 1.mkv",
            "Extras",
            "Episode 02.mkv",
        ];
        names.sort_by(|a, b| super::natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "Episode 1.mkv",
                "episode 2.mkv",
                "Episode 02.mkv",
                "Episode 10.mkv",
                "Extras"
            ]
        );
    }
}
