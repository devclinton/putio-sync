# Contract: What's new API

Both routes need the API token (`Authorization: Bearer …` or the UI session), like every
other data route. Neither exists in setup mode (the setup router answers 503 for unknown
protected paths as it does today).

## GET /whats-new

The built-in changelog with a `new` flag per entry.

```json
{
  "version": "0.1.1",
  "commit": "8eb8687…",
  "new_count": 2,
  "releases": [
    {
      "name": "Unreleased",
      "date": null,
      "kinds": [
        {
          "name": "Added",
          "entries": [
            { "id": "a1b2c3d4e5f60718", "text": "A first-run setup wizard. …", "new": true }
          ]
        }
      ]
    }
  ]
}
```

- `commit` is `null` when the build has no commit (`PUTIO_SYNC_GIT_SHA` unset).
- `new_count` counts distinct new ids.
- First run (no seen rows): every id is recorded as seen and `new_count` is 0.
- No database, or the database fails: `new_count` is 0, every `new` is false; a database
  error is logged at `warn`. Never an error status for that.

## POST /whats-new/seen

```json
{ "ids": ["a1b2c3d4e5f60718", "…"] }
```

- Records the ids that are in the built-in changelog; other ids are ignored. At most 1000
  ids; more → `400`.
- `204 No Content` on success, also when there is no database (nothing to record).
- `500` when the database write fails (the panel closes anyway; it shows again next load).
