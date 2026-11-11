# Quickstart: sync extension points

## Automated

```bash
cargo test -p putio-sync-core sync_hooks
cargo test -p putio-sync-plugins --test examples --test hooks
cargo test -p putio-sync-download --test sync_hooks
```

Expected: the three examples load from `examples/plugins/` and answer as their READMEs say;
a slow or failing plugin falls back and pauses after 3 failures; a sync pass skips the small
sample, saves the renamed file in the plugin's folder.

## By hand

1. Copy `examples/plugins/min-size-filter` into the plugins folder.
2. Settings › Plugins › Reload plugins, then Enable it. Its tab shows "Sync rules: filter".
3. Set "Smallest file (MB)" to 50 and save. Upload a small file to Put.io and sync: the
   file is listed as blocked with "skipped by plugin min-size-filter" in the logs.
4. Do the same with `clean-names` (rename) and `tv-folders` (folder) and download an episode.
