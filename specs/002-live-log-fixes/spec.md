# Feature Specification: Fixes from the Live NAS Logs

**Feature Branch**: `002-live-log-fixes`

**Created**: 2026-10-25

**Status**: Draft

**Input**: User description: "Fix the problems found in the live NAS daemon logs (2026-09-06 to 2026-10-25): (1) first Put.io call of every sync fails and is retried, sometimes losing a sync; (2) 'database is locked' when workers claim queue items at once; (3) watch-folder files uploaded before are skipped forever and never deleted; (4) per-minute idle watcher log noise; (5) misleading or ignored settings (--download-interval in daemon mode, startup banner, RUST_LOG override); (6) skip executable files (fake .exe releases) by default, configurable."

## Evidence

From 338,575 log lines on the NAS (2026-09-06 → 2026-10-25, one daemon instance, 3 workers):

| # | Symptom | Count |
|---|---------|-------|
| 1 | First file-list request of a sync fails ("error sending request"), retried after 100 ms | 12,513 (≈250/day, every hour); 2 syncs lost after all attempts failed |
| 2 | Worker fails to claim the next queue item: "database is locked" | 145 |
| 3 | Watch-folder files skipped every scan and never removed | 41 files; oldest from February 2026 |
| 4 | Idle watcher scan lines at INFO ("processed: 0") | ≈210,000 |
| 5 | "--download-interval not yet supported in daemon mode, ignoring"; banner shows `Watch paths: []`, `Delete after: false` while the config enables both | Every startup (8) |
| 6 | Queued downloads that were executables named as TV episodes | 133 of 244 |

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Syncs don't fail on the first request (Priority: P1)

As the NAS operator, I want each scheduled sync to reach Put.io on the first try, so that logs are free of routine network warnings and no sync is lost.

**Why this priority**: It happens on almost every sync, it hides real network problems in noise, and it has already dropped syncs.

**Independent Test**: Against a mock Put.io server that closes idle connections, run two syncs separated by an idle gap longer than the server's idle timeout. The second sync succeeds without any retry warning. Separately, against a server that fails twice and then succeeds, a sync still completes.

**Acceptance Scenarios**:

1. **Given** the daemon has been idle between syncs, **When** the next sync starts, **Then** its first Put.io request succeeds without a retry.
2. **Given** Put.io fails transiently (connection reset or timeout) for less than the retry budget, **When** a sync runs, **Then** the sync completes, and each retry is logged once with the underlying cause (for example "connection closed before message completed" or "timed out").
3. **Given** Put.io stays unreachable past the retry budget, **When** a sync runs, **Then** the sync is reported as failed once, with the underlying cause, and the next scheduled sync runs normally.

---

### User Story 2 - Workers never fail on a busy queue (Priority: P1)

As the operator, I want every worker to claim queued items reliably when several start at once, so that downloads aren't delayed by spurious errors.

**Why this priority**: It's a correctness bug in the core download path, and it fires right when a sync queues several files, which is the busiest moment.

**Independent Test**: With a file-backed queue holding N pending items, start several workers at the same instant. Every item is claimed exactly once, and no worker reports a lock error.

**Acceptance Scenarios**:

1. **Given** 3 workers and 5 newly queued items, **When** all workers claim at the same time, **Then** each item is claimed by exactly one worker and no "database is locked" error occurs.
2. **Given** another writer holds the store briefly (for example during a sync insert), **When** a worker claims, **Then** it waits and succeeds instead of erroring.

---

### User Story 3 - Already-uploaded watch files get cleaned up (Priority: P1)

As the operator, I want a watch-folder file that was already sent to Put.io to get the configured post-upload action (delete or archive), even if that action didn't happen the first time, so that blackhole folders don't fill up and Radarr/Sonarr see their grabs completed.

**Why this priority**: 41 files are stuck now, and the folder only grows.

**Independent Test**: Seed the processed-records store with the fingerprint of a magnet file in a temporary watch folder, set the post-upload action to delete, and run one scan. The file is removed, and nothing is uploaded again.

