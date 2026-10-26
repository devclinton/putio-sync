# Contract: bulk routes (feature 008)

All routes require the API token (Constitution VI), like the rest of `/putio`. Bodies are
JSON. A body that doesn't parse, or an empty id list, is 400 and nothing is sent to Put.io.
Ids are deduplicated, keeping the first occurrence.

## `POST /putio/files/delete`

```json
{ "file_ids": [101, 102, 110] }
```

| Case | Status | Body |
|---|---|---|
| Done | 200 | `{ "message": "Successfully deleted 3 file(s)" }` |
| Any id pending, downloading or paused in the queue | 409 | `{ "error": "2 of 3 items are queued or downloading", "details": "Skip or cancel them first: a.mkv, b.mkv" }`; nothing deleted |
| Put.io error | as the single delete | |

One `POST /files/delete {"file_ids": "101,102,110"}` to Put.io. Without a database the queue
check is skipped.

## `POST /putio/transfers/remove`

```json
{ "transfer_ids": [1, 4, 5] }
```

200 `{ "message": "Removed 3 transfer(s)" }`. One `POST /transfers/cancel
{"transfer_ids": "1,4,5"}`.

## `POST /putio/transfers/retry`

```json
{ "transfer_ids": [4, 6] }
```

200 always (when the body is valid):

```json
{ "retried": [4], "failed": [{ "id": 6, "error": "Put.io API error: ..." }] }
```

One `POST /transfers/retry {"id": n}` per id, in order.

## `POST /putio/transfers/clear`

```json
{ "status": "completed", "dry_run": true }
```

`status`: `completed` (COMPLETED, SEEDING), `failed` (ERROR, FAILED) or `finished` (both).
`dry_run` is required.

200:

```json
{ "status": "completed", "dry_run": true, "count": 3,
  "transfers": [{ "id": 7, "name": "Sintel", "status": "COMPLETED" }] }
```

- Dry run: `GET /transfers/list` only.
- Real run: `GET /transfers/list`, then (when anything matches) one `POST /transfers/clean
  {"transfer_ids": "7,8,9"}`; ids missing from its `deleted_ids` go in one `POST
  /transfers/cancel`. `transfers` lists what was cleared.
- Unknown `status`: 400.

## UI

- Shared module `ui/select.js`: a checkbox column plus a selection bar above the table with
  "Select all" (mixed state when partly selected), "N selected" and the page's actions,
  enabled only while something is selected.
- Put.io page: bar actions Download, Delete, Clear selection. Queued rows have a disabled
  checkbox.
- Transfers page: bar actions Retry (failed ones only), Remove, Clear selection; Clear
  completed, Clear failed, Clear all finished in the page header.
