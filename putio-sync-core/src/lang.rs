//! Language codes for subtitle names (feature 025).
//!
//! One static table maps ISO 639-1, 639-2/T, 639-2/B codes and English names to a canonical
//! code: the two-letter code where one exists, else the three-letter one. The table lives in
//! `core` because the `[subtitles]` config check and the subtitle engine both need it.

/// `(639-1, 639-2/T, 639-2/B or "" when equal, English name)`. An empty 639-1 means the
/// language has none.
const TABLE: &[(&str, &str, &str, &str)] = &[
    ("af", "afr", "", "Afrikaans"),
    ("am", "amh", "", "Amharic"),
    ("ar", "ara", "", "Arabic"),
    ("as", "asm", "", "Assamese"),
    ("az", "aze", "", "Azerbaijani"),
    ("ba", "bak", "", "Bashkir"),
    ("be", "bel", "", "Belarusian"),
    ("bg", "bul", "", "Bulgarian"),
    ("bn", "ben", "", "Bengali"),
    ("bo", "bod", "tib", "Tibetan"),
    ("br", "bre", "", "Breton"),
    ("bs", "bos", "", "Bosnian"),
    ("ca", "cat", "", "Catalan"),
    ("cs", "ces", "cze", "Czech"),
    ("cy", "cym", "wel", "Welsh"),
    ("da", "dan", "", "Danish"),
    ("de", "deu", "ger", "German"),
    ("el", "ell", "gre", "Greek"),
    ("en", "eng", "", "English"),
    ("es", "spa", "", "Spanish"),
    ("et", "est", "", "Estonian"),
    ("eu", "eus", "baq", "Basque"),
    ("fa", "fas", "per", "Persian"),
    ("fi", "fin", "", "Finnish"),
    ("fo", "fao", "", "Faroese"),
    ("fr", "fra", "fre", "French"),
    ("gl", "glg", "", "Galician"),
    ("gu", "guj", "", "Gujarati"),
    ("ha", "hau", "", "Hausa"),
    ("", "haw", "", "Hawaiian"),
    ("he", "heb", "", "Hebrew"),
    ("hi", "hin", "", "Hindi"),
    ("hr", "hrv", "", "Croatian"),
    ("ht", "hat", "", "Haitian Creole"),
    ("hu", "hun", "", "Hungarian"),
    ("hy", "hye", "arm", "Armenian"),
    ("id", "ind", "", "Indonesian"),
    ("is", "isl", "ice", "Icelandic"),
    ("it", "ita", "", "Italian"),
    ("ja", "jpn", "", "Japanese"),
    ("jv", "jav", "", "Javanese"),
    ("ka", "kat", "geo", "Georgian"),
    ("kk", "kaz", "", "Kazakh"),
    ("km", "khm", "", "Khmer"),
    ("kn", "kan", "", "Kannada"),
    ("ko", "kor", "", "Korean"),
    ("la", "lat", "", "Latin"),
    ("lb", "ltz", "", "Luxembourgish"),
    ("ln", "lin", "", "Lingala"),
    ("lo", "lao", "", "Lao"),
    ("lt", "lit", "", "Lithuanian"),
    ("lv", "lav", "", "Latvian"),
    ("mg", "mlg", "", "Malagasy"),
    ("mi", "mri", "mao", "Maori"),
    ("mk", "mkd", "mac", "Macedonian"),
    ("ml", "mal", "", "Malayalam"),
    ("mn", "mon", "", "Mongolian"),
    ("mr", "mar", "", "Marathi"),
    ("ms", "msa", "may", "Malay"),
    ("mt", "mlt", "", "Maltese"),
    ("my", "mya", "bur", "Burmese"),
    ("ne", "nep", "", "Nepali"),
    ("nl", "nld", "dut", "Dutch"),
    ("nn", "nno", "", "Nynorsk"),
    ("no", "nor", "", "Norwegian"),
    ("oc", "oci", "", "Occitan"),
    ("pa", "pan", "", "Punjabi"),
    ("pl", "pol", "", "Polish"),
    ("ps", "pus", "", "Pashto"),
    ("pt", "por", "", "Portuguese"),
    ("ro", "ron", "rum", "Romanian"),
    ("ru", "rus", "", "Russian"),
    ("sa", "san", "", "Sanskrit"),
    ("sd", "snd", "", "Sindhi"),
    ("si", "sin", "", "Sinhala"),
    ("sk", "slk", "slo", "Slovak"),
    ("sl", "slv", "", "Slovenian"),
    ("sn", "sna", "", "Shona"),
    ("so", "som", "", "Somali"),
    ("sq", "sqi", "alb", "Albanian"),
    ("sr", "srp", "", "Serbian"),
    ("su", "sun", "", "Sundanese"),
    ("sv", "swe", "", "Swedish"),
    ("sw", "swa", "", "Swahili"),
    ("ta", "tam", "", "Tamil"),
    ("te", "tel", "", "Telugu"),
    ("tg", "tgk", "", "Tajik"),
    ("th", "tha", "", "Thai"),
    ("tk", "tuk", "", "Turkmen"),
    ("tl", "tgl", "", "Tagalog"),
    ("tr", "tur", "", "Turkish"),
    ("tt", "tat", "", "Tatar"),
    ("uk", "ukr", "", "Ukrainian"),
    ("ur", "urd", "", "Urdu"),
    ("uz", "uzb", "", "Uzbek"),
    ("vi", "vie", "", "Vietnamese"),
    ("yi", "yid", "", "Yiddish"),
    ("yo", "yor", "", "Yoruba"),
    ("zh", "zho", "chi", "Chinese"),
    ("", "yue", "", "Cantonese"),
];

