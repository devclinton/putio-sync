# Contract: Transfers API, metric and UI (feature 007)

All routes require the API token (Constitution VI), like the rest of `/putio/transfers`.

## `GET /putio/transfers` — additions to each item

```json
{
  "id": 42, "name": "Ubuntu ISO", "status": "DOWNLOADING", "size": 1024,
  "percent_done": 97, "down_speed": 0,
  "availability": 92,
  "magnet": "magnet:?xt=urn:btih:…",
  "stall": { "since": 1793000000, "seconds": 32400, "reason": "availability 92%" }
}
```

- `availability`: integer or omitted.
- `magnet`: string or omitted (research R7).
- `stall`: object or omitted. `since` is Unix seconds; `seconds` is elapsed at response time;
  `reason` is one of `availability N%`, `no peers`, `no progress`.
- `GET /putio/transfers/{id}` carries the same fields.

## `POST /putio/transfers/{id}/readd`

No body. Steps: get transfer → cancel → add magnet to the same `save_parent_id`.

| Case | Status | Body |
|---|---|---|
| Done | 200 | `{ "cancelled_id": 42, "transfer": <new transfer item> }` |
| Transfer has no magnet | 400 | `{ "error": "Transfer 42 has no magnet link to re-add" }`; nothing cancelled |
| Transfer not found | as Put.io error (unchanged mapping) | nothing cancelled |
| Cancel fails | as Put.io error | nothing added |
| Add fails after cancel | 500 | `error`: "Cancelled transfer 42 (name) but couldn't add it again: …", `details`: the magnet link |

Logged at info with `transfer_id`, `name`, `new_transfer_id`; never the magnet (FR-016).

## Metric

| Name | Type | Labels | Meaning |
|---|---|---|---|
| `putio_sync_transfers_stalled` | gauge | none | transfers stalled at the last check |

Basic family (always exported). Starts at 0.

## UI: Transfers page (`ui/pages/transfers.js`)

- Status column: a `badge warn` reading `stalled 9h` next to the status badge when `stall`
  is present; its title (tooltip) and a small line under the name give the reason.
- Actions: `Copy magnet` when `magnet` is present (clipboard; toast "Magnet link copied").
  `Re-add` when `magnet` is present and `stall` is present, after `confirm` ("Re-add <name>?
  Put.io will cancel it and start it again, usually from 0%."). Success toast names the new
  transfer; failure toast shows the error and details.
