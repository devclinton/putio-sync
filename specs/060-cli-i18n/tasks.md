# Tasks: Multi-language CLI (060)

**Input**: `specs/060-cli-i18n/` (spec, plan, research R1–R12, data-model, contracts/cli.md,
contracts/config.md, contracts/catalog.md, quickstart).
**Tests**: included (the plan lists them; Principle III: bug fixes start with a failing test). No
network in tests.
**Format**: `- [ ] T### [P] [US#] description with file path`.

Conventions for every conversion task below:

- Text for a person → `t!("English {0}", value)`; the literal is the catalog key, placeholders
  `{0}`, `{1}` in argument order. Data (names, paths, Put.io/OS error text) goes in as an argument,
  never inside the literal.
- Never call `t!` in `--json`/`--csv` paths, in `output/json.rs`/`output/csv.rs`, in `tracing`
  macros, or in the `main.rs` STARTUP DEBUG block (research R9).
- `y/N` prompts → `ui::confirm` (T030).

## Phase 1: Setup

- [ ] T001 Add `sys-locale` to `[workspace.dependencies]` in `Cargo.toml` and to `putio-sync-core/Cargo.toml`; add `unicode-width` (the version already in `Cargo.lock`) to `putio-sync-cli/Cargo.toml`; add `flate2 = "1"` under `[build-dependencies]` in `putio-sync-cli/Cargo.toml`
- [ ] T002 Add feature `i18n = []` to `putio-sync-cli/Cargo.toml` and put it in `default` (`default = ["server", "i18n"]`)
- [ ] T003 [P] Create `putio-sync-cli/i18n/en.json` as `{}` and an empty JSON object file for each code in `LANGUAGES` of `putio-sync-api/ui/i18n.js` (`ar` … `zh-TW`, 26 files)

## Phase 2: Foundational (blocks all stories)

