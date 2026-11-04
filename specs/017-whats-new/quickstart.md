# Quickstart: What's New

Validates the feature end to end on a scratch instance. Don't use port 15432 (the live
instance); these steps use 15999.

## Automated

```bash
cargo test -p putio-sync-api --test whats_new
cargo test -p putio-sync-api changelog
cargo test -p putio-sync-db whats_new
TMPDIR=<scratchpad> make ci
```

## By hand

1. Start a scratch `server` with a temp config and database on port 15999 and open the UI.
   Sign in: no panel (first run). Status → the version links to What's new, which lists
   every section of `CHANGELOG.md`.
2. Make two entries "new": `sqlite3 <db> "DELETE FROM whats_new_seen WHERE rowid IN
   (SELECT rowid FROM whats_new_seen LIMIT 2)"`. Reload: the panel lists those two under
   their release and kind, and the page marks them new.
3. Close the panel. Reload, and open the UI in a private window: no panel.
4. Check an entry with a link and inline code renders a link and code, and that a
   `<b>` typed into an entry would show as text (parser unit test covers this).
5. Start the daemon with no config (setup mode): `GET /whats-new` is not served
   (`503`), and the wizard has no panel.

See [contracts/whats-new-api.md](contracts/whats-new-api.md) for the responses.
