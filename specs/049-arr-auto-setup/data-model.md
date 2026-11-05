# Data model: 049

All in memory or on the wire; nothing is stored.

## RemotePathMapping (extended)

`{id?, host, remote_path, local_path}`. `id` added (the app's id; needed to update or remove).

## Proposal

| Field | Type | Notes |
|---|---|---|
| `action` | `none` \| `create` \| `update` \| `remove` \| `not_found` | R4 |
| `host` | string | the host in the app's putio-sync client |
| `remote_path` | string | putio-sync's download folder with a trailing separator |
| `local_path` | string | where the app sees it, with the app's separator (empty for remove/none) |
| `id` | int? | the existing mapping for update/remove |

Equality of two proposals (apply guard): all fields equal.

## FolderAnalysis

| Field | Type | Notes |
|---|---|---|
| `path_check` | PathCheck (018) | the check before any change; status upgraded by R2 |
| `proposal` | Proposal | |
| `found` | string? | the app's path of the folder |
| `looked` | [string] | the folders listed, in order (capped at 60) |

## Recommendation

| Field | Type | Notes |
|---|---|---|
| `name` | `completed_download_handling` \| `import_extra_files` | |
| `on` | bool | for import_extra_files: on *and* `srt` in the extensions |

Settings the app lacks are left out of the list.

## Failure reasons (new)

| Reason | Meaning |
|---|---|
| `no_client` | the app has no putio-sync download client (set it up first) |
| `marker_failed` | putio-sync could not create its marker in the download folder |
| `changed` | apply: the proposal differs from the one confirmed; the new one is returned |

Existing reasons (`unreachable`, `unauthorized`, `not_arr`, `arr_refused`, …) are reused.

## Metric

`putio_sync_arr_path_mapping_total{result}` with `result` ∈ `created`, `updated`, `removed`,
`failed`.