- [ ] T004 Write failing unit tests in `putio-sync-core/src/i18n.rs` (`#[cfg(test)]`): `Lang::find` matches case-insensitively, treats `_` as `-`, drops `.encoding` and `@modifier` (`de_DE.UTF-8@euro` → `de`), prefers exact codes (`pt_PT` → `pt-PT`, `zh-TW`), falls back to the base (`pt-BR` → `pt`, `zh-HK` → `zh`), returns "none" for `C`, `POSIX`, `C.UTF-8` and empty, and unknown for `xx`; `Message` `Display` fills `{0}`/`{1}` in English and in any order; a translation whose placeholder set differs from its key's returns English; the `LANGS` list equals `LANGUAGES` in `putio-sync-api/ui/i18n.js` plus `en` (read the file in the test)
- [ ] T005 Implement `putio-sync-core/src/i18n.rs` and export it from `putio-sync-core/src/lib.rs`: `Lang { code, name, rtl }` with the const list (own-language names; `rtl` for `ar`, `he`), `Lang::find`, `Source` (`Flag`, `Env`, `Config`, `Os`, `Default`), `Choice { lang, source, warnings }`, a process-wide `OnceLock` for the choice and for the loaded catalog (`HashMap<String, String>`), `set_catalog`, `tr(template: &str) -> &str` (English on a miss or placeholder mismatch), `fill(template, args)`; `enum Message { Template { template: &'static str, args: Vec<String> }, Text(String) }` with `Display` in English (`Text` as is), `localized()`, and `From<String>`/`From<&'static str>` giving `Text`; `Message` is not `Serialize`
- [ ] T006 Add `t!` and `msg!` macros in `putio-sync-core/src/i18n.rs` (`#[macro_export]`): first argument must be a string literal; arguments formatted with `to_string()`; a `const` assertion that the number of distinct `{N}` in the literal equals the number of arguments, so a mismatch fails to compile; test both macros
- [ ] T007 Write failing tests for `i18n::select(args, env, read_config)` in `putio-sync-core/src/i18n.rs`, with a fake env reader and a fake config reader: `--lang fr` and `--lang=fr` anywhere in args win; then `PUTIO_SYNC_LANG`; then `[ui] language`; then `LANGUAGE` (each colon entry tried in turn), `LC_ALL`, `LC_MESSAGES`, `LANG`; `C`/`POSIX` stop the environment step without a warning; then English with `Source::Default`; an unknown value from flag, env or config adds one warning and falls through; an unmatched OS locale adds none
- [ ] T008 Implement `select` in `putio-sync-core/src/i18n.rs`: OS fallback via `sys_locale::get_locales()` only when none of the four variables is set; config reader takes the path from `-c`/`--config` in args, then `PUTIO_SYNC_CONFIG`, then `Config::default_path()`, and parses only a `[ui]` table with `toml::from_str` (errors ignored here; the full load reports them)
- [ ] T009 Add `UiConfig { language: Option<String> }` as `[ui]` (`#[serde(default)]`) in `putio-sync-core/src/config.rs`, with `UiConfig::lang() -> Option<Lang>` (unknown → `None`); config load does not warn about it (the warning comes once from `select`, and the doctor reports it in T034); test round-trip and `lang()` in the config tests; document the section in `config.example.toml` exactly as in `contracts/config.md`
- [ ] T010 Write `putio-sync-cli/build.rs`: when `CARGO_FEATURE_I18N` is set, deflate each `i18n/*.json` except `en.json` into `$OUT_DIR/i18n/<code>.json.z` and write `$OUT_DIR/i18n_catalogs.rs` with `pub static CATALOGS: &[(&str, &[u8])] = &[("de", include_bytes!(…)), …]`; `rerun-if-changed=i18n`
- [ ] T011 Create `putio-sync-cli/src/i18n.rs` (module in `putio-sync-cli/src/lib.rs`): `init()` runs `core::i18n::select` on `std::env::args_os()`, and with feature `i18n` inflates and parses only the chosen catalog into `set_catalog`; without the feature a non-English pick becomes `en` with the warning `this build has English only.`; prints the warnings once on standard error prefixed `putio-sync: `, using the wording in `contracts/cli.md`, unless `-q`/`--quiet` is in args; never changes the exit code
- [ ] T012 Call `i18n::init()` as the first thing in `putio-sync-cli/src/main.rs`, before the STARTUP DEBUG block and argument parsing
- [ ] T013 Move `putio-sync-cli/src/cli.rs` into the library: `pub mod cli;` in `putio-sync-cli/src/lib.rs`, drop `mod cli;` from `putio-sync-cli/src/main.rs` and use `putio_sync_cli::cli::…` there (gate anything server-only with the same `#[cfg(feature = "server")]` as today); the existing `cli` unit tests keep passing. Then add the global `--lang <CODE>` argument (`global = true`, `value_name = "CODE"`, help "Language for messages and help (see `putio-sync languages`)") to `Cli` in `putio-sync-cli/src/cli.rs`; it is only read by `select`, so `Cli` just accepts it

**Checkpoint**: `cargo test -p putio-sync-core i18n` passes; `--lang de` is accepted and changes nothing yet.

## Phase 3: User Story 1 - See the CLI in my language (P1) 🎯 MVP

**Goal**: with a supported OS locale, the main commands print in that language.
**Independent test**: `LANG=de_DE.UTF-8 putio-sync status` prints German labels; `LANG=C` prints English.

### Tests

- [ ] T014 [US1] Write a failing test in `putio-sync-cli/src/output/table.rs`: a `FileEntry` whose name is 45 CJK characters (or any name with a multi-byte character across byte 37) formats without panicking, and every row's columns line up by display width
- [ ] T015 [P] [US1] Write `putio-sync-cli/tests/i18n_cli.rs` (runs the built binary with `assert_cmd`-style `std::process::Command`, env cleared of `LANG`/`LC_*`/`LANGUAGE`/`PUTIO_SYNC_LANG`, `PUTIO_SYNC_CONFIG` pointing at a temp file). The language cases use the real catalogs, so mark them `#[ignore = "needs catalogs (T047)"]` until T047 fills them. Cases: `LANG=de_DE.UTF-8 … languages` prints `In use: de`; `LANG=C` prints English; `LANG=pt_BR.UTF-8` picks `pt`; `LANG=pt_PT.UTF-8` picks `pt-PT`

