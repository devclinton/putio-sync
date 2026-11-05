# Feature Specification: Download Verification

**Feature Branch**: `034-download-verification` (built on `claude/infallible-mclaren-082190`)

**Created**: 2026-11-04

**Status**: Draft

**Input**: User description: "034 Download verification: check each finished download
against Put.io's CRC32 (or size where there is none), re-download files that do not match,
and offer a "verify everything" run for files already downloaded. Results show in History
and in the doctor. Compute the CRC32 while streaming so a large file is not read twice; the
verify-everything run reads from disk at a limited rate, is pausable, and respects 032
bandwidth schedule pauses; limited re-downloads per file, then fail with a reason; emit 027
events for verify passed, verify failed and re-download (new types, schema v1 stays);
verification happens before download.finished so 031 media-server refresh only sees
verified files; cover config, doctor, metrics, a UI action, and the 039 pipeline view and
045 local files list where they fit."

## Context

- Put.io's file details carry a `crc32` for most files (the field is missing for some, such
  as files still being processed). The queue already fetches a file's details from Put.io
  just before it downloads it, so the CRC32 is in hand when the download starts; no extra
  call is needed.
- Today every download is checked against its size only. Feature 040 added a CRC32 check
  for two cases: a same-size local file met before a download (to skip or keep both), and
  a resumed download, which is re-read from disk as a whole once it ends. A fresh download
  is never checked against the CRC32.
- A download is written to a hidden `.downloading` file and renamed to its real name only
  when it succeeds. `download.finished` (027) is sent after the rename. The 031 media-server
  plugins (Plex, Jellyfin, Emby) refresh a library when they see `download.finished`. So a
  check done before the rename also holds back the media-server refresh, with no change to
  031.
- 032 decides the download speed limit in force, including "paused". Reading files from the
  NAS's own disk does not use the internet line, but a pause usually means "leave the NAS
  alone now".
- History lists finished downloads; the doctor (010) lists checks with ok, warn and fail.

## Clarifications

### Session 2026-11-04

Answered with the recommended option, without a live user; listed in the hand-over note
for review.

- Q: Is verification on by default? → A: Yes. It costs no extra disk reads for a fresh
  download, so `[download] verify = true` by default; it can be turned off.
- Q: How many re-downloads does a file that does not match get? → A: 2 by default
  (`verify_max_redownloads`, 0 to 5). After the last one still does not match, the download fails
  with reason `integrity` and the message says the CRC32 did not match after N downloads.
- Q: When "verify everything" finds a bad file, is it downloaded again on its own? → A: Yes
  by default: it is put back in the queue to be downloaded over the bad copy. The run can be
  started with "report only". A file that is missing from disk, or no longer on Put.io, is
  reported and never queued.
- Q: Does the "verify everything" run follow 032's pause? → A: Yes, by default
  (`verify_follow_bandwidth_pause = true`): while the schedule or an override says "paused", the
  run waits. A slow limit does not apply to disk reads; the run has its own read limit.
- Q: Does a restart carry on a run that was going? → A: No. A restart ends it; the summary
  of the last finished (or stopped) run survives a restart, and a new run starts from the
  beginning.
- Q: Does a "verify everything" run send `verify.passed` for every good file? → A: No. A
  run over thousands of files would flood each hook's queue (1 000 events, oldest
  dropped). A run sends `verify.failed` and `verify.redownload` for problem files only;
  every event carries `trigger` (`download` or `run`) so a hook can tell them apart.
- Q: Can "delete from Put.io after download" (or empty-folder cleanup) act on a file that
  failed verification? → A: No. Only a download that passed (or, with verification off,
  passed its size check) counts as a success, so the delete never sees a bad copy.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Every new download is checked before anyone sees it (Priority: P1)

A user's NAS sometimes ends up with a file that has the right size but broken contents (a
flaky connection, a proxy, a disk error). They want putio-sync to check each download
against the checksum Put.io holds, and to get the file again when it is wrong, before Plex
picks it up.

**Why this priority**: it is the feature; the other stories report on it or extend it to
old files.

**Independent Test**: serve a file from a mock Put.io whose details give a CRC32 that does
not match the bytes the first time and does the second time; the file is downloaded twice,
lands once, and only one `download.finished` is sent, after the good copy.

**Acceptance Scenarios**:

1. **Given** Put.io gives a CRC32 for a file, **When** the download ends and the bytes match
   it, **Then** the file is renamed to its real name, History shows it as verified by
   CRC32, a `verify.passed` event is sent, and then `download.finished` is sent.
2. **Given** the bytes do not match, **When** the download ends, **Then** the bad copy is
   deleted, a `verify.failed` event and a `verify.redownload` event are sent, and the file
   is downloaded again from the start.
3. **Given** the file still does not match after `verify_max_redownloads` re-downloads, **When**
   the last one ends, **Then** nothing is saved at the real name, the download fails with
   reason `integrity` and a message naming the expected and actual CRC32 and the number of
   downloads, and History and the Queue show it as failed for that reason.
4. **Given** Put.io gives no CRC32, **When** the download ends with the right size,
   **Then** it counts as verified by size, and History says so.
