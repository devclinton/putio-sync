# Quickstart: Validating the Web UI

These steps prove the feature works. Steps 1–4 run without Put.io (Principle III). Steps
5–6 run against the live NAS after the operator deploys.

## Prerequisites

- Rust toolchain from `rust-toolchain.toml`. No Node, no browser driver.
- A desktop browser and, for step 3, a phone on the same network (or the browser's
  device mode at 375 px).

## 1. Local gate

```bash
make ci
```

Expected: fmt, clippy, tests and the release build pass. The new tests cover:

- **putio-sync-core:** `[api]` defaults, env overrides, invalid `listen`/`bind`/token
  values rejected with the `invalid <setting>` message.
- **putio-sync-download:** `SyncStatus` transitions; `request_sync()` returns
  `InProgress` while a sync runs and wakes the loop otherwise; `enqueue_tree` from a
  folder id queues files, skips blocked and already-known ones.
- **putio-sync-db:** `history().query(..., name)` matches case-insensitively and treats
  `%`, `_` and `\` literally.
- **putio-sync-api** (`tests/`): no token configured → 401 naming the setting; wrong
  token → 401; sixth failure → 429 with `Retry-After`; `/auth/login` 204/401; `/status`
  with and without a database; `/local/sync` 202 then 409 while in progress;
  `/local/queue` folder enqueue counts; each queue action; `/history?name=`; delete
  guard 409; `/` and every asset referenced by `index.html` served with the right content
  type; the secrets sweep (no test token in any response body or captured log line).

## 2. Click through against a mocked Put.io

```bash
cargo run -p putio-sync-api --example mock_ui
```

It prints `http://127.0.0.1:<port>/` and a test token, serves the UI with a mockito Put.io
(three folders, three files, a running and a failed transfer) and a seeded temp database. Check:

1. Opening the URL shows the sign-in page. A wrong token says "Token not accepted"; six wrong
   tokens show the rate-limit message with a countdown.
2. After sign-in, Status shows version, uptime, queue counts, disk and "Put.io: unknown"
   (no sync yet). Reload keeps you signed in.
3. Queue lists the seeded rows; move/skip/pause/resume/clear update within 3 s; "Sync now"
   turns "in progress" then shows the result, and a second click during the sync says it
   is already running.
4. Put.io browses the mocked folders; the filter box narrows the list; "Download" on a
   folder toasts found/queued/skipped; the delete button is absent on a queued file and
   asks for confirmation on another. Opening `#/putio/110` directly shows the breadcrumb
   "Put.io / Movies / Extras".
5. Transfers lists the mocked transfers; add a magnet link; Retry shows only on the failed
   one; remove asks for confirmation.
6. History pages and filters by action and name; Settings loads all four groups, a save
   shows the API's message, an invalid value shows the validation error.
7. Sign out returns to the sign-in page and `localStorage` holds no token.

## 3. Phone width

Open the same URL on a phone or in device mode at 375 px. Every page fits without
horizontal scrolling; tables show as cards; the actions remain reachable.

## 4. Auth and secrets with curl

```bash
PORT=15432   # or the mock_ui port
curl -i http://127.0.0.1:$PORT/health                      # 200 without a token
curl -i http://127.0.0.1:$PORT/status                      # 401
curl -i -H 'Authorization: Bearer wrong' http://127.0.0.1:$PORT/status   # 401
for i in 1 2 3 4 5 6; do curl -s -o /dev/null -w '%{http_code}\n' \
  -H 'Authorization: Bearer wrong' http://127.0.0.1:$PORT/status; done  # 401 ×5 then 429
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:$PORT/status | grep -c "$TOKEN"  # 0
```

Run the daemon once with no `[api] auth_token` and confirm the single WARN line and that
`/status` answers 401 with the "not configured" body.

## 5. Deploy on the NAS (operator)

1. Add to `/mnt/user/appdata/putio-sync/config.toml` (or set the env variables in the
   container template): `[api] auth_token = "<new random token>"`; `listen` and `bind`
   can stay at their defaults.
2. Publish port 15432 on the container.
3. Restart. Expected log lines: `API listener enabled bind=0.0.0.0:15432 token=set`,
   and the usual daemon lines. The daemon work does not depend on the listener: if the
   port is taken, one `API listener failed to start` ERROR appears and syncing continues.
4. Open `http://192.168.1.50:15432/` from a desktop and a phone and sign in.

## 6. Live check

- Status shows the real queue counts, the last sync time within the configured interval,
  disk figures for `/mnt/user/appdata/torrents/downloaded`, and "Put.io: ok".
- "Sync now" produces a sync log line immediately.
- `grep -c '<token>' <container log>` is 0; the token never appears in a page or response.
