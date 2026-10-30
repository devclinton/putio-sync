# Contract: HTTP API changes (feature 040)

All routes keep the API token (Constitution VI). Only the fields below change.

## `GET /settings`, `PUT /settings`

`download.skip_existing` is replaced by `download.on_existing`.

```json
{ "download": { "on_existing": "resume", "resume_enabled": true, "...": "..." } }
```

- `on_existing`: `"resume" | "overwrite" | "keep_both"`. Any other value is 400 and changes
  nothing.
- A `PUT` that still sends `skip_existing` (an old UI tab) is accepted when `on_existing` is
  absent and mapped as in the config contract.
- `on_existing` and `resume_enabled` are live settings: the download manager uses them from the
  next download, like `delete_after_download` today (`DOWNLOAD_LIVE`).

## Folder rules (`/rules`, feature 009)

**RuleBody**: `skip_existing` is replaced by `on_existing`, `null` for "use global".

```json
{ "preset": "video", "on_existing": "overwrite", "...": "..." }
```

**Effective**: `skip_existing` is replaced by

```json
{ "on_existing": { "value": "overwrite", "from": "backups" } }
```

`from` is `null` when the global setting applies. A rule that only has the old `skip_existing`
reports its mapped value with that rule as `from`. The `/rules` global summary reports
`on_existing` in place of `skip_existing`.

## `GET /history`

Each entry gains two nullable fields:

```json
{
  "filename": "Movie.mkv",
  "action": "downloaded",
  "local_path": "/downloads/movies/Movie (1).mkv",
  "outcome": "saved_as"
}
```

`outcome` is one of `skipped`, `resumed`, `restarted`, `overwritten`, `saved_as`, or `null` (a
plain new download, a delete, or a row from before this feature).

## `GET /local/downloads`

Each active download gains `phase`: `"downloading"` or `"checking"`. While `checking`,
`bytes_downloaded` and `percentage` measure the checksum read, and `speed_bps` is the read speed.

## UI

- Settings → Download: a select "When a file already exists" (Resume, Overwrite, Keep both),
  with a help line per value. "Keep both" shows `Movie (1).mkv` as the example. It replaces the
  "Skip existing" toggle. "Resume" keeps its toggle, now with help text that matches what it
  does.
- Put.io page rule editor and `rules.js`: the same select plus "Use global". The rule card shows
  the value and where it comes from.
- History: the saved path under the name when it differs from `<folder>/<filename>`, and a badge
  for `resumed`, `overwritten`, `saved_as` and `restarted`.
- Queue: an active download in phase `checking` shows "Checking" in place of the speed.