5. **Given** a file of 20 GB is downloaded fresh, **When** it is verified, **Then** the file
   is not read back from disk to do it.
6. **Given** a download resumes from a partial file, **When** it is verified, **Then** only
   the part that was already on disk is read back, not the whole file.
7. **Given** verification is turned off, **When** a download ends, **Then** only its size is
   checked, as before, and History shows it as not verified.

---

### User Story 2 - Verify everything already downloaded (Priority: P2)

A user who has run putio-sync for a year wants to know whether the files already on the NAS
are intact. They press "Verify everything" in History, watch the progress, pause it during
the evening, and later see a list of files that did not match, which putio-sync queues to
download again.

**Why this priority**: a one-off health check for old files; new files are covered by
story 1.

**Independent Test**: record three downloaded files (one good, one corrupted, one deleted
from disk), start a run, and check the results list, the queue and the summary.

**Acceptance Scenarios**:

1. **Given** files recorded as downloaded, **When** "Verify everything" starts, **Then** each
   file still on disk and still on Put.io is read and compared, with progress (files and
   bytes done of the total) shown in History and in the status the page polls.
2. **Given** a file does not match, **When** the run reaches it, **Then** it is listed as a
   mismatch, a `verify.failed` event is sent and (unless the run is "report only") it is
   queued to be downloaded again over the bad copy, with a `verify.redownload` event.
3. **Given** a file is missing from disk or no longer on Put.io, **When** the run reaches it,
   **Then** it is listed as "missing" or "not on Put.io" and nothing else happens.
4. **Given** a run is going, **When** the user presses Pause, **Then** no more is read until
   Resume; **When** they press Stop, **Then** the run ends and its summary is kept.
5. **Given** the 032 schedule says "paused" and `verify_follow_bandwidth_pause` is on, **When** the
   run is going, **Then** it waits until the pause ends, and the page says why.
6. **Given** a read limit of 50 MB/s, **When** the run reads, **Then** it reads no faster
   than that on average.
