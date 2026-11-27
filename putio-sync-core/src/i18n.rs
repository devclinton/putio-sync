//! Languages for text shown to a person (feature 060).
//!
//! The English text in the code is the catalog key, as in the web UI (feature 051). One
//! language is chosen per run, before the arguments are parsed: `--lang`, then
//! `PUTIO_SYNC_LANG`, then `[ui] language` in the config file, then the system locale, then
//! English. [`t!`](crate::t) gives text for the terminal in that language; [`msg!`](crate::msg)
//! gives a [`Message`] that stays English wherever it is stored or sent (JSON, the API, logs)
//! and is translated only when shown.
//!
//! Data is never translated: names, paths and error text from Put.io or the system go in as
//! arguments.

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::sync::OnceLock;

/// A language the CLI can show
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lang {
    /// `en`, or a code from the web UI's `LANGUAGES`
    pub code: &'static str,
    /// The language's own name
    pub name: &'static str,
    /// Written right to left. Informational: text is printed in logical order.
    pub rtl: bool,
}

const fn lang(code: &'static str, name: &'static str) -> Lang {
    Lang {
        code,
        name,
        rtl: false,
    }
}

const fn rtl(code: &'static str, name: &'static str) -> Lang {
    Lang {
        code,
        name,
        rtl: true,
    }
}

/// English
pub const EN: Lang = lang("en", "English");

/// English first, then the web UI's languages in its order
pub const LANGS: &[Lang] = &[
    EN,
    rtl("ar", "العربية"),
    lang("da", "Dansk"),
    lang("de", "Deutsch"),
    lang("es", "Español"),
    lang("fi", "Suomi"),
    lang("fr", "Français"),
    rtl("he", "עברית"),
    lang("hi", "हिन्दी"),
    lang("id", "Bahasa Indonesia"),
    lang("it", "Italiano"),
    lang("ja", "日本語"),
    lang("ko", "한국어"),
    lang("ms", "Bahasa Melayu"),
    lang("nb", "Norsk bokmål"),
    lang("nl", "Nederlands"),
    lang("pl", "Polski"),
    lang("pt", "Português"),
    lang("pt-PT", "Português (Portugal)"),
    lang("ru", "Русский"),
    lang("sv", "Svenska"),
    lang("th", "ไทย"),
    lang("tr", "Türkçe"),
    lang("uk", "Українська"),
    lang("vi", "Tiếng Việt"),
    lang("zh", "中文"),
    lang("zh-TW", "繁體中文"),
];

/// What a language value turned out to be
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// A language this build knows
    Lang(Lang),
    /// `C`, `POSIX` or empty: no language asked for
    None,
    /// Names no language we have
    Unknown,
}

impl Lang {
    /// Match a code or a locale name: case-insensitive, `_` as `-`, without `.encoding` and
    /// `@modifier`. The exact code first (`pt_PT` → `pt-PT`), then its base (`pt_BR` → `pt`).
    pub fn find(input: &str) -> Found {
        let cut = input.find(['.', '@']).unwrap_or(input.len());
        let wanted = input[..cut].trim().to_lowercase().replace('_', "-");
        if wanted.is_empty() || wanted == "c" || wanted == "posix" {
            return Found::None;
        }
        let by_code = |code: &str| LANGS.iter().find(|l| l.code.to_lowercase() == code);
        let base = wanted.split('-').next().unwrap_or(&wanted);
        match by_code(&wanted).or_else(|| by_code(base)) {
            Some(lang) => Found::Lang(*lang),
            None => Found::Unknown,
        }
    }

    /// The language for a code, when we have it
    pub fn get(input: &str) -> Option<Lang> {
        match Self::find(input) {
            Found::Lang(lang) => Some(lang),
            _ => None,
        }
    }
}

/// Where the run's language came from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `--lang`
    Flag,
    /// `PUTIO_SYNC_LANG`
    Env,
    /// `[ui] language` in the config file
    Config,
    /// The system locale
    Os,
    /// Nothing chose one: English
    Default,
}

impl Source {
    /// The word used in `languages --json`
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Flag => "flag",
            Source::Env => "env",
            Source::Config => "config",
            Source::Os => "os",
            Source::Default => "default",
        }
    }
}

/// The language in effect for one run
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The language of this run
    pub lang: Lang,
    /// Where the choice came from
    pub source: Source,
    /// The locale variable that matched, when the source is [`Source::Os`]
    pub os_variable: Option<&'static str>,
    /// Values from the flag, `PUTIO_SYNC_LANG` or the config that name no language we have
    pub warnings: Vec<String>,
    /// A language was picked, but this build has English only
    pub english_only: bool,
}

