# HTTP API Contract Changes: Local CI Gate

## GET /health (new, unauthenticated)

Response `200 OK`, `application/json`:

```json
{ "status": "ok", "version": "0.1.1" }
```

- Never requires a token and never returns config, token, or Put.io data.

## POST /local/queue/:file_id/{action} (changed)

Changed from the previous (invalid) `/local/queue/*path/{action}`. Requires the API token,
as before.

| Action | Path |
|--------|------|
| Move one position earlier | `POST /local/queue/:file_id/move_up` |
| Move one position later | `POST /local/queue/:file_id/move_down` |
| Move to front | `POST /local/queue/:file_id/move_first` |
| Move to back | `POST /local/queue/:file_id/move_last` |
| Skip item | `POST /local/queue/:file_id/skip` |

- `:file_id`: Put.io file id of the queued item (integer); the handlers key on `file_id`.
- `200`: `SuccessResponse` (unchanged shape).
- `400`: `:file_id` is not an integer, or the move is impossible (already first/last).
- `404`: no queue item with that file id. (The handler must return NotFound, not 500.)
- `401`: missing or invalid token.

Unchanged: `GET/PUT /local/queue`, `GET /local/queue/stats`, and `POST /local/queue/clear`,
`/pause` and `/resume`.
