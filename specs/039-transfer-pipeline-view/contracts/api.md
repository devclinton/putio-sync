# Contracts: Active Transfers Pipeline View

All routes sit behind the existing API authentication (bearer token or session). Times are
RFC 3339 UTC. Sizes are bytes.

## `GET /active`

The whole Active page in one call. It is built from the transfers snapshot (at most 10 s old),
the queue and the database.

```json
{
  "items": [
    {
      "key": "transfer:812",
      "name": "Some.Show.S02.1080p",
      "transfer_id": 812,
      "stage": "nas",
      "state": "downloading",
      "held": false,
      "paused": false,
      "size": 18253611008,
      "done_bytes": 6120000000,
      "percent": 33.5,
      "speed_bps": 41943040,
      "eta_seconds": 289,
      "files": { "done": 4, "downloading": 1, "queued": 7, "paused": 0, "failed": 0, "skipped": 0, "total": 12 },
      "putio_status": "SEEDING",
      "seeding": true,
      "stall": null,
      "origin": "arr:sonarr",
      "reason": null,
      "next_sync_at": null,
      "note": null,
      "links": { "transfers": "#/transfers?id=812", "queue": "#/queue?transfer=812" }
    }
  ],
  "counts": {
    "putio": { "queued": 0, "downloading": 2, "stalled": 1, "finishing": 0, "error": 1, "on_hold": 1 },
    "nas":   { "waiting_for_sync": 1, "queued": 0, "downloading": 1, "paused": 0, "failed": 0 }
  },
  "nas_paused": false,
  "putio_as_of": "2026-10-28T09:14:05Z",
  "putio_stale": false,
  "recently_finished": [
    {
      "key": "transfer:799",
      "name": "Some.Movie.2025.2160p",
      "outcome": "done",
      "reason": null,
      "size": 31000000000,
      "done_at": "2026-10-28T08:02:11Z",
      "total_seconds": 5400,
      "putio_seconds": 3100,
      "waiting_seconds": 420,
      "nas_seconds": 1880
    }
  ]
}
```

- `stage`: `putio` | `nas`.
- `state`:
  - in the `putio` stage: `queued` | `downloading` | `stalled` | `finishing` | `error`;
  - in the `nas` stage: `waiting_for_sync` | `queued` | `downloading` | `paused` | `failed`.
- `stall` has the shape from feature 007 (`since`, `reason`), or `null`.
- `putio_stale` is true when the last Put.io fetch failed. Put.io figures are then the last
  known ones, and `putio_as_of` gives their age (FR-029).
- Items are sorted for the UI: failed and error first, then downloading by speed, then the
  rest by name.
- Without a database (dev mode), it returns 503 with the usual error body.

## `POST /active/pause` and `POST /active/resume`

Request:

```json
{ "keys": ["transfer:812", "folder:movies/Some.Movie", "file:5531"] }
```

Response (200), with one result per key in the same order:

```json
{
  "results": [
    { "key": "transfer:812", "result": "paused" },
    { "key": "folder:movies/Some.Movie", "result": "paused" },
    { "key": "file:5531", "result": "not_found" }
  ]
}
```

- `result` for pause: `paused` (NAS stage), `held` (Put.io stage or waiting for sync),
  `already` or `not_found`.
- `result` for resume: `resumed`, `already` or `not_found`.
- Neither route sends any request to Put.io (SC-008).
- 400 if `keys` is empty or has more than 500 entries.

## `/local/queue` changes

| Route | Change |
|---|---|
| `POST /local/queue/pause` | Turns the Pause NAS downloads switch on. Response `{ "nas_paused": true }`. It no longer rewrites row statuses. |
| `POST /local/queue/resume` | Turns the switch off. Response `{ "nas_paused": false }`. |
| `POST /local/queue/{file_id}/pause` | New. Pauses one file. A running file stops and keeps its partial file. |
| `POST /local/queue/{file_id}/resume` | New. Puts one paused file back in the queue in its old place. |
| `POST /local/queue/{file_id}/retry` | New. Puts one failed file back to `pending` in its old place. 409 if the file is not failed. |
| `GET /local/queue?transfer={id}` | New filter: only that transfer's files. |
| `GET /local/queue` | Response gains `nas_paused`. |

## `GET /status` additions

```json
{
  "nas_paused": false,
  "active": {
    "total": 7,
    "putio": 3,
    "nas": 3,
    "failed": 1,
    "top": [
      { "key": "transfer:812", "name": "…", "stage": "nas", "state": "downloading",
        "percent": 33.5, "eta_seconds": 289 }
    ]
  }
}
```

- `top` lists up to 5 items, busiest first: downloading before queued, then by speed.
- `failed` counts `error` and `failed` items, and is also included in `putio` or `nas`.
- `downloads` (the list of single files) stays in the response for API clients. The Status
  page no longer uses it.

## Transmission view (feature 011)

A paused or held item, or any item while the switch is on, is listed with `status: 0`
(stopped), `isFinished: false` and `leftUntilDone > 0`.

## Metrics

| Name | Type | Labels |
|---|---|---|
| `putio_sync_pipeline_items` | gauge | `stage` (`putio`, `nas`), `state` (the 10 states) |
| `putio_sync_pipeline_actions_total` | counter | `action` (`pause`, `hold`, `resume`) |
| `putio_sync_nas_downloads_paused` | gauge | none (0 or 1) |

## Configuration

```toml
[transfers]
# How long finished transfers stay in the Active page's "Recently finished" list
finished_retention_hours = 24   # 1–720
```

## UI

- **Nav**: an "Active" link after Status (`#/active`).
- **Active page**:
  - two rows of count chips; a chip filters on click, and "Show all" clears the filter;
  - a name filter;
  - a table with a selection box, name with its origin label (Radarr/Sonarr category, watch
    folder or web UI) when known, stage badge, state, size, progress bar, files, speed, time
    left, a Pause/Resume button, and a link to the Transfers or Queue page;
  - when nothing is active, "Nothing is downloading right now" with a link to the Transfers
    page to add a transfer;
  - the selection bar with Pause and Resume;
  - the Pause NAS downloads switch, then Recently finished.
- **Held row**: the "On hold" badge, with the text "Put.io keeps downloading; NAS download
  held".
- **Paused row**: speed shows "—", time left shows "Paused".
- **Status page**: the Active transfers card (FR-030). When the switch is on, all three pages
  show the banner "NAS downloads paused" with a Resume button.
- **Queue page**:
  - one switch button replaces Pause all and Resume all;
  - each row gets Pause or Resume, and a failed row gets Retry;
  - `?transfer=` shows "Showing files of <name>" with a Show all link.