impl Default for Choice {
    fn default() -> Self {
        Self {
            lang: EN,
            source: Source::Default,
            os_variable: None,
            warnings: Vec::new(),
            english_only: false,
        }
    }
}

/// The locale variables, in the order they are read
pub const OS_VARIABLES: [&str; 4] = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];

/// `--lang CODE` or `--lang=CODE`, anywhere in the arguments
fn flag_value(args: &[String]) -> Option<String> {
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--lang" {
            return args.next().cloned();
        }
        if let Some(value) = arg.strip_prefix("--lang=") {
            return Some(value.to_string());
        }
    }
    None
}

/// The config path in the arguments: `-c PATH`, `--config PATH`, `--config=PATH`, `-cPATH`
pub fn config_arg(args: &[String]) -> Option<String> {
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "-c" || arg == "--config" {
            return args.next().cloned();
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            return Some(value.to_string());
        }
        if let Some(value) = arg.strip_prefix("-c").filter(|v| !v.is_empty()) {
            if !arg.starts_with("--") {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Pick the run's language.
///
/// `env` reads an environment variable, `config_language` gives `[ui] language` from the
/// config file (see [`config_language`]) and `os_locales` the system's preferred languages
/// (asked only when none of the locale variables is set). They are parameters so the order
/// can be tested without touching the process.
pub fn select(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
    config_language: &dyn Fn() -> Option<String>,
    os_locales: &dyn Fn() -> Vec<String>,
) -> Choice {
    let mut choice = Choice::default();
    let explicit = [
        (Source::Flag, flag_value(args)),
        (Source::Env, env("PUTIO_SYNC_LANG")),
        (Source::Config, config_language()),
    ];
    for (source, value) in explicit {
        let Some(value) = value else { continue };
        match Lang::find(&value) {
            Found::Lang(lang) => {
                choice.lang = lang;
                choice.source = source;
                return choice;
            }
            Found::None => {}
            Found::Unknown => choice.warnings.push(value),
        }
    }

    let mut any_set = false;
    for variable in OS_VARIABLES {
        let Some(value) = env(variable).filter(|v| !v.is_empty()) else {
            continue;
        };
        any_set = true;
        // LANGUAGE is a colon-separated list of preferences; the others hold one locale
        let entries: Vec<&str> = if variable == "LANGUAGE" {
            value.split(':').collect()
        } else {
            vec![value.as_str()]
        };
        for entry in entries {
            match Lang::find(entry) {
                Found::Lang(lang) => {
                    choice.lang = lang;
                    choice.source = Source::Os;
                    choice.os_variable = Some(variable);
                    return choice;
                }
                // C and POSIX ask for no language: stop here, in English
                Found::None if variable != "LANGUAGE" => return choice,
                _ => {}
            }
        }
    }
    if !any_set {
        for locale in os_locales() {
            if let Found::Lang(lang) = Lang::find(&locale) {
                choice.lang = lang;
                choice.source = Source::Os;
                return choice;
            }
        }
    }
    choice
}

/// `[ui] language` from the config file the run will use: the path in the arguments, then
/// `PUTIO_SYNC_CONFIG`, then the default path. Only the `[ui]` table is read, and a file
/// that is missing or does not parse gives `None`: the full load reports that.
pub fn config_language(args: &[String], env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Only {
        ui: Option<crate::config::UiConfig>,
    }
    let path = config_arg(args)
        .or_else(|| env("PUTIO_SYNC_CONFIG"))
        .map(std::path::PathBuf::from)
        .or_else(|| crate::config::Config::default_path().ok())?;
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str::<Only>(&text).ok()?.ui?.language
}

/// The system's preferred languages, most wanted first
pub fn os_locales() -> Vec<String> {
    sys_locale::get_locales().collect()
}

static CHOICE: OnceLock<Choice> = OnceLock::new();
static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Record the run's language. Only the first call counts.
pub fn set_choice(choice: Choice) {
    let _ = CHOICE.set(choice);
}

/// The run's language: English until [`set_choice`] is called
pub fn current() -> &'static Choice {
    static ENGLISH: OnceLock<Choice> = OnceLock::new();
    CHOICE
        .get()
        .unwrap_or_else(|| ENGLISH.get_or_init(Choice::default))
}

/// Load the chosen language's catalog (English text → translation). Only the first call
/// counts. Entries whose placeholders differ from their key's are dropped, so that text
/// stays English instead of losing a value.
pub fn set_catalog(entries: HashMap<String, String>) {
    let entries = entries
        .into_iter()
        .filter(|(key, value)| placeholder_set(key) == placeholder_set(value))
        .collect();
    let _ = CATALOG.set(entries);
}

/// `template` in the run's language; the English itself when there is no translation
pub fn tr(template: &str) -> &str {
    CATALOG
        .get()
        .and_then(|catalog| catalog.get(template))
        .map(String::as_str)
        .unwrap_or(template)
}

/// The numbers of the `{N}` placeholders in `text`, sorted, each once
pub fn placeholder_set(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('}') else { break };
        let inside = &rest[..close];
        if !inside.is_empty() && inside.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = inside.parse() {
                if !found.contains(&n) {
                    found.push(n);
                }
            }
        }
    }
    found.sort_unstable();
    found
}

