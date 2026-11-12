# Contract: `plugins check` and the manifest's link fields

## `putio-sync plugins check <folder>`

Checks one plugin folder the way **Reload plugins** does, plus a Lua parse. Needs no config file,
token or running daemon.

```text
$ putio-sync plugins check ./webhook-notifier
webhook-notifier 1.0.0: ok

$ putio-sync plugins check ./broken
broken/plugin.toml: docs: must be an https address
broken/main.lua:12: '=' expected near 'end'
2 problems
```

| Exit | Meaning |
|------|---------|
| 0 | The folder loads |
| 1 | One or more problems, each on its own line on stdout |
| 2 | Usage error (no such folder, not a folder) |

`--json` prints `{"name", "version", "problems": [..]}` instead, for CI.

## Manifest fields

```toml
author = "devclinton"                       # ≤ 100 characters
license = "MIT"                             # SPDX id or expression, ≤ 60
homepage = "https://example.com"            # links: https (or http to a local address), ≤ 300
docs = "https://example.com/docs"
repository = "https://github.com/you/plugin"
issues = "https://github.com/you/plugin/issues"
```

All optional. Errors use the existing form: `<file>: <field>: <message>`, for example
`plugins/x/plugin.toml: repository: must be an https address`.
