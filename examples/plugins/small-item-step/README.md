# small-item-step

A plugin that runs as a **pipeline step** (feature 057). Its `on_item(item, stage)` is
called for each item that reaches the stage, on the plugin's own queue and under its own
time and memory limits.

It adds up the item's file sizes. An item under **Smallest item (MB)** (default 50) is
skipped before the download (`pre_download`) and fails the step at a later stage.

Turn the plugin on, then add a hook that names it, in Settings › Integrations › Event hooks
(Runs as: a pipeline step, Runs: Plugin) or in the config file:

```toml
[[hooks]]
name = "no-small-items"
stage = "pre_download"
plugin = "small-item-step"
required = false
```

What `on_item` may return:

| Return | The step |
|---|---|
| nothing, `true`, `"done"` | passes |
| `"skip"` | skips the item at `pre_download`; counts as done at later stages |
| `{ fail = "why" }` | fails with that message |
| an error | fails with the error |

See [PLUGINS.md](../../PLUGINS.md#pipeline-steps).
