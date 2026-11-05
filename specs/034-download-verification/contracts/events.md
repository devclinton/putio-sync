# Event Contract: Download Verification

New types on the 027 bus; `schema_version` stays 1 (new types are additive, EVENTS.md
› Versioning). Every one carries `trigger`: `download` (a new download) or `run` (a verify
run). A run never sends `verify.passed`.

## `verify.passed`

| Field | Type | |
|-------|------|-|
| `file_id` | integer | |
| `name` | string | as saved |
| `relative_path` | string | |
| `local_path` | string | |
| `size_bytes` | integer | |
| `method` | string | `crc32` or `size` |
| `crc32` | string or null | Put.io's, lower case |
| `redownloads` | integer | re-downloads it took |
| `trigger` | string | always `download` |

## `verify.failed`

| Field | Type | |
|-------|------|-|
| `file_id`, `name`, `relative_path`, `local_path`, `size_bytes` | | as above |
| `expected_crc32` | string or null | null for a size mismatch |
| `actual_crc32` | string or null | |
| `attempt` | integer | 1 for the first download |
| `will_redownload` | boolean | |
| `trigger` | string | |

## `verify.redownload`

| Field | Type | |
|-------|------|-|
| `file_id`, `name`, `relative_path`, `size_bytes` | | |
| `attempt` | integer | the download about to start (2 = first re-download); for a run, 1 |
| `max_redownloads` | integer | |
| `trigger` | string | |