### Implementation

- [ ] T016 [US1] Add `pad(s, width)` and `truncate(s, width)` by display width (`unicode-width`; cut at a char boundary, add `...`) in `putio-sync-cli/src/output/mod.rs`; use them in `putio-sync-cli/src/output/table.rs` and `putio-sync-cli/src/output/tree.rs` instead of `{:<N}` and byte slicing (makes T014 pass); headers and "No files found." via `t!`
- [ ] T017 [P] [US1] Convert human output in `putio-sync-cli/src/commands/status.rs` (including `health` and `debug` text output; keep `--json` untouched); its tables use `output::pad`
- [ ] T018 [P] [US1] Convert `putio-sync-cli/src/commands/list.rs` and `putio-sync-cli/src/commands/stat.rs`
- [ ] T019 [P] [US1] Convert `putio-sync-cli/src/commands/queue.rs`; tables via `output::pad`
- [ ] T020 [P] [US1] Convert `putio-sync-cli/src/commands/transfers.rs`; tables via `output::pad`
- [ ] T021 [P] [US1] Convert `putio-sync-cli/src/commands/download.rs` and the progress messages and bar templates' words in `putio-sync-cli/src/ui/progress.rs` (indicatif placeholders such as `{bytes}` stay as they are)
- [ ] T022 [P] [US1] Convert `putio-sync-cli/src/commands/delete.rs`, `putio-sync-cli/src/commands/clean_empty.rs` and `putio-sync-cli/src/commands/free_space.rs`
- [ ] T023 [P] [US1] Convert `putio-sync-cli/src/commands/create.rs`, `putio-sync-cli/src/commands/archive.rs` and `putio-sync-cli/src/commands/history.rs`
- [ ] T024 [P] [US1] Convert `putio-sync-cli/src/commands/login.rs`, `putio-sync-cli/src/commands/setup.rs` and `putio-sync-cli/src/commands/setup_mode.rs` (prompts and instructions; the setup link URL is an argument)
- [ ] T025 [P] [US1] Convert `putio-sync-cli/src/commands/daemon.rs`, `putio-sync-cli/src/commands/server.rs` and `putio-sync-cli/src/api_listener.rs`: only console lines meant for a person (setup link, "Open … to finish setup", listening addresses); `tracing` calls stay English
- [ ] T026 [P] [US1] Convert `putio-sync-cli/src/commands/plugins.rs`, `putio-sync-cli/src/commands/mod.rs` and `putio-sync-cli/src/diagnostics.rs` (human sections only)
- [ ] T027 [US1] Convert error messages printed for a person in `putio-sync-cli/src/main.rs` after the STARTUP DEBUG block (the top-level `Error: …` line: the prefix translated, the error chain as an argument)
- [ ] T028 [US1] Doctor findings, per data-model.md "In the doctor": in `putio-sync-doctor/src/report.rs` change `Finding.summary` to `Message` and `.suggestion` to `Option<Message>` (constructors take `impl Into<Message>`); keep `CheckResult.summary: String` and `suggestion: Option<String>` and add `#[serde(skip)] pub local: Option<LocalText>` with `LocalText { summary: Message, suggestion: Option<Message> }`, left out of `PartialEq`/`Eq` (implement them by hand without `local`, or wrap it in a newtype whose `eq` is always true); in `putio-sync-doctor/src/lib.rs` (around line 222) fill the English strings with `to_string()` and `local` from the `Finding`; fix `putio-sync-notify/src/message.rs` and other readers if they break; add a test that a report's JSON is byte-identical before and after
- [ ] T029 [US1] Replace `format!` summaries and suggestions with `msg!` in every file under `putio-sync-doctor/src/checks/` (≈ 80 sites; values as arguments). In `putio-sync-cli/src/commands/doctor.rs` print `check.local` with `.localized()` when present, else the English strings; print check titles and area names through `i18n::tr(check.title)` / `i18n::tr(area)`; convert that file's own text
- [ ] T030 [US1] Add `putio-sync-cli/src/ui/confirm.rs` (`pub fn confirm(question: &str) -> io::Result<bool>`): prints the question and the translated `[y/N]` (first word of the `"y|yes"` catalog entry), accepts `y`, `yes` and the entry's words case-insensitively, everything else is no; unit-test with a reader; replace the prompts in `clean_empty.rs`, `delete.rs`, `free_space.rs`, `queue.rs`, `transfers.rs` and `doctor.rs` under `putio-sync-cli/src/commands/`; add the `"y|yes"` key to the extractor's fixed keys (T040)

