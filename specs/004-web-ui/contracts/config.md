# Contract: `[api]` Settings

## TOML

```toml
[api]
listen = true               # daemon: open the API + web UI listener; restart to change
bind = "0.0.0.0:15432"      # all interfaces; exposure = which port the container publishes
auth_token = ""             # REQUIRED for the UI and every API call except /health;
                            # leave unset here and use PUTIO_SYNC_API_TOKEN if you prefer
# enabled = false           # existing, unrelated: daemon delegates downloads to a remote API
# url = "http://host:15432" # existing, used with enabled = true
```

- `listen` and `bind` are read by the `daemon` command only. The `server` command keeps
  its `--bind`/`--port` flags (defaults `0.0.0.0`/`15432`), which win over `[api]`.
- `auth_token` is read by both commands.
- `enabled` and `url` keep their meaning and are not touched by this feature.

## Environment (wins over TOML)

| Variable | Setting | Allowed |
|---|---|---|
| `PUTIO_SYNC_API_LISTEN` | `api.listen` | `true`, `false`, `1`, `0` |
| `PUTIO_SYNC_API_BIND` | `api.bind` | `host:port` (parses as a socket address) |
| `PUTIO_SYNC_API_TOKEN` | `api.auth_token` | non-empty, no whitespace |

`Config::resolve_api()` returns `Resolved<ApiSettings>` with `source` = `Env` when any of
the three variables is set, else `Config`, mirroring `resolve_metrics()`.

## Startup log lines (INFO unless noted)

```text
API listener enabled bind=0.0.0.0:15432 token=set source=Config
API listener disabled source=Env
API token not configured (set [api] auth_token or PUTIO_SYNC_API_TOKEN); all API and UI requests will be refused  (WARN)
API listener failed to start bind=0.0.0.0:15432 error=...             (ERROR, once)
```

`token=set` or `token=unset`; the value is never logged. The WARN line is emitted by
both `daemon` (when the listener is on) and `server`.

## Errors

| Case | Behavior |
|---|---|
| Invalid value at startup, in TOML or env | exit non-zero: `invalid <setting> '<value>': expected <allowed>` (same wording as `[metrics]`) |
| `auth_token` set but empty or containing whitespace | exit non-zero: `invalid api.auth_token: expected a non-empty value without whitespace` |
| Port in use / bind fails | one ERROR `API listener failed to start`; the daemon keeps syncing, watching and downloading (FR-004) |
| Token unset | one WARN at startup; every protected request answers 401 with the "not configured" body |

## Docker

`Dockerfile` gains `EXPOSE 15432` next to the existing metrics port. Publishing is the
operator's step (`-p 15432:15432` or the Unraid template port mapping).
