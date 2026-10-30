# Research: What to do when a file already exists

## What the code does today

- `DownloadManager::download_file` (`putio-sync-download/src/manager.rs`) skips the file when
  `config.effective(relative_path).skip_existing` is on and `target_path.exists()`, with no size
  check. Otherwise it streams into `.<name>.downloading` (`files::create_temp_path`) and
  `stream.rs` renames it over the target after a size check. That is how "off" overwrites.
- `files::check_resume` resumes only the hidden file: smaller than the remote size → range
  request; equal or larger → deleted and restarted.
- `resume_enabled` (`putio-sync-core/src/config.rs`) and the CLI's `--no-resume`
  (`putio-sync-cli/src/commands/download.rs`) are read and logged, then never used.
- The queue worker calls `get_file_details` before each download (`manager.rs`, queue pass), so
  Put.io's `crc32` (`putio-sync-client/src/models.rs`) is already at hand there, but
  `DownloadFile` does not carry it.
- Folder rules (009) resolve `skip_existing` per folder through `RuleSet::resolve` →
  `Effective.skip_existing: Sourced<bool>` (`putio-sync-core/src/rules.rs`).
- Radarr/Sonarr items (011) list their files in `arr_item_files`, and count a file as done when a
  `downloads` row for its Put.io id is `completed` (`ArrRepository::local_progress`). Their output
  path is `<target>/<category>/<root name>`, from `enqueue_tree_report`.
- `downloads.local_path` stores the path written. `download_history` has no path.

## R1. One setting, `on_existing`