/// How many arguments a template needs: one more than its highest `{N}`. `const`, so the
/// macros can check the count when the code is built.
pub const fn placeholders(template: &str) -> usize {
    let bytes = template.as_bytes();
    let mut needed = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut j = i + 1;
            let mut n = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                n = n * 10 + (bytes[j] - b'0') as usize;
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b'}' && n + 1 > needed {
                needed = n + 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    needed
}

/// `template` with each `{N}` replaced by `args[N]`. Other braces are left alone.
pub fn fill(template: &str, args: &[String]) -> String {
    if args.is_empty() {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let arg = after[..digits]
            .parse::<usize>()
            .ok()
            .and_then(|n| args.get(n));
        match arg {
            Some(arg) if after[digits..].starts_with('}') => {
                out.push_str(arg);
                rest = &after[digits + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `template` in the run's language with its arguments filled in. Use [`t!`](crate::t).
pub fn translate(template: &str, args: &[String]) -> String {
    fill(tr(template), args)
}

/// Text that is English when stored or sent, and in the run's language when shown.
///
/// It reads as its English text (`Display`, `Deref<Target = str>`), so code that stores,
/// compares or logs it keeps working in English. Only [`Message::localized`] translates.
/// It is deliberately not `Serialize`: store `to_string()` where English is wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    english: String,
    /// The catalog key and its arguments; `None` for text from elsewhere (an error chain, a
    /// plugin), which is shown as it is in every language
    template: Option<(&'static str, Vec<String>)>,
}

impl Message {
    /// A message from an English template. Use [`msg!`](crate::msg).
    pub fn template(template: &'static str, args: Vec<String>) -> Self {
        Self {
            english: fill(template, &args),
            template: Some((template, args)),
        }
    }

    /// Text that is the same in every language
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            english: text.into(),
            template: None,
        }
    }

    /// The text in the run's language
    pub fn localized(&self) -> String {
        match &self.template {
            Some((template, args)) => translate(template, args),
            None => self.english.clone(),
        }
    }

    /// The English text
    pub fn english(&self) -> &str {
        &self.english
    }

    /// The catalog key, for a message built by [`msg!`](crate::msg)
    pub fn key(&self) -> Option<&'static str> {
        self.template.as_ref().map(|(template, _)| *template)
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.english)
    }
}

impl Deref for Message {
    type Target = str;

    fn deref(&self) -> &str {
        &self.english
    }
}

impl PartialEq<&str> for Message {
    fn eq(&self, other: &&str) -> bool {
        self.english() == *other
    }
}

impl PartialEq<str> for Message {
    fn eq(&self, other: &str) -> bool {
        self.english() == other
    }
}

impl From<String> for Message {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl From<&str> for Message {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<Message> for String {
    fn from(message: Message) -> Self {
        message.english
    }
}

/// Text for the terminal, in the run's language: `t!("Deleted {0} of {1}", done, total)`.
///
/// The first argument is the English text and the catalog key. Values go in as `{0}`, `{1}`
/// in argument order, and the number of arguments is checked when the code is built. Never
/// use it for JSON, CSV or log lines.
#[macro_export]
macro_rules! t {
    ($template:literal $(, $arg:expr)* $(,)?) => {{
        const _: () = assert!(
            $crate::i18n::placeholders($template) == 0usize $(+ { let _ = stringify!($arg); 1usize })*,
            "t!: the number of numbered placeholders must equal the number of arguments"
        );
        $crate::i18n::translate($template, &[$(::std::string::ToString::to_string(&$arg)),*])
    }};
}

/// A [`Message`](crate::i18n::Message): English where it is stored, translated where it is
/// shown. Same rules as [`t!`](crate::t).
#[macro_export]
macro_rules! msg {
    ($template:literal $(, $arg:expr)* $(,)?) => {{
        const _: () = assert!(
            $crate::i18n::placeholders($template) == 0usize $(+ { let _ = stringify!($arg); 1usize })*,
            "msg!: the number of numbered placeholders must equal the number of arguments"
        );
        $crate::i18n::Message::template(
            $template,
            ::std::vec![$(::std::string::ToString::to_string(&$arg)),*],
        )
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(input: &str) -> Option<&'static str> {
        Lang::get(input).map(|l| l.code)
    }

    #[test]
    fn find_matches_codes_and_locale_names() {
        assert_eq!(code("de"), Some("de"));
        assert_eq!(code("DE"), Some("de"));
        assert_eq!(code("de_DE.UTF-8@euro"), Some("de"));
        assert_eq!(code("pt_PT"), Some("pt-PT"));
        assert_eq!(code("pt_PT.UTF-8"), Some("pt-PT"));
        assert_eq!(code("zh-tw"), Some("zh-TW"));
        assert_eq!(code("pt-BR"), Some("pt"));
        assert_eq!(code("zh_HK"), Some("zh"));
        assert_eq!(code("en_US.UTF-8"), Some("en"));
        for none in ["C", "POSIX", "C.UTF-8", "", "  "] {
            assert_eq!(Lang::find(none), Found::None, "{none:?}");
        }
        assert_eq!(Lang::find("xx"), Found::Unknown);
        assert_eq!(Lang::find("xx_YY.UTF-8"), Found::Unknown);
    }

    #[test]
    fn the_list_is_the_web_ui_list_plus_english() {
        let js = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../putio-sync-api/ui/i18n.js"
        ))
        .unwrap();
        let list = js.split("LANGUAGES = [").nth(1).unwrap();
        let list = &list[..list.find(']').unwrap()];
        let mut web: Vec<&str> = list
            .split(',')
            .map(|c| c.trim().trim_matches('"'))
            .collect();
        web.insert(0, "en");
        let ours: Vec<&str> = LANGS.iter().map(|l| l.code).collect();
        assert_eq!(ours, web);
        let rtl: Vec<&str> = LANGS.iter().filter(|l| l.rtl).map(|l| l.code).collect();
        assert_eq!(rtl, ["ar", "he"]);
    }

    #[test]
    fn fill_and_placeholders() {
        let args = ["a".to_string(), "b".to_string()];
        assert_eq!(fill("{0} of {1}", &args), "a of b");
        assert_eq!(fill("{1} then {0} and {0}", &args), "b then a and a");
        assert_eq!(fill("{bytes} {0} {7} {", &args), "{bytes} a {7} {");
        assert_eq!(placeholders("{0} of {1}"), 2);
        assert_eq!(placeholders("{bytes} plain {}"), 0);
        assert_eq!(placeholder_set("{1} {0} {1} {x}"), [0, 1]);
    }

    #[test]
    fn macros_fill_in_english() {
        assert_eq!(t!("No files found."), "No files found.");
        assert_eq!(t!("Deleted {0} of {1}", 2, 3), "Deleted 2 of 3");
        let m = msg!("{0} is free of {1}", "1 GB", "2 GB");
        assert_eq!(m.to_string(), "1 GB is free of 2 GB");
        assert_eq!(m.key(), Some("{0} is free of {1}"));
        assert!(m.contains("free of"));
        assert_eq!(m.localized(), "1 GB is free of 2 GB");
        let text: Message = String::from("as it is {0}").into();
        assert_eq!(text.localized(), "as it is {0}");
        assert_eq!(text.key(), None);
    }

    #[test]
    fn a_translation_with_other_placeholders_is_dropped() {
        // The catalog is process-wide, so this is the one test that loads it
        set_catalog(HashMap::from([
            ("Good {0} {1}".to_string(), "{1} {0} gut".to_string()),
            ("Bad {0} {1}".to_string(), "schlecht {0}".to_string()),
            ("Plain".to_string(), "Einfach".to_string()),
        ]));
        assert_eq!(t!("Good {0} {1}", "a", "b"), "b a gut");
        assert_eq!(t!("Bad {0} {1}", "a", "b"), "Bad a b");
        assert_eq!(tr("Plain"), "Einfach");
        assert_eq!(tr("Missing"), "Missing");
        let m = msg!("Good {0} {1}", "a", "b");
        assert_eq!(m.to_string(), "Good a b");
        assert_eq!(m.localized(), "b a gut");
    }

    fn pick(args: &[&str], env: &[(&str, &str)], config: Option<&str>) -> Choice {
        let args: Vec<String> = std::iter::once("putio-sync")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        let env = |name: &str| {
            env.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        };
        select(&args, &env, &|| config.map(String::from), &Vec::new)
    }

    #[test]
    fn select_follows_the_order() {
        let all = [
            ("PUTIO_SYNC_LANG", "es"),
            ("LANGUAGE", "it"),
            ("LC_ALL", "ja_JP.UTF-8"),
            ("LC_MESSAGES", "ko_KR"),
            ("LANG", "nl_NL"),
        ];
        let c = pick(&["status", "--lang", "fr"], &all, Some("de"));
        assert_eq!((c.lang.code, c.source), ("fr", Source::Flag));
        let c = pick(&["--lang=fr", "status"], &all, Some("de"));
        assert_eq!((c.lang.code, c.source), ("fr", Source::Flag));
        let c = pick(&["status"], &all, Some("de"));
        assert_eq!((c.lang.code, c.source), ("es", Source::Env));
        let c = pick(&["status"], &all[1..], Some("de"));
        assert_eq!((c.lang.code, c.source), ("de", Source::Config));
        let c = pick(&["status"], &all[1..], None);
        assert_eq!(
            (c.lang.code, c.source, c.os_variable),
            ("it", Source::Os, Some("LANGUAGE"))
        );
        let c = pick(&[], &all[2..], None);
        assert_eq!((c.lang.code, c.os_variable), ("ja", Some("LC_ALL")));
        let c = pick(&[], &all[3..], None);
        assert_eq!((c.lang.code, c.os_variable), ("ko", Some("LC_MESSAGES")));
        let c = pick(&[], &all[4..], None);
        assert_eq!((c.lang.code, c.os_variable), ("nl", Some("LANG")));
        let c = pick(&[], &[], None);
        assert_eq!((c.lang.code, c.source), ("en", Source::Default));
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn select_tries_each_language_entry_and_stops_at_c() {
        let c = pick(&[], &[("LANGUAGE", "xx:yy:sv:de")], None);
        assert_eq!(c.lang.code, "sv");
        let c = pick(&[], &[("LC_ALL", "C"), ("LANG", "de_DE.UTF-8")], None);
        assert_eq!((c.lang.code, c.source), ("en", Source::Default));
        assert!(c.warnings.is_empty());
        let c = pick(&[], &[("LANG", "POSIX")], None);
        assert_eq!(c.lang.code, "en");
    }

    #[test]
    fn select_warns_about_unknown_explicit_values_only() {
        let c = pick(
            &["--lang", "xx"],
            &[("PUTIO_SYNC_LANG", "qq"), ("LANG", "de_DE")],
            Some("zz"),
        );
        assert_eq!((c.lang.code, c.source), ("de", Source::Os));
        assert_eq!(c.warnings, ["xx", "qq", "zz"]);
        let c = pick(&[], &[("LANG", "xx_YY.UTF-8")], None);
        assert_eq!(c.lang.code, "en");
        assert!(c.warnings.is_empty());
    }

    #[test]
    fn select_asks_the_system_only_without_locale_variables() {
        let args = vec!["putio-sync".to_string()];
        let os = || vec!["xx-YY".to_string(), "pt-BR".to_string()];
        let c = select(&args, &|_| None, &|| None, &os);
        assert_eq!(
            (c.lang.code, c.source, c.os_variable),
            ("pt", Source::Os, None)
        );
        let c = select(
            &args,
            &|n| (n == "LANG").then(|| "C".to_string()),
            &|| None,
            &os,
        );
        assert_eq!(c.lang.code, "en");
    }

    #[test]
    fn config_path_comes_from_the_arguments() {
        let args = |list: &[&str]| -> Vec<String> { list.iter().map(|s| s.to_string()).collect() };
        assert_eq!(
            config_arg(&args(&["p", "-c", "a.toml"])),
            Some("a.toml".into())
        );
        assert_eq!(
            config_arg(&args(&["p", "status", "--config=b.toml"])),
            Some("b.toml".into())
        );
        assert_eq!(
            config_arg(&args(&["p", "--config", "c.toml"])),
            Some("c.toml".into())
        );
        assert_eq!(config_arg(&args(&["p", "status"])), None);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[ui]\nlanguage = \"de\"\n\n[sync]\nbroken = [\n").unwrap();
        let with = args(&["p", "-c", path.to_str().unwrap()]);
        // A file that does not parse gives nothing here; the full load reports it
        assert_eq!(config_language(&with, &|_| None), None);
        std::fs::write(&path, "[ui]\nlanguage = \"de\"\n").unwrap();
        assert_eq!(config_language(&with, &|_| None), Some("de".into()));
        let by_env = |name: &str| (name == "PUTIO_SYNC_CONFIG").then(|| path.display().to_string());
        assert_eq!(config_language(&args(&["p"]), &by_env), Some("de".into()));
    }
}
