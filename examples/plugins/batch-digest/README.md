# batch-digest

Groups finished downloads and writes one digest per sync run to the plugin's store, under
`digests` (the last 20 are kept). It shows the two lifecycle callbacks of feature 057:

- `on_unload()` runs before the plugin's state is dropped: a reload, turning the plugin
  off, or the daemon stopping. The downloads grouped so far are written then, so none
  are lost. It has a short time limit; a state that already broke or timed out gets no
  call.
- `on_config_changed(config)` runs when the plugin's own settings are saved, with the new
  values. Here it picks up a new **Largest digest** without a reload.

It needs the `store` permission only. A real plugin would send the digest somewhere
(`putio.http.request`) where this one writes it to the store.

See [PLUGINS.md](../../PLUGINS.md#lifecycle-callbacks).
