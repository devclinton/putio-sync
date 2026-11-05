# Contract: host API additions (API version 1, additive)

```lua
putio.download_dir                 -- string: the daemon's download folder ("" if unknown)

putio.path.map(path, rules)        -- rules: list of {from=, to=} (a path_map config value)
  --> mapped string; `path` unchanged when no rule matches.
  --  Longest `from` that equals `path` or is followed by "/" wins; trailing separators
  --  ignored; a Windows-style `to` ("C:\", "C:/", "\\host") joins with "\".
putio.path.parent(path)            -- "/a/b/c.mkv" -> "/a/b"; "/" -> "/"; "C:\a\b" -> "C:\a"
putio.path.within(path, folder)    -- true when path == folder or is below it on a separator
                                   --  boundary; case-insensitive when either is Windows-style

putio.metrics.count(name, n?)      -- name [a-z0-9_]{1,40}; n integer >= 0, default 1
  --> adds to putio_sync_plugin_counter_total{plugin=<name of plugin>, name=<name>}
  --  a 17th distinct name raises an error
```

Script entry points:

```lua
function plugin.on_action(id, input) -> ActionResult table   -- see data-model.md
function plugin.health() -> { {status=, message=}, ... }
```

`on_action` and `health` run on a worker of their own (fresh Lua state, `init` run first),
with the same sandbox, deadline and memory limit as events. The store they see is the
plugin's shared store.