7. **Given** a run is going, **When** another is asked for, **Then** it is refused ("a run is
   already going").
8. **Given** the user selects some rows on the Local files page, **When** they choose
   Verify, **Then** a run starts for just those files (those that putio-sync downloaded).

---

### User Story 3 - See what verification found (Priority: P2)

The user wants to see, without digging in logs, which files were verified, which were
downloaded again and which failed, and the doctor should say when something needs looking
at.

**Why this priority**: the results are only useful if they are visible.

**Independent Test**: after stories 1 and 2 run, History and the doctor show the expected
entries.

**Acceptance Scenarios**:

1. **Given** finished downloads, **When** the user opens History, **Then** each download
   shows how it was verified (CRC32, size, not verified) and how many re-downloads it took.
2. **Given** a "verify everything" run has finished, **When** the user opens History,
   **Then** its summary (when, how many verified, mismatched, missing, not on Put.io) and
   its problem files are listed.
3. **Given** a download failed verification in the last 7 days, or the last run found
   mismatches that are not yet fixed, **When** the doctor runs, **Then** its "Download
   verification" check warns and names the count and what to do.
4. **Given** verification is off, **When** the doctor runs, **Then** the check warns that
   downloads are checked by size only.
5. **Given** a file is being verified (a resumed file's prefix, or a run), **When** the Queue
   page or the pipeline view shows it, **Then** it reads "Verifying" with progress.

---

### User Story 4 - React to verification from hooks, plugins and monitoring (Priority: P3)

A user wants a notification when a file fails verification, and a graph of how often it
happens.

**Why this priority**: builds on 027, 028 and the metrics endpoint that already exist.

**Independent Test**: a hook subscribed to `verify.*` receives the three events with the
documented fields; the metrics endpoint shows the counters.

**Acceptance Scenarios**:

1. **Given** a hook or plugin subscribed to `verify.failed`, **When** a file does not match,
   **Then** it receives the event with the file, the expected and actual CRC32, the attempt
   and whether a re-download follows.
2. **Given** the metrics endpoint, **When** files are verified, **Then** counters by method
   and result, a re-download counter, the bytes a run read, and whether a run is going are
   exposed.
3. **Given** `EVENTS.md`, **When** the new types are added, **Then** they are documented,
   in the JSON Schema, and `schema_version` stays 1.

### Edge Cases

- A file's CRC32 on Put.io is in upper case or has spaces: compared without case and
  spaces.
- Put.io's CRC32 is not 8 hex digits: treated as no CRC32 (size check), with a debug log.
- A retry inside a download (dropped connection) resumes from the hidden file: the part
  already written is hashed once from disk, then the rest while streaming.
- A file that 040 skips because a same-size local file already matches the CRC32 counts as
  verified by CRC32.
- A file 040 saves under a numbered name is verified the same way.
- `verify_max_redownloads = 0`: a mismatch fails at once, no re-download.
- A run meets a file that is being downloaded right now: it is skipped as "busy".
- A file was renamed or moved on the NAS (for example by Radarr): it reports as "missing".
- A run's file is a folder or not readable: reported as "unreadable" with the error.
- The Put.io file details call fails for network reasons during a run: the file is reported
  as "not checked" with the error; the run goes on.
- In setup mode the run endpoints return 503 like every other route.
- `server --download` runs the download manager on a thread of its own; verification of new
  downloads happens there too. A run is started by the API's manager.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Each finished download MUST be checked against Put.io's CRC32 when Put.io
  gives one, and against its size otherwise, before it is renamed to its real name.
- **FR-002**: For a fresh download the CRC32 MUST be worked out from the bytes as they are
  written, without reading the file back. For a resumed download only the part already on
  disk MAY be read back.
- **FR-003**: A mismatch MUST delete the bad copy and download the file again from the
  start, up to `verify_max_redownloads` times (default 2, range 0 to 5).
- **FR-004**: When the last allowed download still does not match, the download MUST fail
  with reason `integrity` and a message that names the expected and actual CRC32 and how
  many times it was downloaded; nothing is left at the real name.
- **FR-005**: `download.finished` MUST be sent only after the file passed verification (or
  verification is off), so the 031 media-server refresh only sees checked files.
- **FR-006**: The system MUST send `verify.passed`, `verify.failed` and `verify.redownload`
  events on the 027 bus, documented in `EVENTS.md` and the JSON Schema, with
  `schema_version` unchanged. Every event MUST carry `trigger` (`download` or `run`); a run
  MUST NOT send `verify.passed`.
- **FR-007**: Configuration MUST live in `[download]`: `verify` (default true),
  `verify_max_redownloads` (0–5, default 2), `verify_read_limit_mb_per_sec` (1–10000,
  default 50) and `verify_follow_bandwidth_pause` (default true). Values out of range MUST
  be refused by config validation with a message naming the key. All four MUST be editable
  in Settings › Download and apply without a restart.
- **FR-008**: The system MUST offer a "verify everything" run over files recorded as
  downloaded, and a run over a chosen set of local paths, with "report only" as an option.
  Only one run MAY go at a time.
- **FR-009**: A run MUST read no faster than `read_limit_mb_per_sec` on average, MUST be
  pausable, resumable and stoppable, and MUST wait while 032 says "paused" when
  `verify_follow_bandwidth_pause` is on.
- **FR-010**: A run MUST report each file as verified, mismatch, missing, not on Put.io,
  busy, unreadable or not checked; a mismatch MUST be queued to be downloaded again over
  the bad copy unless the run is "report only".
- **FR-011**: A run's progress (state, files and bytes done of the total, counts by result,
  why it waits) MUST be available to the UI while it goes; the summary and problem files of
  the last run MUST survive a restart.
- **FR-012**: History MUST show for each download how it was verified (CRC32, size, not
  verified, failed) and how many re-downloads it took, and MUST show the last run's summary
  and problem files with the "Verify everything" action and its controls.
- **FR-013**: The doctor MUST have a "Download verification" check: ok when on and nothing
  is wrong; warn when off, when a download failed verification in the last 7 days, or when
  the last run found mismatches not fixed since.
- **FR-014**: Metrics MUST include verified files by method and result, re-downloads, bytes
  read by runs, and whether a run is going.
- **FR-015**: The Queue page and the 039 pipeline view MUST show a file being verified as
  "Verifying" with progress. The 045 Local files list MUST offer Verify on selected rows.
- **FR-016**: Messages, events and logs MUST NOT contain the Put.io token or download URL
  tokens.
- **FR-017**: The run endpoints MUST return 503 in setup mode.
- **FR-018**: A file that failed verification MUST NOT be deleted from Put.io by the
  post-download delete or cleanup.

### Key Entities

- **Verification result** (per download): method (`crc32`, `size`, `none`), result
  (`passed`, `failed`), re-downloads made, expected and actual CRC32.
- **Verify run**: state (idle, running, paused, waiting, stopping, finished, stopped), scope
  (everything or chosen paths), report only, started and ended times, files and bytes done
  of the total, counts per result.
- **Run problem**: one file a run did not verify: name, local path, result, detail, whether
  it was queued again, and whether it has been fixed since.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A corrupted download is caught and downloaded again in 100% of test cases
  where Put.io gives a CRC32, and no media-server refresh is triggered for the bad copy.
- **SC-002**: Verifying a fresh download adds no disk reads; the download takes no more
  than 5% longer than with verification off.
- **SC-003**: A "verify everything" run never reads faster than its limit by more than 10%
  over any 10-second window, and stops reading within 2 seconds of Pause.
- **SC-004**: After a run, the user can see every problem file and what was done about it
  from History in one page, without reading logs.
- **SC-005**: A hook receives each verification event within 1 second of it happening.

## Assumptions

- Put.io's `crc32` is the standard CRC-32 (ISO-HDLC) of the whole file, as 040 already
  relies on.
- Files are verified only if putio-sync downloaded them (it has a record of their Put.io id).
  Files added to the folder by hand are out of scope for a run.
- The read limit is a plain average rate; it does not try to measure disk load.
- A mismatch found by a run uses the existing queue (with overwrite) rather than a separate
  re-download path, so the queue's own retry and the per-download `verify_max_redownloads` apply.
- A run uses one reader at a time; it does not hash several files in parallel.