/// Regional codes kept as written, since Radarr and Sonarr parse them
const REGIONAL: &[(&str, &str)] = &[
    ("pt-br", "Portuguese (Brazil)"),
    ("pt-pt", "Portuguese (Portugal)"),
    ("zh-cn", "Chinese (Simplified)"),
    ("zh-tw", "Chinese (Traditional)"),
];

/// Extra names that are not the table's name
const ALIASES: &[(&str, &str)] = &[
    ("castilian", "es"),
    ("flemish", "nl"),
    ("mandarin", "zh"),
    ("burmese", "my"),
    ("myanmar", "my"),
    ("moldavian", "ro"),
    ("moldovan", "ro"),
    ("valencian", "ca"),
    ("pushto", "ps"),
    ("panjabi", "pa"),
    ("letzeburgesch", "lb"),
    ("filipino", "tl"),
    ("farsi", "fa"),
    ("norwegian bokmal", "no"),
    ("norwegian nynorsk", "nn"),
    ("brazilian portuguese", "pt-br"),
    ("portuguese (brazil)", "pt-br"),
    ("simplified chinese", "zh-cn"),
    ("traditional chinese", "zh-tw"),
];

fn canonical_of(row: &(&str, &str, &str, &str)) -> String {
    if row.0.is_empty() {
        row.1.to_string()
    } else {
        row.0.to_string()
    }
}

/// The canonical code for a known language given as a code or an English name, else `None`.
///
/// Accepts ISO 639-1, 639-2/T, 639-2/B, English names (any case), and the regional forms
/// `pt-br`, `pt-pt`, `zh-cn`, `zh-tw` (`_` also works as the separator).
pub fn lookup(input: &str) -> Option<String> {
    let s = input.trim().to_lowercase().replace('_', "-");
    if s.is_empty() {
        return None;
    }
    if REGIONAL.iter().any(|(c, _)| *c == s) {
        return Some(s);
    }
    if let Some((_, c)) = ALIASES.iter().find(|(a, _)| *a == s) {
        return Some(lookup(c).unwrap_or_else(|| (*c).to_string()));
    }
    for row in TABLE {
        let hit = (!row.0.is_empty() && row.0 == s)
            || row.1 == s
            || (!row.2.is_empty() && row.2 == s)
            || row.3.to_lowercase() == s;
        if hit {
            return Some(canonical_of(row));
        }
    }
    None
}

