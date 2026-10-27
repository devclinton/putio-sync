# Research: Radarr/Sonarr Download Client (011)

Sources:

- the Sonarr `develop` branch's Transmission client, `src/NzbDrone.Core/Download/Clients/Transmission/`
  (`TransmissionBase.cs`, `TransmissionProxy.cs`, `Transmission.cs`, `TransmissionTorrent.cs`,
  `TransmissionConfig.cs`), read on 2026-10-26. Radarr shares this code;
- the `putioarr` crate (a Transmission proxy for Put.io);
- the NAS layout (read-only look on 2026-10-26).

## R1. Which protocol to emulate

**Decision**: Transmission RPC. The user chose it (option A).

**Rationale**: Transmission has the smallest surface of any client the *arr apps support.
Requests are JSON over one URL: `session-get`, `torrent-add`, `torrent-get`, `torrent-remove`,
`torrent-set` and `queue-move-top`. Authentication is HTTP Basic plus a session-id handshake.
`putioarr` proves the approach works.

**Alternatives**:

- qBittorrent Web API: cookie login and about 10 endpoints.
- SABnzbd: the wrong protocol for torrents.
- Blackhole plus *arr API calls: no progress reporting and no failure path.

## R2. What the *arr client actually does

These are the facts from the client source that shape the design:

- **Handshake**: it first POSTs to `{urlBase}rpc` with Basic credentials and no session header.
  The reply it expects:
  - 409 with `X-Transmission-Session-Id` means success;
  - 401 means bad credentials;
  - 403 is treated as an IP whitelist failure;
  - any other status is an authentication failure.

  It caches the session id. It handles a 409 on a later request by redoing the handshake once.
- **Responses**: every response is JSON `{"result": "...", "arguments": {...}}`. Any `result`
  other than `"success"` is raised as an error, and its text is shown to the user.
- **Version**: the version comes from `session-get` → `version`. A version below 2.40 is
  rejected. Version 4.0 and up switches the client to labels (`SupportsLabels`).
- **Item identity**: the *arr computes the info hash itself before adding the torrent. It uses
  `HashString.ToUpper()` as the DownloadId, so `torrent-get` must return the real info hash.
- **Status mapping** (`GetItems`), checked in order:
  1. a non-empty `errorString` → **Warning**, with the message shown;
  2. `totalSize == 0` → Queued;
  3. `leftUntilDone == 0` and status Stopped(0)/Seeding(6)/SeedWait(5) → Completed;
  4. `isFinished` and status not Check/CheckWait → Completed;
  5. status 3 → Queued;
  6. otherwise → Downloading.

  **No state maps to Failed**, so a torrent client cannot make the *arr blocklist on its own.
- **Output path**: `downloadDir` + `name` (`:` becomes `_`). Items are filtered as follows:
  - with a Category and no labels, an item is kept only if one of its `downloadDir` path
    segments equals the category;
  - with a Directory, an item is kept only if its folder is inside that directory.
- **Download directory**: with a Category and version < 4.0, the add request sends
  `download-dir = session download-dir + "/" + category`.
- **Removal after import** (`CanBeRemoved`): this needs `RemoveCompletedDownloads` and a
  reached seed limit. One sufficient path is `seedIdleMode == 0`, status Stopped, and session
  `idle-seeding-limit-enabled == true`. On removal the *arr calls
  `torrent-remove {ids:[hash], delete-local-data:true}`.
- **Add requests**:
  - a magnet or URL is sent as `torrent-add {filename}`;
  - a .torrent file is sent as `torrent-add {metainfo: base64}`;
  - either may carry `paused`, `download-dir` and `labels`.

  The *arr then calls `torrent-set` (seed limits) and possibly `queue-move-top`; both are
  ignored.

**Decision**: report version `3.00 (putio-sync)` and rpc-version 17. A version below 4.0 makes
each category a subfolder (no labels), which maps directly onto Put.io folders.
`seedRatioMode`, `seedIdleMode` and `seedRatioLimited` are reported as 0, 0 and false.
`idle-seeding-limit-enabled` is reported as true, and a finished item has status 0 (Stopped),
so the *arr may remove it after import.

## R3. Failures and blocklisting

**Decision**: report failures through `errorString`, which the *arr shows as a Warning.
Optionally call the *arr's own API:

1. `GET /api/v3/queue/details` (Radarr and Sonarr both have it) to find the record whose
   `downloadId` equals the hash, ignoring case;
2. `DELETE /api/v3/queue/{id}?removeFromClient=true&blocklist=true` with `X-Api-Key`.

The *arr then publishes a failed-download event, and re-searches when "Redownload failed" is on.
It also calls our `torrent-remove`, which cleans up. The connection test uses
`GET /api/v3/system/status` (`appName`, `version`).

**Rationale**: R2 shows the protocol has no Failed state. The API call is the only automatic way
to blocklist.

**Alternatives**:

- Sonarr's indexer setting "fail downloads with dangerous files" acts at import, on files that
  exist on disk. putio-sync never downloads blocked extensions, so the *arr would see an empty
  folder and only warn.
- Webhooks: out of scope.

## R4. Info hash

**Decision**:

- **Magnet**: read the `xt=urn:btih:` value. Accept 40 hex characters as is (lowercased).
  Decode 32 base32 characters to hex.
- **.torrent**: find the byte span of the top-level `info` value with a small bencode scanner,
  then take SHA-1 of that span.
- A `filename` that is an http(s) URL to a .torrent: fetch it (size-capped at 10 MB), then
  treat it as metainfo.

The scanner (about 60 lines) and the base32 decoder (about 15 lines) are written in-crate.

