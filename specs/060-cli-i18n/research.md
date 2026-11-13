# Research: Multi-language CLI

## R1. Where the CLI's text is today

- About 300 `println!`/`eprintln!`/`print!` calls across `putio-sync-cli/src/commands/*.rs`,
  `output/` and `ui/`, all English literals or `format!` templates.
- Help text is clap derive attributes in `putio-sync-cli/src/cli.rs` (`about = "…"`, `help = "…"`,
  `long_about`), about 1,370 lines of argument definitions.
- `putio-sync doctor` prints `Finding.summary` and `.suggestion` strings built in
  `putio-sync-doctor/src/checks/` with `format!` (about 80 sites). The same findings go to the API,
  the web UI's Doctor page, notifications and `doctor --json`.
- `main.rs` prints a "STARTUP DEBUG" block on standard error before anything else. It is
  diagnostic output, read like a log.
- Tables (`output/table.rs`) pad with `{:<40}` (counts chars, not display width) and `truncate`
  slices by bytes, which panics on a name whose 37th byte is inside a multi-byte character.

## R2. Catalog format and keys

**Decision**: The same format as 051: one flat JSON object per language, keyed by the English
text, placeholders `{0}`, `{1}`… in order. CLI catalogs live in `putio-sync-cli/i18n/<code>.json`,
with `en.json` as the source.

**Rationale**: `scripts/i18n-translate.mjs` and the atranslate service already handle this format,
placeholders included. Keys are readable in review and need no ID scheme.

**Alternatives**: Fluent (`fluent-rs`) or gettext `.po`: proper plurals, but a new format the
translation script and service don't speak, and plurals are out of scope (spec Assumptions).
Separate keys (`status.header.size`): reviewers lose the English next to each translation.

## R3. Marking text in Rust code

**Decision**: Two macros in a new `putio_sync_core::i18n` module:

- `t!("Downloaded {0} of {1}", a, b)` → `String`, translated now into the run's language. For
  text that only reaches a terminal.
- `msg!("Disk {0} is {1}% full", d, p)` → `Message::Template { template, args }`. `Display`
  writes English; `.localized()` writes the run's language. For text that is both shown to a
  person and sent somewhere that must stay English (doctor findings). `Message::Text(String)`
  carries text from elsewhere unchanged.

Doctor results are serialized, deserialized (stored reports) and compared, so `CheckResult` keeps
English `String`s and carries the `Message`s in a `#[serde(skip)]` field for the CLI to print
(data-model.md, "In the doctor"). Check titles are already `&'static str` literals and are looked
up at print time.

The template must be a string literal; the macros check the placeholder count against the
arguments at compile time (a `const` count of `{N}` in the literal).

**Rationale**: Extraction becomes a plain scan for `t!("` and `msg!("` literals, much simpler than
the JS scanner in 051. `Message` keeps doctor JSON and the API English without two code paths.

**Alternatives**: Translating at print time by looking up whole formatted lines: impossible once
values are interpolated. Translating doctor findings in the doctor crate: the API and JSON would
get the CLI's language.

## R4. Help text from clap

**Decision**: Keep the derive attributes in English. At start-up, build `Cli::command()`, then walk
it (`mut_subcommand`, `mut_args`, recursively) replacing `about`, `long_about`, `help`,
`long_help` and `next_help_heading` with their catalog entries, and set a `help_template` whose
"Usage:", "Options:", "Commands:" and "Arguments:" headings are translated. Parse with
`get_matches_from` then `Cli::from_arg_matches`. The built-in `--help` and `--version` args get
translated help via `mut_arg("help", …)`.

Extraction for help is done by walking the same `Cli::command()` tree in a test, not by scanning
source, so doc comments, `help =` and `about =` are all covered exactly.

Clap's own error text ("unexpected argument", "a value is required") is not translatable. The CLI
catches `clap::Error` and, for the common kinds (`UnknownArgument`, `InvalidValue`,
`MissingRequiredArgument`, `InvalidSubcommand`, `NoEquals`), prints its own translated message
from the error's context (`ContextKind::InvalidArg`, `InvalidValue`, `ValidValue`,
`SuggestedArg`), keeping clap's exit code 2. Other kinds print clap's English.

**Alternatives**: clap has no localization support (open issue since 2020). Forking help
rendering: far too much code.

## R5. Picking the language before clap runs

The help text must be translated before clap parses, but the config language is only known after
the config loads, which happens after parsing.

**Decision**: `i18n::select()` runs first in `main`, from raw `std::env::args_os()`:

1. `--lang X` or `--lang=X` anywhere in argv.
2. `PUTIO_SYNC_LANG`.
3. `[ui] language` from the config file, read with a two-field struct (`toml::from_str` of only
   `[ui]`), from `-c/--config`, `PUTIO_SYNC_CONFIG`, or the default path
   (`Config::default_path`). Read errors are silent here; the full config load reports them later.
4. The OS locale (R6).
5. English.

Each step's value is matched like 051's `match()`: exact code (case-insensitive, `_` → `-`), else
base language, else not found → next step, with one warning (spec FR-004). `--lang` is also a
global clap arg so clap accepts and documents it.

## R6. OS locale

**Decision**: On every platform, read the environment first in gettext order: `LANGUAGE` (a
colon list; each entry tried), `LC_ALL`, `LC_MESSAGES`, `LANG`. `C`, `POSIX` and `C.UTF-8` mean
"no language" and end the environment step. When none is set, ask the OS with `sys-locale`
(`get_locales()`), a new dependency in `putio-sync-core`: on macOS the user's preferred languages
(a CLI started by launchd or a GUI often has no `LANG`), on Windows
`GetUserPreferredUILanguages`.

