# HTTP API: Doctor

All routes sit behind the existing UI token (`Authorization: Bearer …`). Without a valid
token they return 401. Bodies are JSON and follow [data-model.md](../data-model.md).

## GET /doctor

Returns the latest report.

- `200` `Report`.
- `404 {"error": "no doctor run yet"}` when nothing has run (the UI then calls `POST /doctor/run`).

## POST /doctor/run

Runs every check now. If a run is already in progress, the request waits for it and returns
that run's report (single flight).

- `200` `Report`.

## POST /doctor/fixes/{fix_id}/preview

Detects again and lists what the fix would act on. Changes nothing.

- `200` `FixPreview`.
- `404` for an unknown fix id.

## POST /doctor/fixes/{fix_id}/apply

Body: `FixRequest`, for example `{"items": ["101", "205"], "confirm": true}`.

- `200` `FixOutcome`. Items that no longer qualify are in `skipped`.
- `400` when the body is invalid, `items` is empty, or a destructive fix has no
  `confirm: true`.
- `404` for an unknown fix id.
- `409` for `empty_folders.clean` when a fresh dry run would remove a folder that is not in
  `items` ("the list changed, preview again"), or while a cleanup is already running.

## Re-add (refactor, no contract change)

`POST /putio/transfers/{id}/readd` keeps its request and response. Its logic moves to
`PutioApiClient::readd_transfer`.