- **Decision**: `[download] on_existing = "resume" | "overwrite" | "keep_both"` (also accepting
  `"rename"` as an alias for `keep_both`, the user's word). It is the enum
  `ExistingFile` in `putio-sync-core::config`. Folder rules get `on_existing:
  Option<ExistingFile>`, and `Effective.skip_existing` becomes `Effective.on_existing:
  Sourced<ExistingFile>`.
- **Compatibility**: `DownloadConfig.on_existing` is an `Option`. When it is absent,
  `skip_existing` decides (`true` → resume, `false` → overwrite). The same applies to each rule.
  `skip_existing` stays readable but is no longer written: a save from the web UI writes
  `on_existing` and removes `skip_existing` through `edit_table`, keeping comments.
- **Rationale**: one enum replaces the bool everywhere it is resolved, so the nearest-rule logic
  from 009 is unchanged.
- **Alternatives**: a second bool next to `skip_existing` (two knobs that can contradict each
  other); migrating config files on load (rewrites files the user did not ask to change).

## R2. The decision is a pure function

- **Decision**: `files::plan_existing(local, remote, choice, flags) -> ExistingAction` in
  `putio-sync-download`. Inputs: what is at the target path (nothing, folder, or file with size),
  remote size and optional CRC32, the choice, `resume_enabled`, and whether the path is fixed
  (R6). Output: `Fresh`, `Skip`, `Resume { from }`, `Overwrite`, `SaveAs { path, reason }` or
  `Refuse`. The CRC32 of the local file is computed only when the sizes are equal and Put.io
  has a checksum.

  | Local file | resume | overwrite | keep both |
  |---|---|---|---|
  | none | Fresh | Fresh | Fresh |
  | folder | Refuse | Refuse | Refuse |
  | smaller | Resume (Overwrite if resume is off) | Overwrite | SaveAs |
  | same size, checksum matches or none | Skip | Overwrite | Skip |
  | same size, checksum differs | SaveAs (checksum) | Overwrite | SaveAs |
  | larger | SaveAs (larger) | Overwrite | SaveAs |

  With a fixed path (R6), every `SaveAs` becomes `Overwrite`.
- **Rationale**: the whole spec table is unit-testable without a server.
- **Alternatives**: branching inline in `download_file` (already long; hard to test each cell).

## R3. Resuming a partial file at the real name

- **Decision**: rename the real-name file to its hidden `.<name>.downloading` path (same folder,
  so the rename is atomic), then use the existing resume path. After the resume, when Put.io has a
  checksum, compute the whole file's CRC32. On a mismatch, delete the hidden file and download
  once from the start; a second mismatch fails the file.
- **Rationale**: no copy of a large file, and a half-resumed file never sits at the real name
  (FR-006). The partial file was useless at the real name anyway.
- **Alternatives**: copying the partial file into the hidden one (doubles disk I/O and space);
  appending in place (a half-resumed file would be visible at the real name).

## R4. CRC32

- **Decision**: use the `crc` crate (3.4, already in `Cargo.lock` through `sqlx`) with
  `CRC_32_ISO_HDLC` and a 16-slice table, as a direct dependency of `putio-sync-download`.
  `files::crc32_file(path)` reads in 1 MiB blocks on `spawn_blocking`, and returns lowercase hex
  so it can be compared with Put.io's string. Feature 034 can reuse it.
- **Rationale**: no new crate in the tree. A 16-slice table runs well above NAS disk speed.
- **Alternatives**: `crc32fast` (faster, but a new crate for no visible gain on spinning disks);
  hashing in the stream while downloading (does not help with existing files, which this feature
  is about; 034 can add it).
- **Progress**: while a file is checked, its `ActiveDownload` gets `phase = "checking"`, with
  bytes read as progress, so the Queue page shows "Checking" instead of a stalled download.

## R5. Numbered names

- **Decision**: `files::numbered_name("Show.S01E01.mkv", 1)` → `Show.S01E01 (1).mkv`; no
  extension → `name (1)`. A leading-dot name (`.hidden`) has no extension. The first free `n`
  from 1 has neither `name (n).ext` nor its hidden `.downloading` file on disk, and is not
  reserved by another worker (R7).
- **Resume across restarts**: the chosen name is stored in a new nullable column
  `download_queue.save_as`. A later attempt for the same queue row uses it and does not pick a
  new number (FR-010). The column is cleared when the row completes or is removed.
- **Rationale**: scanning for `.name (k).downloading` files cannot tell which belongs to which
  Put.io file. One column can.
- **Alternatives**: keeping the name in memory only (lost on restart, so a second number is
  picked).

## R6. Radarr/Sonarr items keep a fixed path

- **Decision**: a new column `download_queue.fixed_path INTEGER NOT NULL DEFAULT 0`.
  `enqueue_tree_report` gains a `fixed_path: bool` argument. The *arr service passes `true`,
  `enqueue_tree` passes `false`. The upsert keeps the flag once set. The queue worker puts it on
  `DownloadFile`, and `plan_existing` turns every `SaveAs` into `Overwrite`.
- **Rationale**: the flag is written with the queue row, so there is no race with
  `ArrRepository::set_local`, which runs after the tree is queued.
- **Alternatives**: looking up `arr_item_files` at download start (a worker can start before
  `set_local` writes the rows); treating every file under a category folder as fixed (blackhole
  users also use `sonarr/`).
- FR-017 needs no change: an item is finished only when each file has a `completed` `downloads`
  row, and that row is written only after the verified rename.

## R7. Two files, one path

- **Decision**: the manager keeps an in-memory set of target paths in use. A worker that finds
  its path in use waits for it to clear before deciding (resume and overwrite), or picks the next
  number (keep both).
- **Rationale**: workers run in parallel. Without this, two of them could write one file.
- **Alternatives**: a database lock (more code for a per-process problem).

## R8. Outcome: log, metric, history

- **Decision**: `ExistingOutcome` in `putio-sync-core` (`skipped`, `resumed`, `restarted`,
  `overwritten`, `saved_as`, `refused`), plus a closed `ExistingOutcome` label in
  `putio-sync-telemetry/src/labels.rs` and a counter
  `putio_sync_existing_files_total{outcome}`.
- One `info!` event per outcome, with structured fields `file_id`, `name`, `path`, `choice`,
  `outcome`, and `from_bytes`, `saved_as` or `reason` where they apply. It reaches the live log
  like any other info event. `Refuse` logs at `error!`.
- `download_history` gets nullable `local_path` and `outcome` columns, added by the existing
  migration pattern (`pragma_table_info` check, then `ALTER TABLE`). `/history` returns them; the
  History page shows the saved path under the name and a small badge for `resumed`,
  `overwritten` and `saved_as`. Old rows show nothing extra.
- **Rationale**: matches how 009 records rule decisions (structured log, closed-label counter).
- **Alternatives**: a separate outcomes table (History already has one row per download).

## R9. `resume_enabled` starts working

- **Decision**: the manager's `DownloadConfig` gains `resume_enabled`, copied from settings. When
  it is off, `check_resume` is skipped (the hidden file is deleted and the download starts over),
  and `plan_existing` returns `Overwrite` for a smaller local file. `--no-resume` sets it off for
  one run. The Settings page keeps the "Resume" toggle, now with working help text.
- **Rationale**: the spec forbids switches that do nothing (FR-013), and wiring it is a few lines.

## R10. CLI

- **Decision**: `putio-sync download` and `putio-sync daemon` take
  `--on-existing <resume|overwrite|keep-both>` (clap `ValueEnum`), overriding the config for the
  run. `--skip-existing` stays on both and means `--on-existing resume`. Passing both is an
  error.
