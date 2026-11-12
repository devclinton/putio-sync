# Quickstart: checking 059 by hand

The automated tests cover every scenario with a local registry (`mockito`) and archives built in
the test. This is the hand check on a running daemon, with no GitHub involved.

## Set up a local registry

```bash
cd ~/dev/putio-sync-plugin-registry
python3 scripts/registry.py pack examples/webhook-notifier /tmp/reg/webhook-notifier-1.0.0.tar.gz
```

Write `/tmp/reg/index.json` with one entry from `examples/entries/webhook-notifier.toml`, its `url`
set to `http://127.0.0.1:8099/webhook-notifier-1.0.0.tar.gz` and `sha256` to the printed value.
Serve it with `python3 -m http.server 8099 -d /tmp/reg` and set:

```toml
[plugin_registry]
url = "http://127.0.0.1:8099/index.json"
```

## Checks

1. **Links (US1)**: open Settings › Plugins › ntfy. Docs, Source and the license show; links open
   in a new tab. Add `docs = "ftp://x"` to a folder plugin and reload: it is **Broken** with
   `plugin.toml: docs: …`.
2. **`plugins check`**: `putio-sync plugins check examples/webhook-notifier` prints `ok` and exits 0;
   break a line in `main.lua` and it prints the line and exits 1.
3. **Browse (US2)**: **More plugins** lists webhook-notifier with author, hosts, links and
   **Install**. Search for "zzz" hides it.
4. **Install**: press **Install**. `plugins/webhook-notifier/` holds the files and `.registry.json`;
   the plugin is off and marked "From the registry". **Enable** asks to approve the URL's host.
5. **Refused archive**: change one byte of the archive and reinstall after **Remove**: the page
   names the SHA-256 check and `plugins/` is unchanged.
6. **Name in use**: make `plugins/webhook-notifier/` by hand (no `.registry.json`): the entry shows
   **Name in use**.
7. **Offline**: stop the server and press **Refresh**: installed plugins still work; the list shows
   with its age and "could not be reached".
8. **Off**: turn the registry off; with the server's log open, reload the page: no request.
9. **Update (US3)**: pack 1.1.0 with an extra fixed host, add it on top of `releases`, **Refresh**.
   The tab shows **Update to 1.1.0** and the new host; **Update** keeps settings, the plugin shows
   **Needs approval**. Edit `main.lua` first and **Update** asks before replacing it.
10. **Rollback**: list a 1.2.0 whose `main.lua` has a syntax error but passes the checksum: the
    update fails, 1.1.0 is running again, the page says why.
11. **Withdrawn**: add `"withdrawn": "test"`: the installed plugin keeps running, its tab shows the
    reason, and `putio-sync doctor` warns.

## Gate

```bash
make ci
```

Also run `python3 scripts/registry.py check --offline examples/entries/*.toml` in the registry
repository.
