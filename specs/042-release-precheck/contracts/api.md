# Contract: API additions (042)

Every route below sits behind the bearer token, under `/api/v1`. None of them returns a magnet
link, URL, API key or .torrent bytes.

## Transmission RPC (`/transmission/rpc`, 011): changed behaviour only

`torrent-add` with a release that fails the check:

```json
{"result": "success",
 "arguments": {"torrent-added": {"id": 41, "name": "Show.S01E03.1080p", "hashString": "…"}}}
```

The next `torrent-get` lists it with:

- `status: 0` (stopped);
- `error: 3` (local error);
- `errorString: "rejected before Put.io: no video file (found .exe, .txt)"`;
- `percentDone: 0` and `isFinished: false`, as for any failed item in 011.

Reason strings (FR-014):

| Case | `errorString` |
|---|---|
| No wanted file | `rejected before Put.io: no video file (found .exe, .7z)`. With a 009 allow list, "no audio file" or the rule's types instead of "no video file" |
| Only small wanted files | `rejected before Put.io: only a small video (12 MB, minimum 50 MB)` |
| Checked on Put.io | the same text with `before Put.io` replaced by `on Put.io` and `(checked on Put.io)` added |
| Unparseable | `rejected before Put.io: not a valid .torrent` |

A URL that redirects to `magnet:` is handled as a magnet. Today it fails as an invalid torrent.

## `GET /stats/releases?range=24h|7d|30d|90d|1y|all&tz=`

Feeds the Stats page tiles (User Stories 5 and 6). It uses the same range parsing as
`/stats/summary`.

```json
{
  "range": {"preset": "7d", "from": "…", "to": "…"},
  "rejected": {"count": 14, "bytes": 19864223744,
               "by_stage": {"before": {"count": 12, "bytes": 17179869184},
                            "putio":  {"count": 2,  "bytes": 2684354560}},
               "by_reason": [{"reason": "no_wanted", "count": 13}, {"reason": "small_wanted", "count": 1}]},
  "magnets": {"checked": 9, "found": 6, "fell_back": 3,
              "fallback_fake": 2, "fallback_putio_bytes": 2684354560,
              "caches": [{"cache": "itorrents.net", "found": 6, "not_found": 2, "timeout": 1,
                          "wrong_hash": 0, "error": 0}]},
  "reports": {"rejected": 14, "flagged_fake": 1, "flagged_not_fake": 0,
              "top_types": [{"ext": "exe", "count": 9}, {"ext": "7z", "count": 4}],
              "top_trackers": [{"host": "tracker.example.org", "count": 7}]}
}
```

The `by_reason` keys are `no_wanted`, `small_wanted`, `invalid` and `gone`.

## `POST /releases/flags`

Request:

```json
{"target": {"arr_hash": "…"} | {"file_id": 123} | {"history_id": 456},
 "kind": "fake" | "not_fake",
 "note": "optional, ≤ 500 chars",
 "blocklist": false}
```

Responses:

- 201 `{"id": 77, "kind": "flagged_fake", "name": "…", "blocklisted": false}`.
- 400 for:
  - an unknown target shape or kind;
  - a note that is too long;
  - `blocklist: true` on a target that is not an *arr item;
  - `not_fake` on a release that was never rejected.
- 404 when the target doesn't exist.

With `blocklist: true` on an *arr item:

- the item is failed with "flagged fake by the operator", and its Put.io files are deleted;
- `blocklist` is set to `pending`, and the blocklist call itself runs on the next refresh;
- `blocklisted: true` in the response means it was queued.
- Under `precheck.dry_run`, the flag is saved, but the item is not failed and nothing is
  deleted. The delete is only logged, and the response has `blocklisted: false` and
  `"dry_run": true`.

## `DELETE /releases/flags/{id}`

- 204 when the flag row was deleted. A blocklist already sent is not undone.
- 404 when the id is unknown.
- 409 when the row is a `rejected` report rather than a flag.

## `GET /releases/flags?arr_hash=…&file_id=…&history_id=…`

Answers the active flags for the targets on screen, so rows can show "Flagged". Each parameter
may repeat, up to 200 in total.

```json
{"flags": [{"id": 77, "kind": "flagged_fake", "target": {"file_id": 123}, "note": "…"}]}
```

## `GET /releases/reports?range=…&format=json|csv`

Downloads the reports.

- `json`: an array of rows with the columns from data-model.md `release_reports`, with `types`
  and `tracker_hosts` as arrays.
- `csv`: one row per report. `types` is flattened as `exe:1:1503238553;txt:1:120`, and
  `tracker_hosts` is joined with `;`.
- Headers: `Content-Disposition: attachment; filename="putio-sync-releases-<range>.<ext>"`.

## `GET /settings` and `PUT /settings`: new `precheck` block

```json
"precheck": {
  "enabled": true, "dry_run": false, "min_size_mb": 50, "allow_archives": false,
  "magnet_lookup": true, "lookup_timeout_secs": 10,
  "caches": ["https://itorrents.net/torrent/{HASH}.torrent"],
  "categories": {"software": {"enabled": false}},
  "watch": {"/watch/tv": {"min_size_mb": 100}},
  "effective": [
    {"kind": "category", "key": "sonarr", "enabled": true, "min_size_mb": 50,
     "wanted": ["mkv", "mp4", "…"], "wanted_from": null},
    {"kind": "category", "key": "lidarr", "enabled": true, "min_size_mb": 50,
     "wanted": ["flac", "mp3", "…"], "wanted_from": "lidarr"}
  ]
}
```

- `effective` is read-only (FR-020).
- Its category list is the union of the configured `[[arr.servers]]` categories, the categories
  seen in `arr_items`, and the keys in `categories`.
- `wanted_from` names the 009 rule path the wanted types come from, or `null` for the video
  default.
- A save keeps the file's comments, as 009 does.