**Checkpoint**: with the German catalog (after T047), quickstart §1 and §5 pass.

## Phase 4: User Story 2 - Pick the language myself (P1)

**Goal**: `--lang`, `PUTIO_SYNC_LANG` and `[ui] language` choose the language; `languages` lists them.
**Independent test**: quickstart §2.

- [ ] T031 [US2] Add the `languages` subcommand (`--json`) in `putio-sync-cli/src/cli.rs` and `putio-sync-cli/src/commands/languages.rs`: table `CODE`/`LANGUAGE` (headers and "In use: {0} (from {1})" translated; source words `--lang`, `PUTIO_SYNC_LANG`, `config`, the variable name that matched, `default`), listing only languages this build has; JSON exactly `{"current":{"code":…,"source":"flag|env|config|os|default"},"languages":[{"code","name","rtl"}]}`, never translated
- [ ] T032 [US2] Record which OS variable matched (`LANG`, `LC_ALL`…) in `Choice` in `putio-sync-core/src/i18n.rs` for the "In use" line
- [ ] T033 [US2] Extend `putio-sync-cli/tests/i18n_cli.rs`: the order flag > env > config > OS from quickstart §2; `--lang xx` prints one warning on standard error, falls back, exits 0; `-q` hides the warning; a bad `[ui] language` warns the same way; `languages --json` is identical under `--lang en`, `de`, `ja`, `ar`
- [ ] T034 [P] [US2] The doctor's Config check (`config()` in `putio-sync-doctor/src/checks/local.rs`) reports a set but unknown `[ui] language` as a warning, `msg!("[ui] language \"{0}\" is not available", v)` with a suggestion naming `putio-sync languages`; add a case next to `config_findings` in that file's tests

## Phase 5: User Story 3 - Scripts keep working in any language (P1)

**Goal**: machine output, exit codes and logs are identical in every language.
**Independent test**: quickstart §3.

- [ ] T035 [US3] In `putio-sync-cli/tests/i18n_cli.rs`, for each of `en`, `de`, `ja`, `ar`: run `status --json`, `files list --output json`, `files list --output csv`, `queue list --json`, `transfers list --json` and `doctor --json` against the existing wiremock Put.io fixtures and a temp database, and assert stdout and exit code are byte-identical to `en`; also a failing command's exit code
- [ ] T036 [US3] In `putio-sync-cli/tests/i18n_catalog.rs`, fail when `t!(` appears inside an `info!`/`warn!`/`error!`/`debug!`/`trace!` call or in `putio-sync-cli/src/output/json.rs` / `putio-sync-cli/src/output/csv.rs` (text scan of `putio-sync-cli/src` and `putio-sync-doctor/src`)
- [ ] T037 [US3] Note in `--help` of `files list` (`--output`) and in `README.md` that table output follows the language and `--output json|csv` does not, so scripts should use those

## Phase 6: User Story 4 - Translated help (P2)

**Goal**: help and common argument errors in the chosen language; names unchanged.
**Independent test**: quickstart §4.

