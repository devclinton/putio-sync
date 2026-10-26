# Contract: Web UI

The UI is a hash-routed single page served at `GET /` from `putio-sync-api/ui/`. No
build step; the files are embedded in the binary as they are on disk.

## Shell (`index.html`, `app.js`, `app.css`)

- Top bar: product name, page links (Status, Queue, Put.io, Transfers, History,
  Settings), sign-out button, a link to the metrics port if `metrics.enabled` (read from
  `/status`; omitted when unknown).
- Hash routes: `#/status` (default), `#/queue`, `#/putio[/<folder_id>]`, `#/transfers`,
  `#/history`, `#/settings`, `#/login`.
- Auth gate: on load, if no stored token, or if any request returns 401, the page shows
  `#/login` and clears the stored token. 429 shows the login page with "too many
  attempts, try again in N s" using `Retry-After`.
- Disconnected banner: shown after a network error on any poll, hidden after the next
  success. Polling backs off to 10 s while shown.
- Polling pauses while `document.hidden` and resumes on `visibilitychange`.
- Put.io and Transfers pages show a request error with a Retry button instead of an
  empty list.
- `<noscript>` in `index.html` says a current browser with JavaScript is required.
- Phone width (≤ 600 px): the nav collapses to a wrapping row of links; tables render as
  stacked cards (`display: grid`), no horizontal scroll (FR-019).
- Toasts for action results (success 3 s, error until dismissed), showing the server's
  `error`/`details` text.
- Styles use `prefers-color-scheme` for dark mode; no external fonts or CDN assets.

## `api.js`

`request(method, path, body?)`: adds `Authorization: Bearer <token>` from
`localStorage['putio-sync.token']`, sends/receives JSON, throws `{status, error,
details}` on non-2xx, dispatches `auth:expired` on 401. `login(token)` posts to
`/auth/login` and stores the token only on 204. `logout()` clears it.

## Pages

| Page | Data | Routes called | Refresh | Actions (confirmation text) |
|---|---|---|---|---|
| Login | whether a token is configured; token field | `GET /auth/status`, `POST /auth/login` | — | Sign in. When `token_configured` is false: "Set `[api] auth_token` or `PUTIO_SYNC_API_TOKEN` and restart" instead of the form. |
| Status | every `/status` section; "unavailable" sections show their reason | `GET /status` | 3 s | Sync now (`POST /local/sync`; 409 shows "sync already running") |
| Queue | queue table: name, size, path, state, priority, age; active downloads: name, done/size, %, speed, ETA; queue counts | `GET /local/queue`, `GET /local/downloads` | 3 s | Move up/down/first/last, skip (`POST /local/queue/{id}/…`); pause all / resume all (`POST /local/queue/pause|resume`); clear completed (`POST /local/queue/clear`, "Remove N completed items from the queue?"); cancel download (`DELETE /local/downloads/{id}`, "Cancel download of <name>?"); sync now |
| Put.io | breadcrumb from the sync root; folder listing: name, type, size, date; name filter (client-side on the current folder); file details panel | `GET /putio/files?parent_id=`, `GET /putio/files/{id}` | on navigation | Download (`POST /local/queue {file_id}`, toast shows found/queued/skipped); Delete (`DELETE /putio/files/{id}`, "Delete <name> from Put.io? This cannot be undone."; the button is hidden when the id is in the queue as pending/downloading/paused, which the page knows from its last `GET /local/queue`) |
| Transfers | table: name, status, %, size, speed; add form: magnet/URL text or `.torrent` file (read with `FileReader`, sent base64 as `file_data`) | `GET /putio/transfers`, `POST /putio/transfers` | 5 s | Start/stop (`POST /putio/transfers/{id}/start|stop`); Remove (`DELETE /putio/transfers/{id}`, "Remove transfer <name>?") |
| History | newest first; filters: action (all/downloaded/deleted), name fragment; totals from `/history/stats`; prev/next paging of 50 | `GET /history?action&name&limit&offset`, `GET /history/stats` | on filter/page change | none |
| Settings | four groups (download, watcher, logging, notifications) as forms mirroring each GET body; after a save the page shows the API's `message` verbatim, which already says whether the change is live or needs a restart | `GET/PUT /settings/{download,watcher,logging,notifications}` | on load, after save | Save per group; validation errors from the 400 body shown next to the form |

Sync root for the Put.io page: folder `0` (the folder sync walks the whole account from
the root, `DownloadManager::list_all_files(0)`), so the breadcrumb starts at "Put.io".

## Confirmations (FR-020)

Delete file, remove transfer, cancel download and clear completed use a modal dialog
naming the target (or the count), "Cancel" (default focus) and the destructive verb.
Skip, move, pause and resume are reversible and need no confirmation.

## What the UI never does

- Stores or shows the Put.io token, app password or API token after sign-in.
- Offers watcher start/stop (only "scan now"; see plan.md Complexity Tracking).
- Deletes local files.
- Loads anything from a third-party origin.

## Manual check script

`cargo run -p putio-sync-api --example mock_ui` prints a URL and a test token, serves the
UI with a mocked Put.io (two folders, three files, one transfer) and a seeded queue, so
every page can be clicked through offline (see quickstart.md).
