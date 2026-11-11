# Example plugins

Plugins that take part in a sync (feature 028), and starters for event plugins (055).
Copy a folder into your plugins folder, reload plugins on the Plugins tab, then enable it. See [PLUGINS.md](../../PLUGINS.md#sync-rules)
for how sync rules work.

| Folder | Sync rule | What it does |
|---|---|---|
| [`min-size-filter`](min-size-filter/) | filter | Skips videos smaller than a size: samples, trailers, extras |
| [`clean-names`](clean-names/) | rename | `Show.S01E02.1080p.WEB.x264-GRP.mkv` → `Show S01E02.mkv` |
| [`tv-folders`](tv-folders/) | folder | Puts episodes in `TV/<Show>/Season NN` |
| [`rust-clean-names`](rust-clean-names/) | rename | `clean-names` in Rust, built to WebAssembly |
| [`go-min-size-filter`](go-min-size-filter/) | filter | `min-size-filter` in Go, built to WebAssembly |

Three more are the plugin workbench's starters (feature 055); **New plugin** writes the
same files, and a test checks they match and pass a dry run:

| Folder | Runs on | What it does |
|---|---|---|
| [`webhook-notifier`](webhook-notifier/) | `download.finished`, `download.failed` | Posts each event to a webhook as JSON |
| [`store-counter`](store-counter/) | `download.finished` | Counts downloads per day in the store; reports it from `health` |
| [`tick-poller`](tick-poller/) | `on_tick`, `sync.completed` | Checks a URL and logs when its status changes |

The WebAssembly ones (feature 029) have their build steps in their own README; see
[PLUGINS.md](../../PLUGINS.md#webassembly-plugins). A test loads each of them
(`putio-sync-plugins/tests/examples.rs`), so they keep working.
