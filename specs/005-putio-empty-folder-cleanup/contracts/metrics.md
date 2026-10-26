# Metrics Contract: Empty-Folder Cleanup

| Name | Type | Labels | Help |
|------|------|--------|------|
| `putio_sync_empty_folder_runs_total` | counter | `trigger` (`schedule`, `manual`), `result` (`success`, `failure`) | Finished empty-folder cleanup runs (dry runs not counted). |
| `putio_sync_empty_folders_deleted_total` | counter | — | Empty folders removed from Put.io, nested ones included. |
| `putio_sync_empty_folder_delete_failures_total` | counter | — | Empty-folder deletes Put.io refused or that failed. |
| `putio_sync_empty_folder_last_success_timestamp_seconds` | gauge | — | Unix time of the last successful run; absent until the first. |

All four are basic (not detail-gated). Every `trigger × result` series is zero-initialised.
Labels are closed enums: no folder names, ids or paths.
