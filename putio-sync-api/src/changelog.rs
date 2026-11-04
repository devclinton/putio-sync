//! The release notes built into the binary (feature 017)
//!
//! `CHANGELOG.md` at the repository root is embedded at build time and read once into
//! releases (`## [name] - date`), kinds (`### Added`) and entries (`- text`, with indented
//! continuation lines). Everything else in the file (the introduction, link references) is
//! left out. Each entry's id is a hash of its text alone, so an entry keeps its id across
//! builds and when a release moves it out of Unreleased (research R1, R2).

use serde::Serialize;
use std::sync::OnceLock;

/// The repository's `CHANGELOG.md` as it was when this binary was built
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");

/// A `## [name]` section
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Release {
    /// `Unreleased` or the version
    pub name: String,
    /// The text after ` - ` on the heading
    pub date: Option<String>,
    pub kinds: Vec<Kind>,
}

/// A `### Added` (or other) group of a release
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Kind {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// One bullet
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    /// [`entry_id`] of `text`
    pub id: String,
    /// The bullet's markdown, its lines joined by single spaces
    pub text: String,
}

/// The built-in changelog, parsed on first use
pub fn builtin() -> &'static [Release] {
    static PARSED: OnceLock<Vec<Release>> = OnceLock::new();
    PARSED.get_or_init(|| parse(CHANGELOG))
}

/// 16 hex digits of FNV-1a 64 over `text` with whitespace runs collapsed, so the id is the
/// same in every build and rewrapping a paragraph keeps it
pub fn entry_id(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut first = true;
    for word in text.split_whitespace() {
        if !first {
            hash = fnv(hash, b" ");
        }
        hash = fnv(hash, word.as_bytes());
        first = false;
    }
    format!("{hash:016x}")
}

fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Read a Keep a Changelog file into its releases, in file order
pub fn parse(text: &str) -> Vec<Release> {
    let mut releases: Vec<Release> = Vec::new();
    let mut entry: Option<String> = None;

    for line in text.lines() {
        let is_continuation = line.starts_with(char::is_whitespace) && !line.trim().is_empty();
        if is_continuation {
            if let Some(entry) = entry.as_mut() {
                entry.push(' ');
                entry.push_str(line.trim());
            }
            continue;
        }
        finish(&mut releases, entry.take());

        if let Some(heading) = line.strip_prefix("## [") {
            let (name, rest) = heading.split_once(']').unwrap_or((heading, ""));
            let date = rest
                .trim()
                .strip_prefix('-')
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty());
            releases.push(Release {
                name: name.trim().to_string(),
                date,
                kinds: Vec::new(),
            });
        } else if let Some(kind) = line.strip_prefix("### ") {
            if let Some(release) = releases.last_mut() {
                release.kinds.push(Kind {
                    name: kind.trim().to_string(),
                    entries: Vec::new(),
                });
            }
        } else if let Some(bullet) = line.strip_prefix("- ") {
            let in_kind = releases.last().is_some_and(|r| !r.kinds.is_empty());
            if in_kind {
                entry = Some(bullet.trim().to_string());
            }
        }
    }
    finish(&mut releases, entry);
    releases
}

/// Add a finished entry to the last kind
fn finish(releases: &mut [Release], entry: Option<String>) {
    let Some(text) = entry.filter(|t| !t.is_empty()) else {
        return;
    };
    if let Some(kind) = releases.last_mut().and_then(|r| r.kinds.last_mut()) {
        kind.entries.push(Entry {
            id: entry_id(&text),
            text,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Changelog

Intro with a [link](https://example.com) and
- a bullet before any release, ignored.

## [Unreleased]

## [0.2.0] - 2026-11-16

### Added

- A first entry that spans
  two lines.
- A second entry with `code`.

### Fixed

- A fix.

## [0.1.0]

### Changed

- Old change.

[Unreleased]: https://example.com/compare
";

    #[test]
    fn reads_releases_kinds_and_entries() {
        let releases = parse(SAMPLE);
        let names: Vec<_> = releases.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Unreleased", "0.2.0", "0.1.0"]);
        assert!(releases[0].kinds.is_empty(), "empty Unreleased is kept");
        assert_eq!(releases[0].date, None);
        assert_eq!(releases[1].date.as_deref(), Some("2026-11-16"));
        assert_eq!(releases[2].date, None);

        let added = &releases[1].kinds[0];
        assert_eq!(added.name, "Added");
        assert_eq!(added.entries.len(), 2);
        assert_eq!(added.entries[0].text, "A first entry that spans two lines.");
        assert_eq!(added.entries[1].text, "A second entry with `code`.");
        assert_eq!(releases[1].kinds[1].entries[0].text, "A fix.");
        assert_eq!(releases[2].kinds[0].entries[0].text, "Old change.");
    }

    #[test]
    fn leaves_out_the_intro_and_link_references() {
        let all: Vec<_> = parse(SAMPLE)
            .into_iter()
            .flat_map(|r| r.kinds)
            .flat_map(|k| k.entries)
            .map(|e| e.text)
            .collect();
        assert_eq!(all.len(), 4);
        assert!(all
            .iter()
            .all(|t| !t.contains("ignored") && !t.contains("compare")));
    }

    #[test]
    fn ids_follow_the_text_only() {
        assert_eq!(entry_id("a  b\n c"), entry_id("a b c"));
        assert_ne!(entry_id("a b"), entry_id("a c"));
        let moved = "## [0.3.0]\n\n### Added\n\n- A first entry\n  that spans two lines.\n";
        assert_eq!(
            parse(moved)[0].kinds[0].entries[0].id,
            parse(SAMPLE)[1].kinds[0].entries[0].id
        );
    }

    #[test]
    fn id_is_fnv1a_64() {
        // Published FNV-1a 64 test vectors
        assert_eq!(entry_id(""), "cbf29ce484222325");
        assert_eq!(entry_id("a"), "af63dc4c8601ec8c");
        assert_eq!(entry_id("foobar"), "85944171f73967e8");
    }

    #[test]
    fn the_builtin_changelog_has_entries() {
        let releases = builtin();
        assert_eq!(releases[0].name, "Unreleased");
        let entries: Vec<_> = releases
            .iter()
            .flat_map(|r| &r.kinds)
            .flat_map(|k| &k.entries)
            .collect();
        assert!(!entries.is_empty());
        assert!(entries
            .iter()
            .all(|e| !e.text.is_empty() && e.id.len() == 16));
    }
}
