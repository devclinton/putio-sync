# Contract: API and metrics changes (043)

Everything below is under `/api/v1`, behind the bearer token. No response contains a magnet.

## Transmission RPC (`/transmission/rpc`)

`torrent-add` of a magnet that misses the first ask answers as any add:

```json
{"result": "success",
 "arguments": {"torrent-added": {"id": 42, "name": "Show.S01E04.1080p", "hashString": "…"}}}
```

While held, `torrent-get` lists it with `status: 3` (queued for download), `percentDone: 0`,
`error: 0`, `errorString: ""`, `eta: -1`, `isFinished: false`.

After the hold it reports as any other item: a Put.io transfer, or the 042 reject
(`status: 0`, `error: 3`, `errorString: "rejected before Put.io: …"`).

## `GET /arr`: new `held` list (the Queue page's source)

`GET /arr` has counts, not items, so the held magnets are a list of their own, the oldest first,
empty when none are held. Never the magnet:

```json
"held": [{"hash": "…", "name": "Show.S01E04.1080p", "since": "2026-10-28T10:00:00Z",
          "until": "2026-10-28T10:05:00Z", "attempts": 4, "last_lookup": "timeout"}]
```

## `GET /stats/releases`: new `held` block

```json
"held": {"now": 2, "found": 15, "ran_out": 1, "removed": 0,
         "median_found_secs": 150}
```

`found` counts holds a later ask found, passed or rejected. `median_found_secs` is `null` with
none found. Ends with precheck turned off (`unchecked`) count in the metric only.

## `GET /settings/precheck` and `PUT /settings/precheck`: three new keys

```json
{ "…": "…", "hold_magnet_secs": 300, "hold_retry_secs": 30, "first_lookup_secs": 3 }
```

Validation errors use 042's format, for example
`precheck.first_lookup_secs must be between 1 and lookup_timeout_secs (10)`.

## Metrics

| Name | Type | Labels |
|---|---|---|
| `putio_sync_precheck_held_total` | counter | `result` = `found_passed`, `found_rejected`, `ran_out`, `removed`, `unchecked` |
| `putio_sync_precheck_held` | gauge | none |
| `putio_sync_precheck_held_seconds` | histogram | `result` (same values); buckets 30, 60, 120, 180, 300, 600 |

042's cache lookup metrics are unchanged: a re-ask counts as any lookup. The planned `ask`
label was dropped; `putio_sync_precheck_held_total` already tells first asks from re-asks.
