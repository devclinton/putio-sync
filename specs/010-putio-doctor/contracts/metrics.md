# Metrics: Doctor

The daemon's scheduled runs and the API's on-demand runs set these on the global telemetry.
CLI runs do not, because the CLI process serves no metrics.

| Name | Type | Labels | Meaning |
|------|------|--------|---------|
| `putio_sync_doctor_check_status` | gauge | `check` | 0 ok, 1 warning, 2 problem, -1 skipped |
| `putio_sync_doctor_check_items` | gauge | `check` | number of affected items |
| `putio_sync_doctor_last_run_timestamp_seconds` | gauge | none | Unix time the last run finished |
| `putio_sync_doctor_last_run_duration_seconds` | gauge | none | duration of the last run |

`check` takes one of the CheckId values in [data-model.md](../data-model.md), at most 17
series. The series appear after the first run.

Example alert, configured in the home-network stack and not in this repo:
`max(putio_sync_doctor_check_status) >= 2 for 2h`.
