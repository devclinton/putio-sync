# Contract: `[[hooks]]` configuration

```toml
[[hooks]]
name = "on-download"                 # required, unique, [A-Za-z0-9_-]{1,40}
enabled = true                       # default true
events = ["download.finished"]       # required, non-empty; "*" for every type
command = ["/config/hooks/on-download.py", "--verbose"]   # absolute program + args
env = { LIBRARY = "/data/media" }    # optional, commands only
timeout_secs = 30                    # default 30, 1..=600

[[hooks]]
name = "home-assistant"
events = ["download.finished", "storage.low"]
url = "https://ha.local:8123/api/webhook/putio"           # http or https
headers = { Authorization = "Bearer abc" }                # optional, webhooks only
timeout_secs = 10
```

## Validation (`Config::validate`, error names the hook and field)

| Rule | Message (example) |
|------|-------------------|
| at most 32 hooks | `hooks: at most 32 hooks` |
| name required, pattern, unique | `hooks[2].name: "my hook" may only use letters, digits, - and _` |
| exactly one of `command`, `url` | `hooks "x": set either command or url, not both` |
| `command` non-empty, program absolute | `hooks "x".command: the program must be an absolute path` |
| `url` parses, scheme http/https, has a host | `hooks "x".url: must be an http or https URL` |
| `events` non-empty, each known or `*` | `hooks "x".events: unknown event "download.done"` |
| `hook.test` not listed | `hooks "x".events: hook.test is only sent by the test button` |
| `timeout_secs` 1..=600 | `hooks "x".timeout_secs: must be between 1 and 600` |
| `headers` only with `url`, `env` only with `command` | `hooks "x".headers: only for url hooks` |
| header names valid tokens, at most 32 | `hooks "x".headers: "Bad Name" is not a valid header name` |
| env names `[A-Za-z_][A-Za-z0-9_]*`, not `PUTIO_SYNC_EVENT_*`/`PUTIO_SYNC_HOOK_NAME` | `hooks "x".env: PUTIO_SYNC_EVENT_ID is set by putio-sync` |
| a value of `********` in headers or env (unresolved mask) | `hooks "x".headers.Authorization: still masked; type the value again` |
| a cut-down URL ending in `…` (unresolved mask) | `hooks "x".url: still shortened; type the full URL again` |

## Risks (assistant banner and doctor Config check; warnings)

| id | When |
|----|------|
| `hook_plain_http` | `url` is `http://` and the host is not loopback, a private address, `.local`, or a single label |
| `hook_disabled` | a hook has `enabled = false` |
| `hook_shared_dir` | the program's folder is writable by others (Unix mode `o+w`) |
| `notifications_unused` | `[notifications] enabled = true` (it sends nothing; use a hook) |
