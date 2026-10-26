# Data Model: Telemetry and Prometheus Monitoring

This feature adds no persisted data. Everything below lives in memory (metrics) or in the
existing TOML config file (settings). The full metric list is in
[contracts/metrics.md](contracts/metrics.md).

## MetricsSettings (new, `[metrics]` in config)

| Field | Type | Default | Env override | Validation |
|---|---|---|---|---|
| `enabled` | bool | `true` | `PUTIO_SYNC_METRICS_ENABLED` | `true`/`false`/`1`/`0` |
| `bind` | socket address | `0.0.0.0:9742` | `PUTIO_SYNC_METRICS_BIND` | parses as `SocketAddr` |
| `path` | string | `/metrics` | `PUTIO_SYNC_METRICS_PATH` | starts with `/`, no whitespace |
| `detail` | enum `basic` \| `detailed` | `basic` | `PUTIO_SYNC_METRICS_DETAIL` | one of the two |

- It is resolved once at startup into `Resolved<T> { value, source: Config | Env }` per
  field.
- It isn't live-reloaded. A change needs a restart (FR-010).

## LoggingConfig (existing, `[logging]`), changes

| Field | Change |
|---|---|
| `level`, `module_filters` | Now live-reloaded (R8). `RUST_LOG` still overrides; when it is set, config changes are ignored and that is logged. |
| `console_format` | Gains the `PUTIO_SYNC_LOG_FORMAT` env override (`compact`/`pretty`/`json`). Not live-reloaded. |

### LevelChange (result of `apply_level`)

`Unchanged` → nothing is logged. `Changed { old, new }` → INFO `old`, `new`, `source=Config`.
`IgnoredEnv` → INFO once per change: the config change is ignored because `RUST_LOG` is set.
`Invalid(msg)` → ERROR once per change. The current filter is kept.

## Config reload poller state

| Field | Purpose |
|---|---|
| `path` | config file being watched |
| `last_seen: (mtime, len)` | change detection; set at startup |
| `interval` | 10 s |

Transitions: `Idle → (mtime or len differs) → Reparse → apply_level → Idle`. If the file
is missing or unreadable, it logs ERROR once and stays Idle until the stamp changes again.

## Metric family (contract entity)

| Attribute | Rule |
|---|---|
| name | `putio_sync_` prefix; counters end in `_total`; units `_seconds` / `_bytes` |
| type | counter, gauge, or histogram |
| help | one sentence, in the contract |
| labels | names fixed per family; every value from a closed enum below |
| detail | `basic` (always) or `detailed` (only when `detail = detailed`) |

### Closed label sets

| Label | Values |
|---|---|
| `operation` (API) | `list_files`, `get_file`, `delete_files`, `upload`, `add_transfer`, `list_transfers`, `get_transfer`, `cancel_transfers`, `retry_transfer`, `account_info`, `other` (mapped from the request path, see tasks) |
| `outcome` (API) | `success`, `client_error`, `server_error`, `network_error` |
| `reason` (retry) | `network`, `server_error`, `rate_limited` |
| `result` (sync) | `success`, `failure` |
| `kind` (sync files) | `found`, `queued`, `skipped`, `blocked`, `failed` |
| `state` (queue) | `pending`, `downloading`, `paused`, `failed` |
| `reason` (download failure) | `network`, `http`, `io`, `integrity`, `other` |
| `operation` (DB) | `queue` (queue.rs), `history` (history.rs, downloads.rs), `uploads` (torrents.rs), `migrate` (lib.rs, schema.rs), `other` (stats.rs) |
| `worker` (detailed) | `"0"` … `"workers-1"`, fixed at startup |
| `state` (worker, detailed) | gauge value: 0 = idle, 1 = downloading |
| `version`, `commit` (build info) | one value each per process |

Any Put.io request path that isn't mapped uses `other`, never a free-form string.
