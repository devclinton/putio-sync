# Quickstart: Configuration Assistant

## Automated

```bash
cargo test -p putio-sync-core assistant
cargo test -p putio-sync-api --test settings_assistant
cargo test -p putio-sync-doctor config
cargo test -p putio-sync-api --test ui_assets
```

## By hand

Run a server on a free port (not 15432) with a scratch config, e.g.
`putio-sync --config /tmp/x/config.toml server --port 15999`, sign in to the web UI, open
Settings.

1. Download card: change Workers 4 → 8. The preview lists `workers 4 → 8 · applies now`.
   The config file is unchanged until Save. (US1)
2. Torrent watcher: turn on with a folder that does not exist. The preview shows the
   refusal; Save is not needed to see it. (US1)
3. Download folder `/tmp/dl` and Delete from Put.io on: the preview shows a danger warning;
   after Save the banner at the top shows it, and the Doctor page's Config check lists it.
   (US2)
4. Each field shows an "Affects …" line with an applies-now or restart badge. (US3)
5. Logging card → Defaults: the form shows `info`; the preview lists the differences;
   reloading shows the saved values. (US4)
6. Backup and restore → Export: the file has no `[auth]`, `[arr]` or `auth_token`. Change
   Workers, import the file: the preview lists `download.workers`; Apply restores it. (US5)
