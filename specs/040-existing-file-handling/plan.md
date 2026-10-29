# Implementation Plan: What to do when a file already exists

**Branch**: `040-existing-file-handling` | **Date**: 2026-10-28 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/040-existing-file-handling/spec.md`

## Summary

A new setting, `[download] on_existing = "resume" | "overwrite" | "keep_both"`, replaces the
`skip_existing` bool. Old configs still load (`true` → resume, `false` → overwrite), globally
and in folder rules. It resolves per folder through the 009 `RuleSet`, as `skip_existing` does
today.

A pure function, `files::plan_existing`, maps what is at the target path, the remote size and
CRC32, the choice, `resume_enabled` and a fixed-path flag to one action: fresh, skip, resume,
overwrite, save-as or refuse. `DownloadManager::download_file` carries that action out:

- **Resume**: the partial real-name file is renamed to its hidden `.downloading` file, the
  existing range-resume path continues it, and the whole file is checked against Put.io's CRC32
  afterwards.
- **Overwrite**: the new copy only replaces the old file once it is complete.
- **Keep both**: the numbered name is saved on the queue row, so a restart resumes the same
  name.

Radarr/Sonarr items are queued with `fixed_path`, so they overwrite instead of getting a
numbered name. `resume_enabled` and `--no-resume` start working. Each outcome is logged with
structured fields, counted by `putio_sync_existing_files_total{outcome}`, and stored on the
History row with the saved path. Settings, the rule editor, History and Queue show it.

Decisions: [research.md](research.md) R1–R10.

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. Browser: ES2020 modules,
no build step (feature 004).

**Primary Dependencies**: existing, plus `crc` 3.4 as a direct dependency of
`putio-sync-download`. It is already in `Cargo.lock` through `sqlx`, so no new crate enters the
tree (R4).

**Storage**: `config.toml` (`on_existing`, per-rule `on_existing`). SQLite: `download_queue`
gets `save_as` and `fixed_path`; `download_history` gets `local_path` and `outcome`. All four
are added by the existing `ALTER TABLE` migration pattern.

**Testing**: `cargo test`. Unit tests for `plan_existing`, `numbered_name` and `crc32_file`.
mockito plus temporary folders for the download manager (extending
`tests/interrupted_resume.rs`). API route tests. The UI is checked by hand through `mock_ui`.

**Target Platform**: Linux Docker on the NAS; desktop builds unaffected.

**Project Type**: Cargo workspace (daemon + CLI + HTTP API + embedded web UI).

**Performance Goals**: no extra Put.io calls, since the queue worker already fetches file
details. A checksum is computed only for a same-size file that has a Put.io CRC32, and after a
resume. That is one sequential read, above NAS disk speed.

**Constraints**:

- A partial file never sits at the real name.
- The old file survives a failed overwrite.
- No file names or paths in metric labels.
- One config writer.
- UI stays under 200 KB uncompressed.

**Scale/Scope**: thousands of files per sync; a handful of collisions per sync in practice.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | `ExistingFile` and `ExistingOutcome` in `core` (used by `download`, `db`, `api`, `cli`). The decision, checksum and file moves in `download`, which already owns the hidden-file handling in `files.rs`. Columns in `db`, the counter in `telemetry`, routes and UI in `api`, flags in `cli`. `download` learns of Radarr/Sonarr only through a queue column, not by depending on `arr`. ✅ |
| II. Local-first | Everything runs in `make ci`. ✅ |
| III. Tests without Put.io | Pure decision table; mockito with small payloads, including range and failure mid-stream; `mock_ui` for the UI. ✅ |
| IV. Download integrity | Strengthened. Resume and overwrite still write to the hidden file and rename atomically. A resumed file is checked against CRC32, and the size check stays. A complete file is now judged by size plus CRC32, where today any file counts. A "skipped" file gets a `completed` row only after that check, so delete-after-download and the *arr completion still wait for a verified copy. No mode deletes a local file other than by atomic replace in "overwrite". ✅ |
| V. Observable | One structured `info` event per outcome (`error` for refused), a new counter with a closed label set, and outcome and path on History. ✅ |
| VI. Secrets | No new secrets. Routes stay behind the token. Saves keep file credentials. ✅ |
| VII. Simplicity | One enum replaces one bool. The dead `resume_enabled` switch is wired rather than left. The one new direct dependency is already in the tree (R4). No new crate or module beyond functions in `files.rs`. ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/040-existing-file-handling/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── config-and-cli.md
│   └── api.md
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks, not created here
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── config.rs           # ExistingFile, ExistingOutcome; DownloadConfig.on_existing + on_existing(); save drops skip_existing
└── rules.rs            # FolderRule.on_existing; RuleSet/Effective.on_existing replaces skip_existing
putio-sync-download/
├── Cargo.toml          # crc = "3.4"
├── src/files.rs        # plan_existing, LocalState, ExistingAction, numbered_name, crc32_file
├── src/manager.rs      # DownloadConfig.on_existing/resume_enabled; download_file runs the action;
│                       # DownloadFile.crc32/save_as/fixed_path; ActiveDownload.phase ("checking");
│                       # path-in-use set; enqueue_tree_report(fixed_path)
├── src/stream.rs       # unchanged API; called with the numbered target for save-as
└── tests/interrupted_resume.rs, tests/existing_files.rs (NEW)
putio-sync-db/src/
├── schema.rs, lib.rs   # migrations: download_queue.save_as/fixed_path, download_history.local_path/outcome
├── queue.rs            # add(..., fixed_path) upsert keeps 1; set_save_as / clear
└── history.rs          # insert(..., local_path, outcome); query returns them
putio-sync-telemetry/src/
├── labels.rs           # ExistingOutcome label enum
└── download.rs         # existing_files_total{outcome}
putio-sync-arr/src/service.rs      # enqueue_tree_report(root, fixed_path = true)
putio-sync-api/src/routes/
├── system/settings.rs  # on_existing in Download settings and DOWNLOAD_LIVE; old skip_existing mapped
├── system/rules.rs     # RuleBody/Effective on_existing
├── system/history.rs   # local_path, outcome
└── local/downloads.rs  # phase
putio-sync-api/ui/
├── pages/settings.js   # select replaces the Skip existing toggle; Resume help text
├── rules.js, pages/putio.js       # rule select with "Use global", card shows the source
├── pages/history.js    # saved path + outcome badge
└── pages/queue.js      # "Checking"
putio-sync-api/examples/mock_ui.rs # seeded outcomes and a checking download
putio-sync-cli/src/
├── cli.rs              # --on-existing on download and daemon; conflicts_with skip-existing
└── commands/download.rs # resume_enabled / --no-resume actually passed to the manager
putio-sync-daemon/src/daemon.rs    # settings → manager config (on_existing, resume_enabled)
README.md, config.example.toml, CHANGELOG.md
```

**Structure Decision**: existing workspace layout. New logic goes into the module that already
owns hidden-file handling (`putio-sync-download/src/files.rs`), and no new crate is added.

## Complexity Tracking

None.