**Rationale**: The environment is what users and containers set, and what tests can control.
`sys-locale` is about 500 lines with no transitive dependencies on these platforms, and saves
CoreFoundation and Win32 calls of our own.

**Alternatives**: Hand-rolled `windows-sys` `Win32_Globalization` (already a dependency elsewhere):
covers Windows but not macOS without CoreFoundation bindings. Environment only: wrong on Windows
and for macOS GUI launches.

## R7. Embedding catalogs and size

Measured from 051: 1,592 UI strings ≈ 110 KB per catalog. The CLI has an estimated 700–900
strings (prints plus help), shorter on average: ≈ 40 KB per catalog, ≈ 1.1 MB for 28.

**Decision**: `putio-sync-cli/build.rs` (behind the `i18n` feature) compresses each catalog
separately with `flate2` (already a CLI dependency; added as a build dependency) and
`include_bytes!` the result. At start-up only the chosen language is inflated and parsed into a
`HashMap<String, String>` stored in a `OnceLock`. JSON text deflates about 3.5×: ≈ 320 KB added,
within SC-004's 400 KiB.

The `i18n` feature is on in `default` and added to both lite builds in the release workflow and
`INSTALLATION.md`'s build commands. Without it, `i18n::select` still runs (so `--lang` is accepted
and warns) but only English exists: under 20 KiB.

**Alternatives**: One compressed bundle of all languages: smaller by ~10% but inflates 1 MB at every
start. `zstd`: better ratio, new C dependency. `lzma-rs` (in the tree via `putio-sync-update`): its
encoder doesn't compress meaningfully.

## R8. Display width in tables

**Decision**: Use `unicode-width` (already in `Cargo.lock` through `indicatif`/`console`, so no new
code) for padding and truncation in `output/table.rs` and the status/queue/transfers tables. A
small `pad(s, width)` and `truncate(s, width)` helper in `output/mod.rs`; truncation cuts at a char
boundary by width. Fixes the byte-slicing panic in R1 as a side effect (a test reproduces it
first).

Right-to-left: printed in logical order; no bidi reordering (spec edge case).

## R9. What stays English

- `--json`, `--csv`, `--output json` and anything written by `output/json.rs` and `output/csv.rs`:
  they never call `t!`, and doctor JSON is built from `CheckResult`'s English strings.
- `tracing` log messages and fields: unchanged; a CI check (R11) fails if `t!` appears inside a
  `tracing` macro.
- The `main.rs` STARTUP DEBUG block (diagnostic, read like a log).
- Exit codes, command/flag/value names, config keys.
- Text from Put.io, the OS, plugins and hooks.

The daemon's foreground console lines meant for a person (the setup link and "Open … to finish
setup") are translated.

## R10. Confirmations

**Decision**: One helper `ui::confirm(question) -> bool` replaces the five `read_line` prompts. It
prints the translated question with translated `[y/N]`, and accepts `y`/`yes` plus the
translation's own yes words, listed in the catalog under the key `"y|yes"` (for example
`"j|ja"` in German). Anything else is no, as today.

## R11. Keeping catalogs current

**Decision**: Rust test `putio-sync-cli/tests/i18n_catalog.rs`, so `make ci` runs it with
`cargo test`:

- Builds the expected `en.json` from (a) a scan of every `.rs` file in `putio-sync-cli/src` and
  `putio-sync-doctor/src` for `t!("…")`/`msg!("…")` literals, (b) the walk of `Cli::command()`
  and (c) every `CheckId::title()` and `Area::title()`. `cli.rs` is a module of the binary only
  today (`mod cli;` in `main.rs`), so it moves into the library (`pub mod cli;` in `lib.rs`) for
  the test and for `src/i18n.rs` to reach it.
  Fails with "run `make i18n`" when it differs. `UPDATE_I18N=1` rewrites it (that's what
  `make i18n` does, alongside the existing web UI extract).
- Every shipped catalog is valid JSON, has no keys that `en.json` lacks, keeps each message's
  placeholder set, and translates at least 90% of keys (SC-003).
- The file list matches the web UI's `LANGUAGES` plus `en`.
- No `t!(` inside `info!`/`warn!`/`error!`/`debug!`/`trace!` calls.

`scripts/i18n-translate.mjs` gets a `--catalog-dir` option (default the web UI's) and
`make i18n-translate` runs it for both. Before calling the service it copies translations for keys
the web UI catalog already has (FR-018).

**Alternatives**: a node extractor like 051's: needs a Rust string-literal parser in JS; the test
is shorter and runs in `cargo test` already.

## R12. The config setting and the web UI

**Decision**: New `[ui]` section in `Config` with `language: Option<String>` (`None` = automatic).
Config load does not validate it, so a bad value is reported once: by `i18n::select` for the CLI
and by the doctor's Config check. Readers treat an unknown value as `None`.
`config.example.toml` documents it. The web UI's "Browser default" becomes "Automatic": saved
browser choice, else the config's `ui.language` (a new `GET`/`PUT /settings/ui` group, like the other settings
groups in `routes/system/settings.rs`), else the browser's languages, else English. 056 will write
the same key. The sign-in page is shown before the UI can read settings, so it follows the saved
or browser language only.

**Alternatives**: a top-level `language` key: ambiguous next to `[subtitles] languages`.
