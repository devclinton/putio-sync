//! The CLI's translation catalogs (feature 060): `i18n/en.json` lists every string the CLI
//! can translate, and each other catalog holds translations of those strings only, with the
//! same placeholders.
//!
//! Regenerate after changing a string: `make i18n-cli`
//! (`UPDATE_I18N=1 cargo test -p putio-sync-cli --test i18n_catalog`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

type Catalog = BTreeMap<String, String>;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The value of the Rust string literal starting at `text[0] == '"'`
fn literal(text: &str) -> Option<String> {
    let mut chars = text.chars();
    if chars.next() != Some('"') {
        return None;
    }
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\n' => {
                    // A line continuation: skip the next line's indentation
                    let rest = chars.as_str().trim_start();
                    chars = rest.chars();
                }
                'u' => {
                    let rest = chars.as_str();
                    let end = rest.find('}')?;
                    let code = u32::from_str_radix(&rest[1..end], 16).ok()?;
                    out.push(char::from_u32(code)?);
                    chars = rest[end + 1..].chars();
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Every literal given to `t!` or `msg!` in `source`
fn macro_keys(source: &str, keys: &mut BTreeSet<String>) {
    for name in ["t!(", "msg!("] {
        let mut from = 0;
        while let Some(found) = source[from..].find(name) {
            let at = from + found;
            from = at + name.len();
            let before = source[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            if let Some(key) = literal(source[from..].trim_start()) {
                keys.insert(key);
            }
        }
    }
}

fn command_keys(cmd: &clap::Command, keys: &mut BTreeSet<String>) {
    let texts = [
        cmd.get_about(),
        cmd.get_long_about(),
        cmd.get_after_help(),
        cmd.get_before_help(),
    ];
    keys.extend(texts.into_iter().flatten().map(|s| s.to_string()));
    for arg in cmd.get_arguments() {
        keys.extend(
            [arg.get_help(), arg.get_long_help()]
                .into_iter()
                .flatten()
                .map(|s| s.to_string()),
        );
    }
    for sub in cmd.get_subcommands() {
        command_keys(sub, keys);
    }
}

/// Everything the CLI can translate: `t!`/`msg!` literals, help text, and the doctor's
/// titles and labels
fn extract() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    let root = crate_dir();
    let mut files = Vec::new();
    for dir in ["src", "../putio-sync-doctor/src", "../putio-sync-core/src"] {
        rust_files(&root.join(dir), &mut files);
    }
    for file in files {
        macro_keys(&std::fs::read_to_string(file).unwrap(), &mut keys);
    }
    keys.extend(
        putio_sync_cli::i18n::FIXED_KEYS
            .iter()
            .map(|k| k.to_string()),
    );
    command_keys(
        &<putio_sync_cli::cli::Cli as clap::CommandFactory>::command(),
        &mut keys,
    );
    use putio_sync_doctor::report::{Area, CheckId, FixId};
    keys.extend(Area::ALL.iter().map(|a| a.title().to_string()));
    keys.extend(CheckId::ALL.iter().map(|c| c.title().to_string()));
    keys.extend(FixId::ALL.iter().map(|f| f.label().to_string()));
    keys.remove("");
    keys
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        if let Some(close) = rest.find('}') {
            let inner = &rest[..close];
            if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
                found.insert(inner.to_string());
            }
        }
    }
    found
}

fn path(code: &str) -> PathBuf {
    crate_dir().join("i18n").join(format!("{code}.json"))
}

fn read(code: &str) -> Option<Catalog> {
    serde_json::from_str(&std::fs::read_to_string(path(code)).ok()?).ok()
}

/// The repository's catalog format: sorted keys, one space of indent, a final newline
fn render(catalog: &Catalog) -> String {
    use serde::Serialize;
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    catalog
        .serialize(&mut serde_json::Serializer::with_formatter(
            &mut out, formatter,
        ))
        .unwrap();
    out.push(b'\n');
    String::from_utf8(out).unwrap()
}

