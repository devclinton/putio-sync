# Quickstart: Select All, Sort and Filter

## Automated

```bash
node --test putio-sync-api/tests/ui/
cargo test -p putio-sync-api --test ui_js --test ui_assets
```

Both pass; `ui_assets` confirms every import resolves and the UI is under its size budget.

## In a browser

Run the daemon locally (`make run` or the dev config), sign in, then:

1. **Put.io**: open a folder with several files. Type `-*sample* *.mkv`: only non-sample mkv
   files show and the heading reads "N of M items". Tick Select all: the count equals the shown
   rows minus queued ones. Clear the filter: rows hidden before are not selected.
2. Click **Size** twice: largest first, folders still on top. Reload: filter and sort are kept.
   Open a subfolder: sort is kept, filter cleared.
3. **Transfers**: type a name fragment; wait 10 s: the filter still applies after refreshes.
   Sort by Progress; Select all; Retry and Remove act on the selected transfers only.
4. **Local files**: in a temp download folder with `a.exe`, `Photoshop.exe`, `b.mkv` and a
   folder, type `-*photoshop* *exe*`: only `a.exe`. Select all, Delete: the confirm names one
   file; after it, the others remain.
5. Phone width (375 px): the Sort menu next to the filter changes the order; Select all works.
