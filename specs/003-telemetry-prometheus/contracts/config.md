# Contract: Telemetry Settings

## TOML

```toml
[logging]
level = "info"              # live-reloaded (≤30 s); RUST_LOG overrides
module_filters = []         # live-reloaded with level
console_format = "compact"  # compact | pretty | json; restart to change

[metrics]
enabled = true              # restart to change
bind = "0.0.0.0:9742"       # all interfaces; exposure = which port the container publishes
path = "/metrics"
detail = "basic"            # basic | detailed
```

## Environment (wins over TOML)

| Variable | Setting | Allowed |
|---|---|---|
| `RUST_LOG` | logging level + directives | any `EnvFilter` directive |
| `PUTIO_SYNC_LOG_FORMAT` | `logging.console_format` | `compact`, `pretty`, `json` |
| `PUTIO_SYNC_METRICS_ENABLED` | `metrics.enabled` | `true`, `false`, `1`, `0` |
| `PUTIO_SYNC_METRICS_BIND` | `metrics.bind` | `host:port` |
| `PUTIO_SYNC_METRICS_PATH` | `metrics.path` | starts with `/` |
| `PUTIO_SYNC_METRICS_DETAIL` | `metrics.detail` | `basic`, `detailed` |

## Startup log lines (INFO)

```text
Log level level=info source=Config format=compact format_source=Config
Metrics enabled bind=0.0.0.0:9742 path=/metrics detail=basic source=Config
```

When metrics are disabled, the second line is `Metrics disabled source=Env` (or
`source=Config`).

## Errors

| Case | Behavior |
|---|---|
| Invalid value at startup, in TOML or env | exit non-zero: `invalid <setting> '<value>': expected one of <allowed>` |
| Metrics port in use / bind fails | one ERROR `Metrics listener failed to start` with `bind` and `error`; daemon keeps running |
| Config edited to an invalid level while running | one ERROR `Log level not changed` with `error`; current level kept |
| Config edited while `RUST_LOG` set | one INFO `Log level change in config ignored; RUST_LOG is set` |
| Valid level change while running | one INFO `Log level changed` with `old`, `new`, `source=Config` |
