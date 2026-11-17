# Quickstart: checking the command palette

## Automated

```bash
make ci
```

This covers:

- `putio-sync-file-manager` unit tests for `local_files::search`: match, hidden files
  skipped, symlinks not followed, the 50-hit limit, the time limit, `truncated`.
- `putio-sync-client` mockito test for `search_files`: the request shape, the cap, and
  leaving out the trash.
- `putio-sync-api/tests/palette_search.rs`: both routes, including 401 without sign-in, 400
  for a short `q`, the mocked put.io results, temp-dir roots and `../` safety.
- `node --test` over `tests/ui/fuzzy.test.mjs` and `tests/ui/settings_catalog.test.mjs`.
  The second checks that every group field has an entry and that every address resolves
  through `fieldId`.

## By hand (dev server, mocked or real account)

1. Sign in and go to Status. Press Cmd/Ctrl+K. The palette opens with the focus in the search
   box.
2. Type `queue` and press Enter. The Queue page opens.
3. From Queue, open the palette and type `bandwidth`. Pick the Bandwidth card. Settings opens
   at that card.
4. Type `clear`. Pick "Clear completed". A confirmation shows, and Cancel changes nothing.
5. Type part of a queued file's name. Pick it. The Queue page opens filtered to that item.
6. Type part of a put.io file's name and wait. Put.io files are listed. Picking one opens its
   folder with the row marked.
7. Type part of a local file's name. Local files are listed with their sync folder. Picking
   one opens its folder with the row marked.
8. Press Cmd/Ctrl+K on the login page: nothing happens. With a confirm dialog open: nothing
   happens.
9. Repeat at phone width and in the dark theme. Repeat in another UI language: the palette
   text is translated.
