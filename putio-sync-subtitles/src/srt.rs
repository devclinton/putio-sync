//! SRT and WebVTT: parse leniently, render canonical SRT, turn bytes of any common encoding
//! into text.
//!
//! Canonical output is UTF-8 without BOM, LF endings, cues renumbered from 1, times as
//! `HH:MM:SS,mmm`.

use thiserror::Error;

/// One subtitle cue
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    /// Start, milliseconds
    pub start_ms: u64,
    /// End, milliseconds
    pub end_ms: u64,
    /// Text, lines joined with `\n`
    pub text: String,
}

/// Why a subtitle could not be used
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SrtError {
    /// Zero bytes, or only whitespace
    #[error("subtitle is empty")]
    Empty,
    /// Text decoded but holds no cues
    #[error("no subtitle cues found")]
    NoCues,
    /// Too many characters could not be decoded
    #[error("subtitle text is not readable in any supported encoding")]
    BadEncoding,
}

/// More than this share of replacement characters means the encoding guess was wrong
const MAX_REPLACEMENT_SHARE: f64 = 0.05;

/// `HH:MM:SS,mmm`
pub fn format_time(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms / 60_000) % 60;
    let s = (ms / 1000) % 60;
    format!("{h:02}:{m:02}:{s:02},{:03}", ms % 1000)
}

/// Parse `HH:MM:SS,mmm`, `HH:MM:SS.mmm` or `MM:SS.mmm` (WebVTT)
pub fn parse_time(token: &str) -> Option<u64> {
    let token = token.trim();
    let (clock, frac) = match token.find([',', '.']) {
        Some(i) => (&token[..i], &token[i + 1..]),
        None => (token, ""),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    let nums: Vec<u64> = parts
        .iter()
        .map(|p| p.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (h, m, s) = match nums.as_slice() {
        [h, m, s] => (*h, *m, *s),
        [m, s] => (0, *m, *s),
        _ => return None,
    };
    if !frac.chars().all(|c| c.is_ascii_digit()) || frac.len() > 9 {
        return None;
    }
    // ".5" is 500 ms; extra digits are cut
    let mut digits: String = frac.chars().take(3).collect();
    while digits.len() < 3 {
        digits.push('0');
    }
    let ms: u64 = digits.parse().ok()?;
    Some(h * 3_600_000 + m * 60_000 + s * 1000 + ms)
}

/// Parse SRT or WebVTT text. Never fails: blocks without a time range are skipped.
pub fn parse(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}');
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(first) = lines.iter().position(|l| !l.trim().is_empty()) else {
            continue;
        };
        let head = lines[first].trim();
        if head.starts_with("WEBVTT")
            || head.starts_with("NOTE")
            || head.starts_with("STYLE")
            || head.starts_with("REGION")
        {
            continue;
        }
        let Some(t) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let Some((a, b)) = lines[t].split_once("-->") else {
            continue;
        };
        // cue settings follow the end time after whitespace
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start_ms), Some(end_ms)) = (parse_time(a), parse_time(b)) else {
            continue;
        };
        let body = lines[t + 1..]
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        let body = body.trim().to_string();
        if body.is_empty() {
            continue;
        }
        cues.push(Cue {
            start_ms,
            end_ms,
            text: body,
        });
    }
    cues
}

/// Render canonical SRT
pub fn render(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_time(c.start_ms),
            format_time(c.end_ms),
            c.text
        ));
    }
    out
}

/// The legacy code page to try for a language when the bytes are not UTF-8
fn legacy_encoding(language: &str) -> &'static encoding_rs::Encoding {
    use encoding_rs::*;
    match language {
        "pl" | "cs" | "sk" | "hu" | "ro" | "hr" | "sl" | "bs" | "sq" => WINDOWS_1250,
        "ru" | "bg" | "uk" | "be" | "sr" | "mk" | "kk" | "tt" | "tg" => WINDOWS_1251,
        "el" => WINDOWS_1253,
        "tr" | "az" => WINDOWS_1254,
        "he" | "yi" => WINDOWS_1255,
        "ar" | "fa" | "ur" | "ps" | "sd" => WINDOWS_1256,
        "et" | "lv" | "lt" => WINDOWS_1257,
        "vi" => WINDOWS_1258,
        "th" => WINDOWS_874,
        "ja" => SHIFT_JIS,
        "ko" => EUC_KR,
        "zh" | "zh-cn" => GBK,
        "zh-tw" | "yue" => BIG5,
        _ => WINDOWS_1252,
    }
}

/// Bytes to text: strict UTF-8 (BOM stripped), else UTF-16 by BOM, else the language's legacy
/// code page, else Windows-1252.
pub fn decode_bytes(bytes: &[u8], language: &str) -> Result<String, SrtError> {
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(SrtError::Empty);
    }
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        encoding_rs::UTF_16LE
            .decode_without_bom_handling(&bytes[2..])
            .0
            .into_owned()
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        encoding_rs::UTF_16BE
            .decode_without_bom_handling(&bytes[2..])
            .0
            .into_owned()
    } else if let Ok(s) = std::str::from_utf8(bytes) {
        s.to_string()
    } else {
        legacy_encoding(language)
            .decode_without_bom_handling(bytes)
            .0
            .into_owned()
    };
    let total = text.chars().count().max(1);
    let bad = text.chars().filter(|c| *c == '\u{fffd}').count();
    if bad as f64 / total as f64 > MAX_REPLACEMENT_SHARE {
        return Err(SrtError::BadEncoding);
    }
    Ok(text)
}

