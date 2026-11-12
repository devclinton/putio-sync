# Implementation Plan: Plugin Links and a Plugin Registry

**Branch**: `059-plugin-registry` | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/059-plugin-registry/spec.md`

## Summary

Four parts, in story order:

1. **Links (US1)**: six optional fields on `Manifest` (`author`, `license`, `homepage`, `docs`,
   `repository`, `issues`), checked in `Manifest::check`, returned as `links` on `PluginStatus`,
   shown on the plugin's tab and its **Available plugins** line. The seven bundled manifests fill
   them in. `putio-sync plugins check <folder>` reuses `discover::from_folder` plus a Lua parse.
2. **Registry client (US2)**: a `registry` module in `putio-sync-plugins` that fetches the index
   only when asked (6 h cache in `plugin-data/registry-index.json`), downloads and checks a release
   archive in memory (SHA-256, paths, sizes, manifest against entry), and writes
   `plugins/<name>/` through a dot-folder and a rename, with an install record
   `.registry.json`. Routes under `/plugins/registry`; **More plugins** on the Plugins page.
3. **Updates (US3)**: preview, then update with the previewed SHA-256. The previous folder is kept
   as `.previous-<name>` until the new one loads, else swapped back. Remove deletes only folders
   with an install record. The doctor warns on withdrawn and unlisted installs from the cached
   index.
4. **Registry repository (US4)**: already drafted at `~/dev/putio-sync-plugin-registry` (README,
   CONTRIBUTING, REVIEW, template, examples, `scripts/registry.py`, CI). Its format is
   [contracts/registry-index.md](contracts/registry-index.md). Remaining: publish it, attach the
   example archives to an `examples-v1.0.0` release, and set `PUTIO_SYNC_VERSION` once a release
   has `plugins check`.

Decisions are in [research.md](research.md); entities in [data-model.md](data-model.md).

**Since main moved (029, 055, 057)**: installs write folders through `putio-sync-plugins/src/install.rs`
(`Package`, hidden folder then rename, plugin starts off); the registry installer unpacks and checks
the archive, then hands the folder's files to it rather than writing its own. `plugins check` reuses
the workbench's checks in `source.rs`. Plugins may be `runtime = "wasm"` with an `entry` and may
declare `hooks`; registry entries and the index carry `runtime` and `hooks` so people see them before
installing, and the archive check compares them with the manifest.

## Technical Context

**Language/Version**: Rust 1.96 (workspace); vanilla ES modules for the UI; Python 3.11+ (stdlib)
for the registry repository's script.

**Primary Dependencies**: existing `reqwest`, `mlua`, `toml`, `serde_json`, `axum`, `clap`. New in
`putio-sync-plugins`:
- `sha2` (already in the lock file): archive SHA-256.
- `flate2` (already a dev-dependency of the CLI) and `tar`: `.tar.gz` archives.
- `zip` with `default-features = false, features = ["deflate"]`: `.zip` archives.
- `semver`: release and `min_putio_sync` comparison.

**Storage**: files only. `plugin-data/registry-index.json` (cache), `plugins/<name>/.registry.json`
(install record), `[plugin_registry]` in `config.toml`.

**Testing**:
- Unit tests in `putio-sync-plugins` for manifest links, index parsing, archive checks (archives
  built in the test: links, `..`, two top folders, size and count limits, wrong SHA-256, manifest
  mismatch), and install, update, rollback and remove on a `tempfile` plugins folder.
- A `mockito` registry for fetch, cache, offline, redirects, size caps, and "off means no request"
  (SC-004: the mock expects zero hits).
- API tests in `putio-sync-api/tests/` for the new routes and auth.
- A CLI test for `plugins check` with `assert_cmd`.
- `node --test` for the new helpers in `tests/ui/plugins.test.mjs`.
- A doctor test for withdrawn and unlisted warnings.
- Registry repository: `registry.py check --offline` on the examples in its CI.

**Target Platform**: the daemon on Linux (NAS containers), macOS and Windows; browser UI.

**Project Type**: Cargo workspace with an embedded web UI, plus one small separate repository.

**Performance Goals**: listing parses an index of up to 2 MB in well under a second; installing a
5 MB archive is bound by download time.

**Constraints**: no background registry traffic; nothing written on any failed check; an update
never loses settings, secrets, approvals or store; on Windows a rename over an existing folder
fails, so the old folder is renamed away first (R6).

**Scale/Scope**: tens to low hundreds of registry entries; up to 32 loaded plugins (existing cap).

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Registry logic in `putio-sync-plugins`; `api` adds routes, `cli` adds `plugins check`, `core` adds the `[plugin_registry]` config, `doctor` reads the cache through the service. No upward dependency. ✅ |
| II. Local-first | Everything runs in `make ci`; registry tests use `mockito`, never GitHub. ✅ |
| III. Tests without Put.io | No Put.io calls. ✅ |
| IV. Download integrity | Put.io downloads untouched. Plugin archives are SHA-256-checked before use. ✅ |
| V. Observable by default | Two counters with bounded labels (R12); install, update, remove and fetch failures are logged with the plugin's name in the log, not in metrics. ✅ |
| VI. Secrets stay secret | No new secrets. Updates leave `[plugins.secrets]` alone; registry text never includes config values. Links refuse schemes other than http(s). ✅ |
| VII. Simplicity | Four new dependencies, each justified above; no new crate; two settings (`enabled`, `url`); no signing or accounts. `.zip` support is the one extra (R5). ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/059-plugin-registry/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── api.md
│   ├── registry-index.md
│   └── cli-and-manifest.md
├── checklists/requirements.md
└── tasks.md            # /speckit-tasks
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── config.rs                    # + plugin_registry: PluginRegistryConfig
└── plugins.rs                   # + link rule helper (shared by manifest and config checks)
putio-sync-plugins/
├── Cargo.toml                   # + sha2, flate2, tar, zip, semver
├── bundled/*/plugin.toml        # + author, license, docs, repository, issues
└── src/
    ├── manifest.rs              # + link fields and checks
    ├── discover.rs              # from_folder pub; clean or restore .install-* and .previous-*
    ├── service.rs               # links and registry on PluginStatus; reload_one; registry handle
    └── registry/
        ├── mod.rs               # Registry: config, cache, state per entry, install lock
        ├── index.rs             # Index, Entry, Release parsing and per-entry checks
        ├── fetch.rs             # HTTP client with limits and the redirect rule
        ├── archive.rs           # SHA-256, tar.gz and zip reading, path and size checks
        └── install.rs           # write, record, update with rollback, remove, diff, changed
putio-sync-api/src/routes/system/
├── plugins.rs                   # + /registry, /registry/preview, /install, /update, /remove
└── settings.rs                  # + plugin_registry
putio-sync-cli/src/
├── cli.rs                       # + Plugins { Check { folder, json } }
└── commands/plugins.rs          # new: plugins check
putio-sync-doctor/src/checks/plugins.rs   # withdrawn and unlisted warnings
putio-sync-telemetry/src/plugins.rs       # registry counters
putio-sync-api/ui/
├── plugins.js                   # + linkOk, links, filterEntries, entryState, diffText
├── settings/plugins.js          # links row, More plugins, Install/Update/Remove dialogs
├── app.css                      # More plugins list
└── i18n/*.json                  # new strings (en, then i18n-translate)
putio-sync-api/tests/            # plugins_registry.rs; ui/plugins.test.mjs
PLUGINS.md, config.example.toml, example-config.toml, README.md, CHANGELOG.md, ROADMAP.md
site/ docs pages for plugins     # registry section, link to CONTRIBUTING
```

Separate repository (drafted): `~/dev/putio-sync-plugin-registry`, to become
`devclinton/putio-sync-plugin-registry`.

**Structure Decision**: one new module tree in the plugins crate and one new CLI command file;
everything else is edits to existing files.

## Complexity Tracking

No violations.
