# Implementation Plan: Multi-language CLI

**Branch**: `060-cli-i18n` | **Date**: 2026-11-13 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/060-cli-i18n/spec.md`

## Summary

Keep the CLI's English text in the code as the catalog key, as 051 does for the web UI. A new
`putio_sync_core::i18n` module picks the run's language before clap parses (`--lang`, then
`PUTIO_SYNC_LANG`, then `[ui] language` in the config, then the OS locale, then English) and
offers two macros: `t!` for terminal-only text and `msg!` for text that must stay English in JSON
and the API (doctor findings). The CLI walks clap's command tree to translate help, renders its
own messages for common argument errors, pads tables by display width, and shares one confirmation
helper. Catalogs live in `putio-sync-cli/i18n/`, are deflated by `build.rs` behind a default `i18n`
feature (also on in the lite builds) and only the chosen one is inflated. A Rust test keeps
`en.json` current and checks every catalog; the 051 translate script fills them. Details:
[research.md](research.md).

## Technical Context

**Language/Version**: Rust stable (edition 2021); Node 22 for the translate script only
**Primary Dependencies**: new: `sys-locale` (core, R6); `flate2` as a build dependency of the CLI
(already a normal one); `unicode-width` as a direct CLI dependency (already in the lock tree, R8)
**Storage**: catalogs in `putio-sync-cli/i18n/*.json`, embedded compressed; `[ui] language` in
`config.toml`
**Testing**: `putio-sync-core` unit tests for selection and matching; `putio-sync-cli/tests/
i18n_catalog.rs` (en.json current, catalogs valid, placeholders, coverage ≥ 90%, list matches
`LANGUAGES`, no `t!` in `tracing` macros); `putio-sync-cli/tests/i18n_cli.rs` (runs the binary
under `LANG`/`--lang`, compares `--json`/`--csv` byte for byte across languages, help is
translated, unknown language warns); a table test reproducing the truncate panic
**Target Platform**: Linux, macOS, Windows; full and lite builds
**Project Type**: CLI and daemon in a Cargo workspace
**Performance Goals**: language selection plus one catalog inflate and parse in under 10 ms (SC-005)
**Constraints**: each binary grows by ≤ 400 KiB, English-only build by ≤ 20 KiB (SC-004); no
network or files needed at run time; machine output and logs unchanged
**Scale/Scope**: an estimated 700–900 messages × 28 languages, about 40 KB per catalog before
compression

## Constitution Check

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | Pass | Selection, matching, `Message` and the macros are shared by `cli` and `doctor`, so they live in `core`. Catalogs, clap walking and rendering stay in `cli`. `doctor` only gains `msg!`. `api` only gains the `/settings/ui` group. |
| II. Local-first | Pass | `make ci` runs both i18n tests via `cargo test`. Translating is a maintainer step (`make i18n-translate`), not CI. |
| III. Tests without Put.io | Pass | Tests run commands that need no Put.io (`--help`, `languages`, `doctor` against a mock, `files list` against the existing wiremock fixtures). |
| IV. Download integrity | Pass | No change. |
| V. Observable | Pass | Logs stay English with the same fields; a test forbids `t!` inside `tracing` macros. |
| VI. Secrets | Pass | Nothing new is logged; the translation service address comes from the environment, as in 051. |
| VII. Simplicity | Pass with note | One new runtime dependency (`sys-locale`, justified in R6) and one cargo feature (`i18n`, needed for the English-only build in FR-015). No new catalog format. |

## Project Structure

### Documentation (this feature)

```text
specs/060-cli-i18n/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R12
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── cli.md           # --lang, PUTIO_SYNC_LANG, `languages`, warnings, what stays English
│   ├── config.md        # [ui] language and /settings/ui
│   └── catalog.md       # catalog files, keys, placeholders, the "y|yes" key
└── checklists/requirements.md
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── i18n.rs              # Lang list, matching, select(), Message, t!/msg!, the OnceLock catalog
└── config.rs            # + UiConfig { language }

putio-sync-cli/
├── build.rs             # deflate i18n/*.json into OUT_DIR (feature "i18n")
├── i18n/                # en.json (source) + 28 catalogs
├── src/
│   ├── main.rs          # i18n::select() first; localized clap parse
│   ├── i18n.rs          # embedded catalogs, clap tree walk, clap error rendering
│   ├── lib.rs           # + pub mod cli, pub mod i18n
│   ├── cli.rs           # moved into the library; + global --lang, `languages` subcommand
│   ├── commands/*.rs    # literals → t!(…); prompts → ui::confirm
│   ├── commands/languages.rs
│   ├── output/mod.rs    # pad/truncate by display width
│   ├── output/table.rs  # uses them; headers via t!
│   └── ui/confirm.rs
└── tests/
    ├── i18n_catalog.rs
    └── i18n_cli.rs

putio-sync-doctor/src/checks/*.rs   # summary/suggestion format! → msg!
putio-sync-doctor/src/report.rs     # Finding.summary/suggestion: Message; CheckResult.local (serde skip)
putio-sync-doctor/src/lib.rs        # fill CheckResult's English strings and `local` from Finding
putio-sync-api/src/routes/system/settings.rs  # GET/PUT /settings/ui
putio-sync-api/ui/i18n.js           # "Automatic" uses ui.language before the browser's
scripts/i18n-translate.mjs          # --catalog-dir; reuse web UI translations
Makefile                            # i18n / i18n-translate cover the CLI
.github/workflows/release-binaries.yml, INSTALLATION.md  # lite builds add the i18n feature
config.example.toml, README.md, CHANGELOG.md
```

**Structure Decision**: Existing workspace layout. The shared part goes to `core` because both
`cli` and `doctor` need it (Principle I); everything that knows about clap or embedded bytes stays
in `cli`.

## Phasing

1. **Core i18n** (US1, US2): `core::i18n` with matching, `select()`, `t!`, `msg!`, English-only;
   `[ui] language`; `--lang` and `languages`. Testable with a fake catalog.
2. **Catalogs and embedding** (US5, US6): `build.rs`, `i18n` feature, the catalog test, `make i18n`,
   translate script changes, lite build flags.
3. **Command output** (US1, US3): convert the commands, tables (display width), prompts; the
   byte-identical machine-output test.
4. **Help and argument errors** (US4): clap tree walk, help template, error rendering.
5. **Doctor findings** (US1): `Message` in `Finding`; doctor JSON and API unchanged.
6. **Web UI default** (FR-006): `/settings/ui`, "Automatic" in the Language card.
7. Translate, measure sizes and start-up time, update docs and CHANGELOG.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| New dependency `sys-locale` | macOS and Windows locale without our own OS bindings (FR-003) | Environment only is wrong on Windows and for macOS GUI launches; own bindings are more code than the crate |
| New cargo feature `i18n` | English-only build (FR-015) | A runtime switch still ships the catalogs |
