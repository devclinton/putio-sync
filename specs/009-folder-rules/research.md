# Research: Per-folder file rules

## R1. One path for Put.io and local

- **Decision**: A rule's key is the folder path relative to the Put.io root, segment by segment, after the same character replacement `list_all_files` applies (`/ \ : * ? " < > |` → `_`). That is exactly the `relative_path` the queue stores and the folder under `target_directory` the file lands in.
- **Rationale**: `enqueue_tree` already builds that path while descending and stores it per queue row, so the sync, the queue pass and the UI can all look a rule up from data they already have. No extra Put.io call per file.
- **Alternatives**: Put.io folder id (survives renames, unreadable in the config file; the user chose paths); both (more code, two sources of truth).

## R2. Inheritance: per setting, nearest rule wins

- **Decision**: For a folder path `a/b/c` the candidates are the global settings, then rules for `a`, `a/b`, `a/b/c`. Each boolean (`skip`, `delete_blocked`, `delete_after_download`, `skip_existing`) comes from the deepest rule that sets it, else from `[download]` (`skip` and `delete_blocked` default to off). The type filter is folded root to leaf (R3). Each part keeps the path it came from, so the UI can show "inherited from Movies".
- **Rationale**: Lets a child override one thing ("delete after download off") without restating the parent's preset. Lookup is a hash probe per path segment.
- **Alternatives**: the deepest rule replaces everything (simple, but forces copying parents' settings into every child rule).

## R3. Type filter model

- **Decision**: A filter is `only: Option<set>` (allow list) plus `blocked: set`. Start: `only = None`, `blocked = [download].blocked_extensions`. For each rule on the path, root first:
  1. `preset` set → replace the filter with the preset's (`only` list, or `block` list with `only = None`);
  2. each `allow` extension → added to `only` when it is an allow list, and removed from `blocked`;
  3. each `block` extension → added to `blocked`.
  A file is blocked when its extension is in `blocked` (reason "blocked"), or `only` is a list and the extension is not in it, or the file has no extension (reason "not allowed").
- **Rationale**: Covers every case in the spec with two lists per rule: `preset = "video"` replaces; `allow = ["exe"]` alone unblocks a global extension; `block = ["nfo"]` alone adds to the inherited filter without dropping the global list; `preset = "audio", allow = ["jpg"]` extends a preset.
- **Alternatives**: separate "mode" field per rule (more knobs); merging allow lists across levels (makes "Everything" below "Video only" impossible without a special case).
- Extension = lowercased last extension, as `blocklist::blocked_extension` does today (`a.mkv.exe` → `exe`; `.exe` has none).

## R4. Presets

- **Decision**: Built-ins live in `putio-sync-core::rules` as `video`, `audio`, `ebooks`, `no-binaries`, `everything` (lists in spec Assumptions; `no-binaries` = today's default list plus `dll so bin deb rpm appimage run sh app`). `[download.presets.<name>]` with `only = [...]` or `block = [...]` adds a user preset or, under a built-in name, replaces it. Reset = delete the override entry. Names: `[a-z0-9][a-z0-9_-]{0,31}`.
- **Rationale**: Built-ins work with no config; edits are visible and diffable in `config.toml`; the UI shows `builtin` / `overridden`.
- **Alternatives**: writing all built-ins into the file on first save (noisy diffs, harder upgrades of the built-in lists).

## R5. Storage and saving

- **Decision**: `[download]` gains `rules` (array of tables, written as `[[download.rules]]`) and `presets` (table of tables), both skipped when empty. Saves go through the existing `update()` helper in `routes/system/settings.rs` (validate the whole candidate, `save_keeping_file_credentials`, swap in memory), then `download_manager.update_config`. `edit_table` learns arrays of tables: when the document holds `[[...]]` for the key, entries are edited in place by index (keeps comments on unchanged keys), extra ones appended, surplus removed; a new non-empty array of tables is written as `[[...]]`, not an inline array.
- **Rationale**: Keeps one config writer; without the `edit_table` change, the first UI save would turn `[[download.rules]]` into one long inline array and drop its comments (FR-011, SC-006).
- **Alternatives**: SQLite (user chose config); a separate `rules.toml` (second writer and reload path).

## R6. Delete blocked files

- **Decision**: In `enqueue_tree`, files blocked by the type filter (not skipped) whose effective `delete_blocked` is on, and that are not in the queue, are collected and deleted with one `delete_files` call per sync, then recorded as `deleted`. A failed delete logs a warning; the file is tried again next sync.
- **Rationale**: One Put.io call per sync; skipped folders are never deleted (FR-015); the queue guard matches 008's.
- **Dry run (Principle IV)**: `POST /rules/preview` runs the same decision code over a folder's tree with a candidate rule and returns what would be blocked, skipped and deleted, with no side effects. The UI shows it, and saving a rule that turns delete on asks for confirmation with the preview's count.

## R7. Recording decisions

- **Decision**: New table `rule_decisions` (one row per file id: name, folder path, decision `blocked|skipped|deleted`, reason, rule path or NULL for global, time). Written the first time per process a file is decided (the existing `blocked_warned` set, renamed `decided`), and on delete; removed when the file is queued. Exposed by `GET /rules/decisions` (newest first, limit) and cleared by `POST /rules/decisions/clear`.
- **Rationale**: Bounded writes (no row churn each sync), answers "why didn't this download?" (SC-004).
- **Metric**: `putio_sync_rule_decisions_total{decision}` with `decision` ∈ `blocked`, `skipped`, `deleted` (closed label enum, no paths). `putio_sync_files_blocked_total` keeps counting blocked and skipped files once per file id, so existing dashboards keep working.

## R8. Preview cost

- **Decision**: Preview lists the folder's tree with the same `list_tree` the queue uses (one `list_files` per folder), returns counts plus the first 200 items, and is triggered only by the user.
- **Rationale**: Exact, reuses code. A huge folder costs as much as "Download" on it, which the UI already allows.

## R9. Where the code lives (Principle I)

- `core::rules`: types, built-in presets, validation, path normalisation, resolution and decisions (pure, unit-tested). Used by `download` and `api`.
- `download`: applies decisions in `enqueue_tree`, per-file delete-after-download / skip-existing, preview, folder path of a Put.io id.
- `db`: `rule_decisions` table and repository.
- `telemetry`: the new counter and label enum.
- `api`: `/rules` routes and UI.