/// Like [`lookup`], but an unknown input of two or three ASCII letters is kept lowercased and
/// anything else becomes `und`. For text a server or Put.io sent, which must not fail a step.
pub fn map_lenient(input: &str) -> String {
    if let Some(code) = lookup(input) {
        return code;
    }
    let s = input.trim().to_lowercase();
    if (s.len() == 2 || s.len() == 3) && s.chars().all(|c| c.is_ascii_lowercase()) {
        s
    } else {
        "und".to_string()
    }
}

/// The English name of a canonical code, if known
pub fn name(code: &str) -> Option<String> {
    if let Some((_, n)) = REGIONAL.iter().find(|(c, _)| *c == code) {
        return Some((*n).to_string());
    }
    TABLE
        .iter()
        .find(|row| canonical_of(row) == code)
        .map(|row| row.3.to_string())
}

/// Every known language as `(canonical code, English name)`, in table order, then the
/// regional forms
pub fn known() -> Vec<(String, String)> {
    TABLE
        .iter()
        .map(|row| (canonical_of(row), row.3.to_string()))
        .chain(
            REGIONAL
                .iter()
                .map(|(c, n)| ((*c).to_string(), (*n).to_string())),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_names_map_to_one_canonical_code() {
        let cases = [
            ("en", "en"),
            ("EN", "en"),
            ("eng", "en"),
            ("English", "en"),
            ("spa", "es"),
            ("es", "es"),
            ("fra", "fr"),
            ("fre", "fr"),
            ("French", "fr"),
            ("deu", "de"),
            ("ger", "de"),
            ("ces", "cs"),
            ("cze", "cs"),
            ("zho", "zh"),
            ("chi", "zh"),
            ("Chinese", "zh"),
            ("nld", "nl"),
            ("dut", "nl"),
            ("haw", "haw"),
            ("Hawaiian", "haw"),
            ("yue", "yue"),
            ("castilian", "es"),
            ("PT-BR", "pt-br"),
            ("pt_br", "pt-br"),
            ("pt-pt", "pt-pt"),
            ("zh-TW", "zh-tw"),
            ("Brazilian Portuguese", "pt-br"),
            ("  ita ", "it"),
        ];
        for (input, want) in cases {
            assert_eq!(lookup(input).as_deref(), Some(want), "{input}");
        }
    }

    #[test]
    fn unknown_input_is_not_a_known_language() {
        for input in ["", "xx", "klingon", "english-ish", "e"] {
            assert_eq!(lookup(input), None, "{input}");
        }
    }

    #[test]
    fn lenient_mapping_keeps_short_codes_and_gives_und_otherwise() {
        assert_eq!(map_lenient("english"), "en");
        assert_eq!(map_lenient("QQ"), "qq");
        assert_eq!(map_lenient("qqq"), "qqq");
        assert_eq!(map_lenient("klingon"), "und");
        assert_eq!(map_lenient(""), "und");
        assert_eq!(map_lenient("e1"), "und");
    }

    #[test]
    fn table_has_no_duplicate_codes_or_names() {
        let mut seen = std::collections::HashSet::new();
        for row in TABLE {
            let keys: std::collections::HashSet<String> =
                [row.0, row.1, row.2, &row.3.to_lowercase()]
                    .into_iter()
                    .filter(|k| !k.is_empty())
                    .map(String::from)
                    .collect();
            for key in keys {
                assert!(seen.insert(key.clone()), "duplicate {key}");
            }
        }
        assert!(TABLE.len() >= 90, "{}", TABLE.len());
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(name("en").as_deref(), Some("English"));
        assert_eq!(name("haw").as_deref(), Some("Hawaiian"));
        assert_eq!(name("pt-br").as_deref(), Some("Portuguese (Brazil)"));
        assert_eq!(name("zz"), None);
        for (code, n) in known() {
            assert_eq!(lookup(&code).as_deref(), Some(code.as_str()), "{code}");
            assert!(!n.is_empty());
        }
    }
}
