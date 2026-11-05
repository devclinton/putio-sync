# Data Model: Log Viewer (047)

All in memory, in `putio-sync-core`.

## LogLine (`log_capture`)

| Field | Type | Notes |
|-------|------|-------|
| `seq` | u64 | Starts at 1, strictly increasing, process-wide |
| `time` | String | RFC 3339 UTC, milliseconds |
| `level` | String | `ERROR` `WARN` `INFO` `DEBUG` `TRACE` |
| `target` | String | tracing target (module path) |
| `message` | String | Redacted |
| `fields` | Vec<(String, String)> | In logging order, values redacted; `message` is not repeated |
| `truncated` | bool | Message and fields cut to 4 KB in total |

JSON: `{"seq":1,"time":"…","level":"INFO","target":"…","message":"…","fields":[["file_id","42"]],"truncated":false}`

## LogBuffer (`log_capture`, global)

- `capacity`: 100 to 50,000 (`[logging] buffer_lines`, default 2,000). `set_capacity`
  clamps and drops the oldest lines if it shrinks.
- `lines`: `VecDeque<Arc<LogLine>>`, oldest first.
- `next_seq`.
- `evicted`: the total pushed out (also counted on the metric).
- `tx`: `broadcast::Sender<Arc<LogLine>>`.

Query (`LogQuery`): `after: Option<u64>`, `level: Option<Level>` (at or more severe),
`target: Option<String>` (equal or `target::` prefix), `q: Option<String>` (case-insensitive
substring of message, target, field names or values), `limit` (default and max: capacity).
It returns the newest `limit` matches, oldest first.

## LevelState (`logging`, global)

| Field | Type | Notes |
|-------|------|-------|
| `base` | String | Directive from config or `RUST_LOG` |
| `source` | LevelSource | `Config` or `Env` |
| `global` | Option<Override> | Overall level from the page |
| `modules` | BTreeMap<String, Override> | ≤ 20 |

`Override { level: String, until: Option<SystemTime> }`. `until = None` means keep.

Transitions:
- `set(target?, level, minutes?)`
- `clear(target?)`: one module, or with no target, everything
- `expire(now)`: drops ended overrides
- `set_base(config)`: the poller

Each one recomputes the effective directive, reloads the filter if it changed, and logs one
info line `Log level changed` with `old`, `new` and `reason` (`page`, `expired`, `reverted`,
`config`).

`LevelStatus` (API and doctor): `base`, `source`, `effective`, `global`,
`modules: [{target, level, until}]` and `verbose: bool` (any directive at debug or trace).
`until` is RFC 3339 or null.
