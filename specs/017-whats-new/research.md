# Research: What's New

## R1 How "last seen" is tracked

- **Decision**: per entry, by a hash of its text, in a SQLite set of seen ids.
- **Rationale**: builds from main add entries under Unreleased without changing the version,
  and a release moves entries into a version section without changing their text. A
  version number (or the section an entry is in) cannot tell an edge user what is new; the
  entry text can.
- **Alternatives**: last seen version (misses every edge build); last seen commit plus a
  changelog per commit (needs git at runtime); a hash of the whole file (says something
  changed, not what).

## R2 The id hash

- **Decision**: FNV-1a 64-bit over the entry text with runs of whitespace collapsed to one
  space and trimmed, printed as 16 hex digits.
- **Rationale**: must be identical across builds and Rust versions; `DefaultHasher` (SipHash
  with unspecified keys/algorithm) is not guaranteed stable. Re-wrapping a paragraph does
  not change the id. 64 bits is plenty for a few hundred entries.
- **Alternatives**: `sha2` (a new dependency for no gain), `fnv` crate (only transitive
  today; the function is 10 lines).

## R3 Where "seen" is stored

- **Decision**: a new table `whats_new_seen(id TEXT PRIMARY KEY, seen_at)` in the existing
  database.
- **Rationale**: shared by every browser (spec decision), survives restarts and upgrades,
  and the database is already created and migrated with `CREATE TABLE IF NOT EXISTS`.
  `daemon_flags` holds "1"/"0" switches, not sets.
- **Alternatives**: browser `localStorage` (per browser, so a second device sees the notes
  again); a file next to the config (another path to manage in Docker/snap).
- Without a database (`AppState.database` is `None`, e.g. some `server` runs) every entry
  reports as seen: the panel never shows, the page still works.

## R4 First run

- **Decision**: when the seen table is empty, `GET /whats-new` inserts every built-in id and
  reports none as new.
- **Rationale**: a new install has nothing to be told about, and the first build with this
  feature cannot know what an upgrading user saw before. Two concurrent first GETs both
  insert with `INSERT OR IGNORE`, so the outcome is the same.

## R5 Parsing CHANGELOG.md

- **Decision**: a line parser: `## [Name]` (optionally ` - date`) starts a release;
  `### Kind` starts a kind; `- text` starts an entry; an indented non-empty line continues
  the current entry; a blank line ends it; everything else (intro, link references) is
  ignored.
- **Rationale**: the file follows Keep a Changelog strictly (`make ci` checks entries are
  added) and `scripts/changelog-section.sh` relies on the same headings. A markdown crate
  would be a new dependency for three inline forms.
- Empty sections (Unreleased right after a release) are kept, with no kinds, and skipped by
  the UI.

## R6 Rendering entries safely

- **Decision**: the server sends raw entry text; the UI turns `` `code` ``,
  `[text](url)` and `**bold**` into elements with `textContent`, and every other character
  stays text. Links are kept only for `http:`/`https:` URLs or relative paths; relative
  repository paths (`putio-sync-cli/CHANGELOG.md`) point at GitHub's copy, since the
  daemon doesn't serve them.
- **Rationale**: the UI never sets `innerHTML` (index.html has a strict CSP and `h()` uses
  text nodes); a small tokenizer keeps it that way and costs well under 1 KB.

## R7 When the UI checks

- **Decision**: once per page load, on the first route to a signed-in page (also right
  after signing in), never in setup mode or on the sign-in page.
- **Rationale**: one request per load is negligible; checking on every route change is
  pointless because the set only changes on upgrade or when the panel closes.

## R8 Embedding and packaging

- **Decision**: `include_str!("../../CHANGELOG.md")` in `putio-sync-api/src/changelog.rs`.
  The Dockerfile's builder stage copies `CHANGELOG.md` before `cargo build` (cargo-chef's
  cook step builds dependencies only, so it does not need it). Snap, Flatpak, Homebrew and
  plain cargo builds build from the repository root, where the file is.
- `cargo` reruns the build when an `include_str!` file changes, so the notes are always the
  build's own.
