# API contract: file search

Both routes need the same sign-in as the rest of the API (constitution VI). Without it they
return 401.

## GET /putio/files/search?q=<text>

- `q`: 2–200 characters after trimming. Otherwise 400.
- 200:
  ```json
  { "files": [ { "id": 123, "name": "Movie.mkv", "parent_id": 45, "is_folder": false, "size": 1048576 } ] }
  ```
  At most 50 results, from put.io's first page of results only. Files in the trash are left out.
- put.io fails or times out: 502 with the usual `{ error, details }` body. The palette shows
  "Couldn't search" in that group.

## GET /local/files/search?q=<text>

- `q`: as above.
- 200:
  ```json
  {
    "hits": [ { "root": "download", "root_label": "Downloads", "path": "TV/Show/ep1.mkv", "name": "ep1.mkv", "kind": "file", "size": 734003200 } ],
    "truncated": false
  }
  ```
  - Searches every configured root, with one limit for all of them: 50 hits or 2 seconds.
  - `truncated` is true when the search stopped early.
  - Shortcuts (symlinks) are not followed, and names starting with `.` are skipped.
- No roots configured: 200 with `{ "hits": [], "truncated": false }`.

# UI contract: deep links

| Link | Effect |
|---|---|
| `#/putio/<folder_id>?select=<name>` | opens that folder and marks the row named `<name>` (new: `select`) |
| `#/files/<root>?path=<folder>&select=<name>` | as above, for a local folder (new: `select`) |
| `#/queue?file=<file_id>` | existing filtered view |
| `#/settings/<section>?field=<address>` | existing jump to a card or field |

# UI contract: shortcut

- Cmd+K on macOS, Ctrl+K elsewhere. Opens the palette from any signed-in route.
- Does nothing on `#/login`, `#/setup` and `#/signin`, and while the confirm dialog is open.
