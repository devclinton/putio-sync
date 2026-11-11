# Quickstart: Plugin Workbench

Run from the repository root.

## Tests

```bash
cargo test -p putio-sync-plugins --test workbench --test examples
cargo test -p putio-sync-plugins --lib
cargo test -p putio-sync-api --test plugins_workbench --test ui_assets --test ui_i18n
node --test putio-sync-api/tests/ui/workbench.test.mjs
```

Expected: a dry run of a webhook plugin records the request with the secret masked and a local
server gets nothing; a dry run's store writes show in `store_after` and the store file is
unchanged; `putio.fs.write` in a dry run writes nothing; a real run reaches the local server;
a Lua error names `main.lua` and the line; the WebAssembly example runs the same way; save keeps
a `.bak` and refuses a stale version; the editor routes answer 403 with the setting off; the
static UI stays under 488 KiB.

## By hand

1. Start the server, open Settings › Plugins, open **Plugin workbench**.
2. Pick `ntfy`, choose the event `download.finished`, press **Run**: the result lists one request
   marked not sent, and the phone gets nothing.
3. Press **Turn the editor on**, then **New plugin**, starter **Webhook notifier**, name
   `my-hook`. **Edit** shows both files with line numbers.
4. Add an error on line 5, **Check**: the problem names `main.lua:5`. Fix it, **Save**: the
   folder has `main.lua.bak`.
5. Change the log line without saving and **Run**: the result says it ran the unsaved text and
   shows the new line.
6. Pick a WebAssembly example: the manifest shows read-only with the install form; **Run** the
   `filter` hook with the sample file and see its answer.
7. Every help link opens a page on the docs site.
