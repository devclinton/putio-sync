# Research: Download Verification

## R1 Where the CRC32 comes from

- **Decision**: use `FileEntry.crc32` from `get_file_details`, which `process_queue` already
  calls just before each download, so `DownloadFile.crc32` is filled with no extra call. A
  value that is not 8 hex digits after trimming is treated as absent.
- **Rationale**: no new Put.io traffic; 040 already relies on the same field and algorithm
  (CRC-32/ISO-HDLC).
- **Alternatives**: a separate call per file after the download (wasteful); storing the
  CRC32 at enqueue time (stale if Put.io recomputes it).

## R2 Hashing while streaming

- **Decision**: a `Crc32` newtype around `crc::Digest<'static, u32, Table<16>>` in
  `files.rs`. `stream_to_temp` takes `Option<Crc32>` (the digest so far) and updates it in
  the disk writer task, chunk by chunk, in write order; it returns the hex value in
  `StreamDownloadResult.crc32`. For an attempt that starts at byte `from > 0`, `transfer`
  first hashes the first `from` bytes of the hidden file (`crc32_prefix`), with the file
  shown as "verifying" on the Queue page meanwhile.
- **Rationale**: a fresh download is never read back (SC-002); a resumed one reads only its
  prefix, which is less than 040's whole-file re-read. The writer task already sees every
  byte in order.
- **Alternatives**: hashing in the network loop (same cost but ahead of the write, so a
  failed write could hash bytes never written); `crc32fast` (another dependency for no gain).

## R3 Re-download loop

- **Decision**: in `download_file`, after `transfer` succeeds, compare; on a mismatch delete
  the hidden file and `transfer(.., None)` again, up to `verify_max_redownloads` (0–5,
  default 2). Each mismatch sends `verify.failed`; each new download sends
  `verify.redownload`. When the limit is spent, fail with
  `"Checksum mismatch: <name> did not match Put.io's CRC32 <expected> (got <actual>) after N downloads"`,
  which `fail_reason` already maps to `integrity`, and record a `verify_problems` row.
- With `verify = false` the old 040 rule stays: a resumed file is still compared and
  restarted once; a fresh one is checked by size only.
- **Rationale**: keeps the retry close to the bytes; the queue's own failure handling then
  sees one failed download with a clear reason.

## R4 Ordering with media-server refresh (031)

- **Decision**: no change to 031. Verification happens before `finish_download` renames the
  hidden file, and `download.finished` is sent after the rename, so the 031 plugins (which
  react to `download.finished`) never see an unverified or bad copy. `verify.passed` is sent
  just before `download.finished`.
- The post-download delete from Put.io runs only on `result.success`, which now means
  verified (FR-018).

## R5 The verify run

- **Decision**: `VerifyRunner` in `putio-sync-download/src/verify.rs`, owned by the
  `DownloadManager` (one per manager; the API's manager starts runs). It:
  1. Lists `downloads` rows with status `completed` (all, or those whose `local_path` is in
     the chosen set).
  2. Walks Put.io once (`walk_files`) for id → size and crc32; a file whose walk entry has no
     crc32 gets one `get_file_details` call. A file absent from a complete walk is "not on
     Put.io"; absent from an incomplete walk it is "not checked".
  3. For each file: missing → `missing`; path in use or queue row `downloading` → `busy`;
     size differs → `mismatch`; else hash at the read limit and compare.
  4. A mismatch is queued with `redownload = 1` (overwrite whatever is there) unless the run
     is report-only, and sends `verify.failed` and `verify.redownload` with
     `trigger = "run"`.
- Pause/resume/stop through a `tokio::sync::watch` of the desired state, checked between
  1 MiB blocks; Pause takes effect within one block (SC-003).
- Read rate: a `RateLimiter::new(limit)` of its own, re-set when the config changes.
- 032 pause: when `verify_follow_bandwidth_pause` is on and the manager's shared limiter
  `is_paused()`, the run waits (state `waiting`, reason "bandwidth paused").
- **Rationale**: the download crate owns local files and the queue; one walk scales to large
  libraries better than one call per file.

## R6 Storage

- **Decision**: `downloads` gains `crc32 TEXT`, `verify TEXT`, `verified_at TIMESTAMP`;
  `download_history` gains `verify TEXT` and `redownloads INTEGER`; `download_queue` gains
  `redownload INTEGER NOT NULL DEFAULT 0`; new tables `verify_runs` and `verify_problems`
  (see data-model.md). Runs older than the last 10 are pruned with their problems.
- **Rationale**: History's `action` CHECK cannot grow without rebuilding the table;
  separate tables keep run output out of the download history.

## R7 Doctor

- **Decision**: `CheckId::Verification` ("verification", "Download verification",
  Area::Sync), reading the database only: warn when verification is off, when an unfixed
  `download` problem is under 7 days old, or when the last run has unfixed mismatches; ok
  otherwise, saying when the last run was. A problem is fixed when the same file id later
  passes (download or run).

## R8 Metrics

- `putio_sync_verify_total{trigger,method,result}`: trigger `download|run`, method
  `crc32|size`, result `passed|failed`.
- `putio_sync_verify_redownloads_total{trigger}`.
- `putio_sync_verify_run_files_total{result}`: `verified|mismatch|missing|not_on_putio|busy|unreadable|not_checked`.
- `putio_sync_verify_read_bytes_total`: bytes hashed from disk (prefixes and runs).
- `putio_sync_verify_run_active`: 1 while a run goes.

## R9 UI

- History: a "Verified" column (CRC32, size, —, with "re-downloaded N×"), and a
  "Verification" panel above the list with the last run's summary, Verify everything /
  Report only, Pause, Resume, Stop, live progress (polled every 2 s while running), and
  the problem list.
- Local files (045): a "Verify" bulk action on selected file rows.
- Queue: phase `verifying` reads "Verifying" with progress; the 039 pipeline view shows a
  file in that phase as "Verifying".
- Settings › Download: the four keys.
- Helpers in `ui/verify.js` (DOM-free, node-tested).