**New dependency**: `sha1 = "0.10"`. It is from the RustCrypto family the workspace already uses
(`sha2 = "0.10"`), and no SHA-1 is available today. A bencode crate is not needed for one span
lookup.

**Rationale**: the hash is the item's identity in the *arr (R2), so it must be byte-exact.

## R5. Where items go on Put.io and on disk

**Decision**:

- The *arr's `download-dir` must be the reported download folder or inside it. The reported
  folder is `download.target_directory` as putio-sync sees it.
- The relative part (`sonarr`, or a nested `movies/4k`) names a folder path under the Put.io
  root, created segment by segment if missing. The transfer is saved there.
- The regular sync mirrors the Put.io tree from the root under the target directory, so
  the files land in `<target>/<relative part>/<name>`.
- An empty relative part means the Put.io root.

**Rationale**: no new path settings are needed. It matches feature 009, which keys folder rules
by a path that is both the Put.io path and the local path. A rule on `sonarr` therefore applies
to *arr items naturally.

**Alternative rejected**: put category folders under `watch.parent_folder_id`. That needs the
folder's path from the root (extra calls), and it ties two unrelated features together.

**Path mapping**: the reported folder is always putio-sync's own path. Where the *arr containers
mount it elsewhere, the *arr's Remote Path Mappings cover it. On the NAS all three containers use
`/downloads`.

## R6. When an item counts as finished

**Decision**: the Put.io transfer reaches COMPLETED or SEEDING with a `file_id`. putio-sync then
queues that subtree itself, through the same filters as the sync (`enqueue_tree(file_id)`
extended to report each file's decision). It records the files it accepted as the item's
files. An item is finished when every one of those files has a `downloads` row with status
`completed`.

- If the subtree has no accepted files, the item fails. The message names the blocked
  extensions when files were blocked, or says "no files" otherwise.
- Queuing the subtree when the item completes, instead of waiting for the next sync, also
  shortens the time to import.

**Rationale**: SC-003 says never report finished early. Recording the item's files when it
completes avoids a race with the sync loop.

**009 interaction**: after 009 merges, the per-file decision comes from the effective folder
rule (blocked, skipped). The item-files logic only reads "accepted / blocked + why", so the
rebase changes the decision source, not this feature's logic.

## R7. When the state is refreshed

**Decision**: refresh lazily. A `torrent-get` that finds the last refresh older than 20 s runs
one refresh, single-flight behind a mutex. A refresh:

1. lists Put.io transfers once;
2. updates each tracked item's Put.io state;
3. queues subtrees of items that have just completed;
4. reads local state from the DB;
5. fires pending blocklist calls.

Otherwise `torrent-get` answers from the DB.

**Rationale**: the *arr apps poll the client about once a minute, so there is no need for a
daemon loop or a new interval setting. This costs one Put.io call per minute while the *arr
apps run. It works the same under `daemon` and `server`.

**Alternative**: a daemon loop. Rejected, because it would add a task and a setting (VII).

## R8. Removing an item

**Decision**:

1. Cancel the Put.io transfer. `/transfers/cancel` removes it from the list whatever its
   state, and never deletes files.
2. Delete the item's Put.io file or folder (`file_id`), when it exists.
3. Remove queue rows for the item's files that are pending or paused, and cancel any of them
   that are downloading.
4. When `delete-local-data` is set, remove `<target>/<relative>/<output_name>`.
   - The path is built only from stored, validated parts: no `..`, no separators in the name,
     and it must stay inside the target directory.
   - It never removes the category folder itself.
5. Without `delete-local-data`, delete on Put.io only if the item is finished (FR-017).
   Otherwise cancel the transfer and leave any Put.io files to the regular sync.
6. Drop the item's rows.

A Put.io error is logged and does not keep the item (FR-018).

**Constitution IV**: deleting Put.io data before a local copy exists happens only on an explicit
remove from the *arr. It is the user's cancel, not a post-download delete. See the plan's
Complexity Tracking.

## R9. Authentication and secrets

**Decision**:

- `[arr] username` (default `putio-sync`) and `password`. The password is required for the
  RPC to answer at all, and can come from `PUTIO_SYNC_ARR_PASSWORD`.
- The comparison runs in constant time.
- Failures count against the existing `AuthLimiter`, so a guessing client is throttled like the
  UI login.
- The bearer-token middleware does not cover `/transmission/rpc`; it has its own check.
- *arr API keys live in `[[arr.servers]] api_key`. `PUTIO_SYNC_SONARR_API_KEY` and
  `PUTIO_SYNC_RADARR_API_KEY` override the first server of that kind.
- `Debug` impls print `<set>`/`<unset>`.
- `save_keeping_file_credentials` keeps `[arr] password` and every `api_key` exactly as on
  disk, the same treatment `[auth]` gets.

## R10. Session id

**Decision**: a random 32-character id is created when the process starts and kept in memory.
A request without it, or with a stale one, gets 409 with the header (after Basic auth passes).

## R11. Last contact

**Decision**: keep an in-memory map from the first token of the `User-Agent` (`Sonarr`,
`Radarr`, …) to the last time it made an authenticated request. Settings shows this map.

## R12. Metrics

**Decision**:

- `putio_sync_arr_rpc_requests_total{method,outcome}`: `method` comes from a fixed set
  (`session_get`, `torrent_add`, `torrent_get`, `torrent_remove`, `torrent_set`, `other`), and
  `outcome` is `ok`, `error` or `auth_failed`.
- `putio_sync_arr_items{state}`: `state` is `putio`, `local`, `done` or `failed`.
- `putio_sync_arr_blocklist_total{outcome}`: `outcome` is `ok`, `not_found` or `error`.

All label sets are bounded.
