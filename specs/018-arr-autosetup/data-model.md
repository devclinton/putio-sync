# Data Model: Radarr/Sonarr Settings and Auto-Setup

No database changes. Settings stay in the config file. Everything else is in memory.

## Config file (`[arr]`, owned by `putio-sync-core`)

```toml
[arr]
enabled = true                 # was: file-only, needed a restart
username = "putio-sync"
password = "…"                 # PUTIO_SYNC_ARR_PASSWORD wins when set

[[arr.servers]]
kind = "radarr"                # radarr | sonarr
name = "Radarr"                # NEW, optional; default "Radarr"/"Sonarr" (+ " (2)" …)
url = "http://radarr:7878"     # host, port, SSL and URL base joined
api_key = "…"                  # PUTIO_SYNC_RADARR_API_KEY / _SONARR_ wins for the first of its kind
# category = "radarr"          # LEGACY, optional, ignored (warning when it differs, FR-015)
```

| Type | Change |
|---|---|
| `ArrConfig` | unchanged fields; written by the new `Config::save_arr_section` (research R1) |
| `ArrServer` | `+ name: Option<String>`; `category: String` → `Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` |
| `ArrConfig::validate` | drops the category rules; requires an http(s) URL and unique normalized URLs; `enabled = true` requires a password (from the file or env), FR-006 |
| `ArrSettings` | unchanged shape plus `name`; it's now swapped at runtime (R2) |

**Env-set fields**: `resolve_arr` reports which fields come from env (`password_from_env`, and
`api_key_from_env` per server), so the UI can make them read-only (FR-007). Env values are never
written to the file.

## Runtime state (`putio-sync-arr`)

### ArrService (changed)

| Field | Type | Notes |
|---|---|---|
| `settings` | `RwLock<Arc<ArrSettings>>` | was a plain field; `apply()` swaps it (R2) |
| `clients` | `RwLock<HashMap<String, ClientInfo>>` | keyed by normalized server URL (R4) |
| `discovery` | `tokio::sync::Mutex<()>` | one discovery at a time (R9) |

### ClientInfo (new): what the *arr has for putio-sync

| Field | Type | Notes |
|---|---|---|
| `categories` | `Vec<String>` | from the matching clients' `movieCategory` / `tvCategory` |
| `client_present` | `bool` | at least one client points at putio-sync (R6) |
| `read_at` | `Option<SystemTime>` | last successful read |
| `last_error` | `Option<ArrApiErrorKind>` | the last failed read; the value is kept |
| `path_check` | `Option<PathCheck>` | last result of R10 |

### DiscoveredInstance (new, response only)

`kind`, `url`, `found_by` (`saved` \| `container` \| `host` \| `browser`), and `version`
(`null` until a key is known).

### SetupRequest (new, request only)

| Field | Notes |
|---|---|
| `target` | `{ url }` for a new address, or `{ application: index }` for a saved one (that key is used) |
| `api_key` | required for `url`; secret; never logged; saved only when `remember` is set (FR-023b) |
| `category` | default `radarr`/`sonarr`; checked with `normalize_arr_category` |
| `host` | optional override for the host the *arr uses (FR-022) |
| `disable_blackhole` | bool, default false (FR-021) |
| `remember` | bool, default false (FR-023b) |

### SetupPlan (new, preview response)

The *arr's `kind`, `name` and `version`, plus:

- `action`: `create` \| `update` \| `unchanged`;
- `client_name`;
- `category`;
- `existing_clients[]`: name, implementation, enabled, `points_at_putio_sync`;
- `blackhole_enabled`: bool;
- `download_client`: `{ will_enable, will_generate_password }`.

### SetupResult (new)

| Field | Notes |
|---|---|
| `outcome` | `created` \| `updated` \| `unchanged` \| `failed` |
| `reason` | on failure, one of the FR-026 reasons (below) |
| `detail` | the *arr's message or the address tried; secrets are stripped |
| `client_name`, `host` | the name used and the host the *arr accepted |
| `hosts_tried` | for `arr_cannot_reach` |
| `blackhole_disabled` | bool |
| `remembered` | bool |
| `path_check` | `PathCheck` |

**Failure reasons** (a fixed set, also the metric label from R13):

`unreachable`, `timed_out`, `untrusted_certificate`, `unauthorized`, `not_arr`,
`unsupported_version`, `invalid_category`, `arr_cannot_reach`, `arr_test_failed`,
`arr_refused`, `config_write_failed`.

These extend `ArrApiError::kind()` (it has `unreachable`, `unauthorized` and `not_arr` today).
`reqwest` errors are split as follows: `is_timeout()` → `timed_out`; a certificate error in the
source chain → `untrusted_certificate`.

### PathCheck (new)

`status` (`visible` \| `not_visible` \| `unknown`), `folder` (putio-sync's path), `arr_path`
(after the mappings), `mappings[]` (`host`, `remote_path`, `local_path`), and `detail`.

## State transitions

**Download client**

```text
off ──Save(on, password)──▶ on            (RPC accepts at once)
off ──Setup──▶ on (+ generated password when none in file/env)
on  ──Save(off)──▶ off                    (RPC → 404; tracked items kept)
```

**Setup**

```text
check address+key ─▶ plan (preview) ─▶ [ensure download client on] ─▶ test candidate hosts
   │ fail                                                        │ none pass → failed(arr_cannot_reach)
   ▼                                                             ▼ pass
 failed(reason)                     unchanged? ──yes──▶ unchanged
                                       │ no
                                       ▼
                                create / update ──fail──▶ undo (delete / restore) ─▶ failed(arr_refused)
                                       │ ok
                                       ▼
                     [disable blackhole] ─▶ [remember → save application] ─▶ refresh ClientInfo
                                       ─▶ path check ─▶ created / updated
```

If "Remember" fails to write (`config_write_failed`), the *arr change stays done. The result
reports success in the *arr, with a warning that the application wasn't saved.
