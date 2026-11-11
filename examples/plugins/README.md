# Example plugins

Plugins that take part in a sync (feature 028). Copy a folder into your plugins folder,
reload plugins on the Plugins tab, then enable it. See [PLUGINS.md](../../PLUGINS.md#sync-rules)
for how sync rules work.

| Folder | Sync rule | What it does |
|---|---|---|
| [`min-size-filter`](min-size-filter/) | filter | Skips videos smaller than a size: samples, trailers, extras |
| [`clean-names`](clean-names/) | rename | `Show.S01E02.1080p.WEB.x264-GRP.mkv` → `Show S01E02.mkv` |
| [`tv-folders`](tv-folders/) | folder | Puts episodes in `TV/<Show>/Season NN` |

A test loads each of them (`putio-sync-plugins/tests/examples.rs`), so they keep working.