- [ ] T038 [US4] In `putio-sync-cli/src/i18n.rs` add `localize(cmd: clap::Command) -> clap::Command`: recursively (`mut_subcommand` for every subcommand) translate `about`, `long_about`, each arg's `help`/`long_help` (`mut_args`), help headings, and the built-in `help`/`version` args' help; set a `help_template` with translated "Usage:", "Options:", "Commands:", "Arguments:"; English returns the command unchanged
- [ ] T039 [US4] In `putio-sync-cli/src/cli.rs` change `Cli::parse_args` to `localize(Cli::command()).try_get_matches()` then `Cli::from_arg_matches`; on `clap::Error`, for kinds `UnknownArgument`, `InvalidValue`, `MissingRequiredArgument`, `InvalidSubcommand`, `NoEquals`, print a translated message built from `ContextKind::InvalidArg`, `InvalidValue`, `ValidValue`, `SuggestedArg`/`SuggestedSubcommand` plus the usage line, exit 2; other kinds (`DisplayHelp`, `DisplayVersion` included) `err.exit()`; keep the existing `try_parse_from` unit tests passing
- [ ] T040 [US4] Write `putio-sync-cli/tests/i18n_catalog.rs` extraction: collect `t!("…")`/`msg!("…")` literals (handle `\"`, `\n`, raw strings `r"…"`/`r#"…"#`) from every `.rs` under `putio-sync-cli/src` and `putio-sync-doctor/src`, plus every string from walking `putio_sync_cli::cli::Cli::command()` (about, long_about, help, long_help, headings; reachable after T013), every `CheckId::title()` and `Area::title()` (iterate all variants; add an `ALL` const in `putio-sync-doctor/src/report.rs` if there is none), and the fixed keys (`"y|yes"`, help template headings, built-in help/version text); compare with `putio-sync-cli/i18n/en.json` (each key maps to itself, sorted, one-space indent, final newline); on difference fail with "CLI catalog is out of date: run `make i18n`"; with `UPDATE_I18N=1` rewrite it; also drop keys from other catalogs that `en.json` no longer has when updating
- [ ] T041 [US4] In `putio-sync-cli/tests/i18n_catalog.rs`, the catalog checks from `contracts/catalog.md`: valid JSON object of strings, no keys that `en.json` lacks, same placeholder set per key (message names language and key), ≥ 90% of keys translated per catalog (a value equal to its key counts as untranslated unless the key is a product name), file list equals `LANGUAGES` + `en`; mark the coverage check `#[ignore]` until T047
- [ ] T042 [US4] Extend `putio-sync-cli/tests/i18n_cli.rs`: `--lang de download --help` contains German descriptions and `--dry-run`, `--output` unchanged; `--lang de download --nope` prints the German unknown-argument message with the suggestion and exits 2; `--lang en --help` output equals today's (snapshot taken before T038)

## Phase 7: User Story 5 - Keep CLI catalogs current (P2)

**Goal**: one command refreshes `en.json`, one fills the translations; CI catches drift.
**Independent test**: quickstart §6.

- [ ] T043 [US5] `Makefile`: `i18n` also runs `UPDATE_I18N=1 cargo test -p putio-sync-cli --test i18n_catalog en_json_current`; `i18n-translate` also runs `node scripts/i18n-translate.mjs --catalog-dir putio-sync-cli/i18n $(LANGS)`; update the targets' `##` help text
- [ ] T044 [US5] `scripts/i18n-translate.mjs`: add `--catalog-dir <dir>` (default `putio-sync-api/ui/i18n`; positional language list still works); when the dir is not the web UI's, first copy translations for keys the web UI catalog of the same language has (FR-018), then send only the rest; keep `{N}` placeholders (reject a returned translation whose placeholder set differs, leaving the key missing)
- [ ] T045 [P] [US5] Document the CLI catalogs next to 051's in `MAINTAINING.md`: where they are, `t!` vs `msg!`, `make i18n`, `make i18n-translate`, and what must not be translated
- [ ] T046 [US5] Run `make i18n`; check `putio-sync-cli/i18n/en.json` reads as expected (no data strings, no log messages)
- [ ] T047 [US5] Run `make i18n-translate` for all 26 languages through atranslate (maintainer step; needs `ATRANSLATE_URL`); commit `putio-sync-cli/i18n/*.json`; spot-check `de`, `ja`, `ar` for kept placeholders and nothing left in English; remove the `#[ignore]`s from T015 and T041

## Phase 8: User Story 6 - Small and English-only builds (P3)

**Goal**: lite builds keep translations within budget; an English-only build exists.
**Independent test**: quickstart §7.