/// Decode, parse (SRT or VTT) and render canonical SRT. No cues is an error, so the next
/// candidate is tried.
pub fn convert(bytes: &[u8], language: &str) -> Result<String, SrtError> {
    let text = decode_bytes(bytes, language)?;
    let cues = parse(&text);
    if cues.is_empty() {
        return Err(SrtError::NoCues);
    }
    Ok(render(&cues))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str =
        "1\n00:00:01,000 --> 00:00:02,500\nHello\n\n2\n00:01:00,5 --> 01:00:00,000\nTwo\nlines\n\n";

    #[test]
    fn parses_srt() {
        let cues = parse(SRT);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 2500);
        assert_eq!(cues[1].start_ms, 60_500);
        assert_eq!(cues[1].end_ms, 3_600_000);
        assert_eq!(cues[1].text, "Two\nlines");
    }

    #[test]
    fn parses_vtt_with_headers_notes_settings_and_short_times() {
        let vtt = "WEBVTT\nKind: captions\n\nNOTE a comment\nmore\n\nSTYLE\n::cue {}\n\nintro\n00:01.000 --> 00:03.250 align:start position:0%\n<i>Hi</i>\n\n01:00:00.000 --> 01:00:01.000\nLater\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 2, "{cues:?}");
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 3250);
        assert_eq!(cues[0].text, "<i>Hi</i>");
        assert_eq!(cues[1].start_ms, 3_600_000);
    }

    #[test]
    fn tolerates_bom_crlf_and_missing_numbers() {
        let raw = "\u{feff}00:00:01,000 --> 00:00:02,000\r\nA\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nB\r\n";
        let cues = parse(raw);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "A");
        assert_eq!(cues[1].text, "B");
    }

    #[test]
    fn render_is_canonical() {
        let out = render(&parse("\u{feff}WEBVTT\r\n\r\n00:01.5 --> 00:02.0\r\nX\r\n"));
        assert_eq!(out, "1\n00:00:01,500 --> 00:00:02,000\nX\n\n");
    }

    #[test]
    fn times_round_trip() {
        for ms in [0, 1, 999, 1000, 61_001, 3_661_001, 36_000_000] {
            assert_eq!(parse_time(&format_time(ms)), Some(ms));
        }
        assert_eq!(parse_time("nope"), None);
        assert_eq!(parse_time("1:2:3:4"), None);
        assert_eq!(parse_time("00:00:01,x"), None);
    }

    #[test]
    fn utf8_bom_is_stripped_and_utf16_is_read() {
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(SRT.as_bytes());
        assert_eq!(decode_bytes(&bom, "en").unwrap(), SRT);

        let mut le = vec![0xFF, 0xFE];
        for u in SRT.encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_bytes(&le, "en").unwrap(), SRT);

        let mut be = vec![0xFE, 0xFF];
        for u in SRT.encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode_bytes(&be, "en").unwrap(), SRT);
    }

    #[test]
    fn latin1_and_language_code_pages() {
        // "café" in Windows-1252
        let latin = b"1\n00:00:01,000 --> 00:00:02,000\ncaf\xe9\n";
        assert!(convert(latin, "fr").unwrap().contains("café"));
        assert!(convert(latin, "en").unwrap().contains("café"));
        // Cyrillic "Привет" in Windows-1251
        let (cp1251, _, _) = encoding_rs::WINDOWS_1251.encode("Привет");
        let mut ru = b"1\n00:00:01,000 --> 00:00:02,000\n".to_vec();
        ru.extend_from_slice(&cp1251);
        assert!(convert(&ru, "ru").unwrap().contains("Привет"));
        // Polish "żółć" in Windows-1250
        let (cp1250, _, _) = encoding_rs::WINDOWS_1250.encode("żółć");
        let mut pl = b"1\n00:00:01,000 --> 00:00:02,000\n".to_vec();
        pl.extend_from_slice(&cp1250);
        assert!(convert(&pl, "pl").unwrap().contains("żółć"));
    }

    #[test]
    fn bad_output_is_detected() {
        assert_eq!(convert(b"", "en"), Err(SrtError::Empty));
        assert_eq!(convert(b" \n\r\n ", "en"), Err(SrtError::Empty));
        assert_eq!(
            convert(b"<html>not a subtitle</html>", "en"),
            Err(SrtError::NoCues)
        );
        // mostly undecodable UTF-8-looking garbage
        let junk: Vec<u8> = "\u{fffd}".repeat(200).into_bytes();
        assert_eq!(decode_bytes(&junk, "en"), Err(SrtError::BadEncoding));
    }

    #[test]
    fn convert_turns_vtt_into_srt() {
        let out = convert(b"WEBVTT\n\n00:01.000 --> 00:02.000\nHi\n", "en").unwrap();
        assert_eq!(out, "1\n00:00:01,000 --> 00:00:02,000\nHi\n\n");
    }
}
