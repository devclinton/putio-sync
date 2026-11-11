# Contract: manifest and settings additions

## `plugin.toml`

```toml
hooks = ["filter"]     # any of "filter", "rename", "folder"; optional
events = []            # may be empty (or left out) when hooks is not empty
```

Errors (prefixed with the file name):

- `hooks: unknown sync hook "foo" (filter, rename or folder)`
- `hooks: "filter" listed twice`
- `events: list at least one event, or "*" for all` (only when `hooks` is empty)
- at load: `main.lua: the returned table has no filter function` (for a declared hook),
  `main.lua: the returned table has no on_event function` (only when `events` is not empty)

## `[[plugins]]`

```toml
hook_timeout_ms = 500  # 50 to 5000; each sync rule call
```

Error: `plugins "x".hook_timeout_ms: must be between 50 and 5000`.

## `GET /plugins`

Each plugin gains `hooks: ["filter", ...]`. `GET/PUT /settings/plugins` carry
`hook_timeout_ms`.