fn codes() -> Vec<&'static str> {
    putio_sync_core::i18n::LANGS
        .iter()
        .map(|lang| lang.code)
        .filter(|code| *code != "en")
        .collect()
}

#[test]
fn catalogs_match_the_source() {
    let keys = extract();
    let full = cfg!(feature = "server");

    if std::env::var_os("UPDATE_I18N").is_some() {
        assert!(full, "update the catalogs with the default features");
        let en: Catalog = keys.iter().map(|k| (k.clone(), k.clone())).collect();
        std::fs::write(path("en"), render(&en)).unwrap();
        for code in codes() {
            let mut catalog = read(code).unwrap_or_default();
            catalog.retain(|k, v| keys.contains(k) && placeholders(k) == placeholders(v));
            std::fs::write(path(code), render(&catalog)).unwrap();
        }
    }

    let en = read("en").expect("i18n/en.json");
    for (key, value) in &en {
        assert_eq!(key, value, "en.json maps each string to itself");
    }
    let listed: BTreeSet<String> = en.keys().cloned().collect();
    let missing: Vec<&String> = keys.difference(&listed).collect();
    assert!(
        missing.is_empty(),
        "not in en.json, run `make i18n-cli`: {missing:#?}"
    );
    if full {
        let stale: Vec<&String> = listed.difference(&keys).collect();
        assert!(
            stale.is_empty(),
            "no longer used, run `make i18n-cli`: {stale:#?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(path("en")).unwrap(),
        render(&en),
        "en.json is not in the catalog format"
    );

    assert_eq!(codes().len(), 26);
    for code in codes() {
        let catalog = read(code).unwrap_or_else(|| panic!("i18n/{code}.json"));
        for (key, value) in &catalog {
            assert!(listed.contains(key), "{code}: {key:?} is not in en.json");
            assert!(!value.trim().is_empty(), "{code}: {key:?} is empty");
            assert_eq!(
                placeholders(key),
                placeholders(value),
                "{code}: placeholders of {key:?} differ in {value:?}"
            );
        }
        if let Some(yes) = catalog.get("y|yes") {
            assert!(
                yes.split('|')
                    .all(|w| !w.is_empty() && w.to_lowercase() == w),
                "{code}: \"y|yes\" must be lower-case words separated by |: {yes:?}"
            );
        }
    }
}

#[test]
fn the_literal_reader_handles_escapes_and_continuations() {
    let mut keys = BTreeSet::new();
    macro_keys(
        "t!(\"a \\\"b\\\"\\n\", x); msg!(\n  \"one \\\n      two {0}\", y); not!(\"z\"); fmt!(\"q\")",
        &mut keys,
    );
    let keys: Vec<String> = keys.into_iter().collect();
    assert_eq!(keys, ["a \"b\"\n", "one two {0}"]);
}

/// Machine output and logs stay English: no `t!` in the JSON and CSV writers or inside a
/// logging macro
#[test]
fn no_translation_in_machine_output_or_logs() {
    let root = crate_dir();
    for file in ["src/output/json.rs", "src/output/csv.rs"] {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        let mut keys = BTreeSet::new();
        macro_keys(&text, &mut keys);
        assert!(keys.is_empty(), "{file} must not translate: {keys:?}");
    }
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("../putio-sync-doctor/src"), &mut files);
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for name in ["trace!(", "debug!(", "info!(", "warn!(", "error!("] {
            let mut from = 0;
            while let Some(found) = text[from..].find(name) {
                let at = from + found;
                from = at + name.len();
                let end = text[at..].find(';').map_or(text.len(), |e| at + e);
                let call = &text[at..end];
                let mut keys = BTreeSet::new();
                macro_keys(call, &mut keys);
                assert!(
                    keys.is_empty(),
                    "{}: a log line is translated: {call}",
                    file.display()
                );
            }
        }
    }
}
