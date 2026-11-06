# Data Model: Media Server (DLNA)

## DlnaConfig (`[dlna]`, in `putio-sync-core`)

| Field | Type | Default | Rules |
|-------|------|---------|-------|
| `enabled` | bool | `false` | |
| `name` | string | `"putio-sync"` | Trimmed. Empty → default. Cut to 64 chars. |
| `port` | u16 | `8200` | 1–65535, not 1900, and not the API or metrics port. |
| `bind` | list of address/interface entries (037's `BindList`) | all LAN IPv4 | The same entry syntax as `[api] bind`. Bridges are left out of "all". |
| `roots` | list of root keys | `["downloads"]` | Each is `downloads`, `cat:<name>` (an existing category subfolder name, no `/`), `watch:<path>` (one of `[watch] paths`) or `putio`. At least one if enabled. No duplicates. `downloads` and `cat:*` both may be listed: a category is then shown twice (as a top-level root and inside Downloads). |
| `putio_folder` | i64 | none | Required when `roots` has `putio`. The folder ID on Put.io. |

Validation runs in `Config::validate`. Errors name the field, as other sections do.

## SharedRoot (runtime)

- `key`: the root key above. It is the first segment of an object ID.
- `title`: "Downloads", the category name, the watch path, or "Put.io".
- `source`: `Local(PathBuf)` or `Putio(folder_id)`.

It is built from `DlnaConfig` and the rest of `Config` on each `update`. A `cat:` or `watch:`
root whose folder is missing is shown as an empty container, and a WARN is logged once.

## ObjectId

`"0"` is the root. Otherwise it is `percent-encode(key) + "/" + percent-encode(rel)`, where
`rel` is a `/`-joined relative path for local roots or a Put.io file ID for `putio`. It is
decoded and checked on every request (research R4). It has no stored state.

## MediaItem (Browse result)

- **Container**: `id`, `parentID`, `title`, `childCount` (when cheap: local listing count;
  absent for Put.io).
- **Item**:
  - `id`, `parentID`, `title`;
  - `upnp:class` (video, audio or image);
  - one `res` with `protocolInfo`, `size` and the stream URL
    `http://<iface addr>:<port>/media/<ObjectId>`;
  - for Put.io: the size of the chosen version (MP4 or original).

Ordering: containers first, then items, each sorted by name, case-insensitive. Paging uses
`StartingIndex` and `RequestedCount` (0 = all, capped at 500). `TotalMatches` is the full
count.

## Stream (runtime, for metrics and logs)

`root_kind` (`local` or `putio`), `size`, `bytes_sent`, `started`, `ended`, `result` (`done`,
`client_closed` or `error`). It is logged at INFO on end. `streams_active` is the gauge of the
open ones.

## MediaServerStatus (API)

- `state`: `off`, `starting`, `running` or `failed`.
- `reason`: for `failed`, or a warning while running (no LAN interface).
- `addresses`: the URLs of `description.xml`.
- `streams_active`.
- `players_last_hour`: distinct requesting IPs, kept in memory only, never exported as a
  label.
- `putio`: `ok`, `off` or `error` with a reason.

## State transitions

```
off --enable--> starting --bound + announced--> running
starting --bind error--> failed(reason) --save/port free--> starting
running --disable--> (byebye) off
running --port/bind change--> (byebye) starting
running --name/roots change--> running (next NOTIFY carries new name)
```