**Acceptance Scenarios**:

1. **Given** a watch file whose contents were already uploaded, and post-upload delete is on, **When** a scan runs, **Then** the file is deleted and not uploaded again.
2. **Given** the same, but with post-upload archive configured, **When** a scan runs, **Then** the file is moved to the archive folder.
3. **Given** post-upload delete and archive are both off, **When** a scan runs, **Then** the file is left in place and not uploaded again (today's behavior).
4. **Given** the cleanup action fails (for example, permission denied), **When** a scan runs, **Then** the failure is logged at WARN once per file per daemon run, not every minute.

---

### User Story 4 - Executables are never downloaded (Priority: P1)

As the operator, I want files with executable extensions skipped by default, so that fake releases (malware posing as episodes) never land in my media folders.

**Why this priority**: It's a security issue: over half of recent downloads were executables.

**Independent Test**: A mock Put.io listing contains `Show S01E01.mkv`, `Show S01E02.exe` and `setup.MSI`. After a sync, only the `.mkv` is queued, and the other two are logged as blocked.

**Acceptance Scenarios**:

1. **Given** the default configuration, **When** a sync finds a file with a blocked extension (case-insensitive), **Then** it is not queued or downloaded, and a WARN names the file and the reason.
2. **Given** a blocked file stays on Put.io, **When** later syncs run, **Then** the warning is not repeated on every sync (at most once per file per daemon run).
3. **Given** the operator sets their own blocked-extension list in the config, **When** a sync runs, **Then** that list replaces the default, and an empty list turns the guard off.
4. **Given** a blocked file sits inside a folder alongside normal files, **When** the folder syncs, **Then** only the blocked file is skipped.
5. **Given** a blocked file, **Then** it is never deleted from Put.io by the post-download delete (it was never downloaded or verified).

---

### User Story 5 - Logs and settings tell the truth (Priority: P2)

As the operator, I want logs to be quiet when nothing happens, and the startup banner and log level to reflect the config actually in effect, so that I can read the logs when something goes wrong.

**Why this priority**: It's quality of life and diagnosability, not a data-path fix.

**Independent Test**: Start the daemon with a config enabling watch paths and delete-after-download, plus `--download-interval 30`. The banner shows the effective values, and syncs run every 30 s. Run idle scans for several minutes, and none are logged at INFO.

**Acceptance Scenarios**:

1. **Given** a watcher scan that finds nothing new to process, **When** it completes, **Then** it logs at DEBUG. **Given** a scan that processed, deleted or failed something, **Then** it logs at INFO.
2. **Given** `--download-interval N` on the daemon command, **When** the daemon runs, **Then** syncs happen every N seconds, and no "not supported" warning appears.
3. **Given** a config file with watch paths and delete-after enabled, **When** the daemon starts, **Then** the banner prints the merged, effective settings after the config loads, never placeholder CLI defaults.
4. **Given** both `RUST_LOG` and a config log level are set, **When** the daemon starts, **Then** it logs once which source won and the effective level.

### Edge Cases

- A sync starts while a previous sync is still running. Behavior is unchanged: no overlapping syncs.
- A watch file is deleted by someone else between the scan and the cleanup. Treat it as already done, with no error.
- An empty or unreadable magnet file is reported once as invalid, not silently "skipped" every scan.
- Extensions are matched case-insensitively on the final extension only. `Show.exe.mkv` is not blocked, and `Show.mkv.exe` is.
- `--download-interval` and the config interval are both set. The CLI flag wins, consistent with other flags.
- The process-wide retry budget isn't consumed by idle-connection failures (US1), so a real outage still gets all its retries.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The daemon MUST NOT reuse a connection to Put.io that has likely been closed by the server or the network after an idle period, so that the first request of a sync after an idle interval succeeds without a retry.
- **FR-002**: Transient Put.io failures (connection errors, timeouts, HTTP 5xx and 429) MUST be retried with increasing delays across a total window of at least 30 seconds before the operation is reported as failed.
- **FR-003**: Every network error log line MUST include the underlying cause chain, not just "error sending request", and MUST NOT include tokens.
- **FR-004**: Concurrent workers claiming queue items MUST wait for brief store contention rather than fail. Each pending item MUST be claimed by at most one worker.
- **FR-005**: A watch-folder file whose contents were already uploaded MUST receive the configured post-upload action (delete or archive) on the next scan, and MUST NOT be uploaded again.
- **FR-006**: Post-upload action failures MUST be logged at WARN at most once per file per daemon run.
- **FR-007**: Watcher scans with no processed, cleaned-up or failed files MUST log their summary at DEBUG. Other scans log at INFO.
- **FR-008**: Download syncs MUST skip files whose final extension is on a blocked-extension list. The default list is `exe, msi, bat, cmd, com, scr, pif, vbs, js, jar, ps1, lnk, apk, dmg, pkg`. Matching is case-insensitive.
- **FR-009**: The blocked-extension list MUST be configurable in the TOML config. A configured list replaces the default, and an empty list disables the guard. The setting MUST be documented in `config.example.toml`.
- **FR-010**: A blocked file MUST NOT be queued, downloaded, counted as completed, or deleted from Put.io. It MUST be logged at WARN once per file per daemon run, and counted as "blocked" in the sync summary.
- **FR-011**: The daemon MUST honor `--download-interval`, overriding the configured interval.
- **FR-012**: The startup banner MUST show the effective settings after the config file, environment and CLI flags are merged.
- **FR-013**: At startup the daemon MUST log the effective log level and whether it came from `RUST_LOG` or the config file.
- **FR-014**: Each fix MUST come with an automated test that fails before the fix and passes after, without network access to Put.io (Constitution III).

### Key Entities

- **Processed watch record**: the fingerprint of a watch file's contents that was uploaded. It is used to avoid re-uploading, and now also triggers cleanup of leftovers.
- **Blocked-extension list**: the operator-configurable set of file extensions that are never downloaded.
- **Queue item**: a pending or active download. Claiming moves it from pending to active for exactly one worker.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After one week on the NAS, routine first-request network warnings drop from about 250 a day to under 5 a day.
- **SC-002**: Zero "database is locked" errors over one week of normal operation (baseline: about 20 a week).
- **SC-003**: Within one scan after deployment, the 41 stuck watch files are cleaned up per the configured action, and the watch folders then hold only files not yet uploaded.
- **SC-004**: Daemon log volume drops by at least 60% for the same workload (baseline: about 7,000 lines a day).
- **SC-005**: Zero executable files are downloaded after deployment with the default config.
- **SC-006**: The startup banner matches the config file for every setting shown, verified on the NAS.
- **SC-007**: `make ci` passes, with a new reproducing test for each of FR-001 to FR-013.

## Assumptions

- **Blocked files stay on Put.io.** The daemon only skips and warns. Removing them is left to the operator, so the tool never deletes data it didn't download (Constitution IV).
- **Root cause of the network warnings.** The first-request failures are assumed to be stale pooled connections. The fix is verified with a mock server that drops idle connections. If the improved error logging (FR-003) shows a different cause on the NAS, that is follow-up work.
- **Watch folders keep their current layout.** The 41 stuck files are assumed to be uploaded already, based on their fingerprint records. If any isn't in the store, it is uploaded normally, which is acceptable.
- **Deployment is out of scope.** Rebuilding and redeploying the NAS image is not part of this feature. That includes the Docker Hub vs GHCR question raised during 001. The success criteria measured on the NAS (SC-001–SC-006) are checked after the operator deploys.
- **Out of scope.** Telemetry and metrics (feature 003) and the web UI (feature 004) are not part of this feature.
- **The daemon's weekly restarts are expected.** They come from the Unraid appdata backup at Monday 00:00 Pacific, and are not a bug.