- [ ] T048 [US6] Add `i18n` to both lite builds' `--features` in `.github/workflows/release-binaries.yml` (`hooks,i18n` and `hooks,plugins,i18n`, both jobs) and in the `lite` and `lite-plugins` builds in `Dockerfile.dhi` (lines 42–43); update the build commands in `INSTALLATION.md` and the CHANGELOG lite entry, and describe the English-only build (leave out `i18n`)
- [ ] T049 [US6] Test in `putio-sync-cli/tests/i18n_cli.rs`, `#[cfg(not(feature = "i18n"))]`: `--lang de languages` prints the English-only warning and lists only `en`; and add a `make ci` step (or extend the existing lite check, if any, in `Makefile`) that runs `cargo test -p putio-sync-cli --no-default-features --features hooks --test i18n_cli`
- [ ] T050 [US6] Measure: release-build full, lite and lite English-only binaries before and after; record sizes in `specs/060-cli-i18n/research.md` R7; if a binary grows by more than 400 KiB (SC-004), raise the deflate level or report back before going on

## Phase 9: Web UI default (FR-006)

- [ ] T051 [P] Add `GET`/`PUT /settings/ui` in `putio-sync-api/src/routes/system/settings.rs` per `contracts/config.md` (`{"language": "de" | null}`; `""` or `null` clears; unknown code → 400 `unknown language "xx"`), saved with the same config writer as the other groups; tests in the settings route tests with the mocked backend
- [ ] T052 Rename "Browser default" to "Automatic" in the Language card (`putio-sync-api/ui/settings/general.js`) and make `choice()`/boot in `putio-sync-api/ui/i18n.js` use the saved browser choice, else `ui.language` from `/settings/ui`, else the browser's languages, else English; the sign-in page skips the settings call; run `make i18n` for the web UI catalog and translate the one new string; update `putio-sync-api/tests/ui/i18n.test.mjs`

## Phase 10: Polish

- [ ] T053 [P] `README.md`: a short "Language" section (order of sources, `languages`, what stays English)
- [ ] T054 [P] `CHANGELOG.md` `[Unreleased]` › Added: the CLI in the 051 languages, `--lang`, `PUTIO_SYNC_LANG`, `[ui] language`, `putio-sync languages`, English-only builds; › Fixed: tables no longer break on wide or multi-byte names
- [ ] T055 [P] `ROADMAP.md`: mark 060 done with a one-paragraph summary and what's left (native review, plurals, daemon messages in the web UI)
- [ ] T056 Measure start-up (quickstart §8); the gap must be under 10 ms (SC-005); record it in research R7
- [ ] T057 Run every quickstart section; then `make ci` passes

## Dependencies

- Phase 1 → Phase 2 → everything else.
- US1 (Phase 3) and US2 (Phase 4) depend only on Phase 2; US2's tests that check real languages wait for T047.
- US3 (Phase 5) depends on the US1 conversions (T017–T029) being done, since it checks they left machine output alone.
- US4 (Phase 6): T038–T039 depend only on Phase 2; T040 (extraction) needs the macros (T006) and should land before T046.
- US5 (Phase 7): T046 after all conversions (Phases 3, 4, 6) so `en.json` is complete; T047 after T046.
- US6 (Phase 8): after T047 (sizes need real catalogs).
- Phase 9 depends only on T009 (`UiConfig`).
- Polish last.

## Parallel examples

- Phase 3: T017–T026 touch different files and can run together once T016 is in (they share `output::pad`).
- T028–T029 (doctor) in parallel with T017–T026.
- Phase 6's T038–T039 in parallel with Phase 3 conversions.
- Phase 9 (T051, T052) in parallel with Phases 3–8.
- T045, T053, T054, T055 in parallel at the end.

## Implementation strategy

1. **MVP**: Phases 1–3 plus T040, T046 and a German-only `make i18n-translate de` — the CLI speaks German from the OS locale with machine output unchanged.
2. Add US2 (choice and `languages`) and US3 (byte-identical test).
3. Add US4 (help), then translate all languages (T047).
4. US6 sizes and lite builds, Phase 9, polish.
