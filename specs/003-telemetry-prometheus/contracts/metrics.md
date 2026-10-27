# Contract: Metrics Endpoint

## Endpoint

- `GET http://<bind><path>`. The default is `http://0.0.0.0:9742/metrics`.
- The response is `200` with `Content-Type: text/plain; version=0.0.4; charset=utf-8`, in
  the Prometheus text exposition format.
- Any other path gets `404`, and any other method gets `405`.
- There is no auth (clarification Q2). The endpoint never returns Put.io data, tokens,
  file names, ids, paths or URLs.
- It answers in under 1 s while downloads run (SC-002).
- The same listener and output are used in `daemon` and `server` modes (clarification Q5).

## Families

H = histogram. API buckets (s): 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30. Download
buckets (s): 1, 5, 15, 60, 300, 900, 1800, 3600, 7200. Sync buckets (s): 1, 5, 15, 30,
60, 120, 300, 600.

### Put.io API

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_api_requests_total` | counter | `operation`, `outcome` | Each HTTP attempt to Put.io, retries included |
| `putio_sync_api_retries_total` | counter | `reason` | Each retry scheduled by the retry policy |
| `putio_sync_api_request_duration_seconds` | H | — | Latency of each attempt |
| `putio_sync_api_operation_duration_seconds` | H | `operation` | **detailed only.** Latency per operation |

### Sync runs

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_sync_runs_total` | counter | `result` | Completed sync runs |
| `putio_sync_sync_duration_seconds` | H | — | Duration of each sync run |
| `putio_sync_last_sync_duration_seconds` | gauge | — | Duration of the latest run |
| `putio_sync_last_success_timestamp_seconds` | gauge | — | Unix time the last successful run finished; absent until the first one |
| `putio_sync_last_sync_files` | gauge | `kind` | Counts from the latest run |
| `putio_sync_files_blocked_total` | counter | — | Files blocked or skipped by a folder rule or the blocked-extension list (warned once per file id) |
| `putio_sync_rule_decisions_total` | counter | `decision` (blocked, skipped, deleted) | Folder rule decisions (feature 009) |

### Queue and downloads

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_queue_items` | gauge | `state` | Queue rows by state, refreshed every 15 s and at startup |
| `putio_sync_downloads_active` | gauge | — | Downloads in progress |
| `putio_sync_download_workers` | gauge | — | Configured worker count |
| `putio_sync_downloads_completed_total` | counter | — | Downloads verified and renamed into place |
| `putio_sync_downloads_failed_total` | counter | `reason` | Downloads that ended in failure |
| `putio_sync_downloaded_bytes_total` | counter | — | Bytes written to disk; `rate()` gives throughput |
| `putio_sync_download_duration_seconds` | H | — | Time per completed download |
| `putio_sync_transfers_stalled` | gauge | — | Put.io transfers stalled at the last check (feature 007) |
| `putio_sync_worker_state` | gauge | `worker` | **detailed only.** 0 idle, 1 downloading |
| `putio_sync_worker_inflight_bytes` | gauge | `worker` | **detailed only.** Bytes written so far for the current file |

### Watch folder

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_watch_scans_total` | counter | — | Watch-folder scans |
| `putio_sync_watch_uploads_total` | counter | — | Torrent/magnet files uploaded to Put.io |
| `putio_sync_watch_cleanups_total` | counter | — | Already-uploaded files cleaned up |
| `putio_sync_watch_invalid_total` | counter | — | Invalid files (warned once) |
| `putio_sync_watch_cleanup_failures_total` | counter | — | Cleanup (delete/archive) failures |

### Database, disk, process

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_db_errors_total` | counter | `operation` | Database errors |
| `putio_sync_disk_free_bytes` | gauge | — | Free space for the download folder; absent if unreadable |
| `putio_sync_disk_total_bytes` | gauge | — | Size of the download folder's filesystem; absent if unreadable |
| `putio_sync_start_time_seconds` | gauge | — | Process start, Unix seconds |
| `putio_sync_build_info` | gauge (=1) | `version`, `commit` | Build identity |

Label values are listed in [data-model.md](../data-model.md#closed-label-sets).

## Invariants (tested)

1. Every family above is present after one scrape at `basic`. Exceptions: the two
   detailed-only families, and `last_success_timestamp`/disk gauges in their documented
   "absent" cases.
2. At `basic`, no series carries a `worker` label, and no `operation` label appears on
   duration histograms.
3. Every label value in the full output belongs to its closed set (SC-006).
4. The configured token appears nowhere in the output. No label value contains a URL (`://`),
   `/`, or any file name used in the test fixtures (SC-006).
5. Counters never decrease within one process.

## Questions → queries (SC-001)

| Question | Query |
|---|---|
| Syncs failed last week | `increase(putio_sync_sync_runs_total{result="failure"}[7d])` |
| Retries and why | `sum by (reason) (increase(putio_sync_api_retries_total[1d]))` |
| Pending and moving? | `putio_sync_queue_items{state="pending"}` and `increase(putio_sync_downloads_completed_total[1h])` |
| Executables blocked | `increase(putio_sync_files_blocked_total[7d])` |
| Watch cleanups | `increase(putio_sync_watch_cleanups_total[7d])` |
| DB errors | `increase(putio_sync_db_errors_total[1d])` |
