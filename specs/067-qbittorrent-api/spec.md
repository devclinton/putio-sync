# Feature 067: qBittorrent API emulation

**Status**: Done

putio-sync answers as Transmission for Radarr and Sonarr (011). Some tools only speak
qBittorrent. This feature answers qBittorrent's Web API v2 on the same listener, with the
same `[arr]` password and the same items.

## User stories

1. **autobrr** (P1): I add putio-sync as a qBittorrent client, and a filter's action sends
   releases to Put.io.
2. **Radarr and Sonarr** (P1): I can add putio-sync as a qBittorrent client instead of a
   Transmission one, and grabs, the queue, import and removal work the same.
3. **Phone app** (P2): a qBittorrent remote shows the items and can add, pause, resume and
   remove.
4. **cross-seed** (P3): it can sign in and read the items. Injection adds a normal Put.io
   transfer; linking and rechecks are not possible.

## Requirements

- **FR-001** The API is at `/api/v2/` on the API listener. It answers 404 unless `[arr]` is
  enabled and the database is open, and 503 when no password is set.
- **FR-002** `auth/login` (POST, `username` and `password`) answers 200 `Ok.` with a `SID`
  cookie, or 200 `Fails.`. Every other path needs the cookie, else 403 `Forbidden`.
- **FR-003** Answered: `auth/logout`; `app/version` (`v4.6.7`), `app/webapiVersion`
  (`2.9.3`), `app/preferences`, `app/buildInfo`, `app/defaultSavePath`; `torrents/add`,
  `info`, `properties`, `files`, `trackers`, `webseeds`, `categories`, `createCategory`,
  `editCategory`, `removeCategories`, `setCategory`, `tags`, `pause`, `stop`, `resume`,
  `start`, `delete`; `sync/maindata`; `transfer/info`, `transfer/speedLimitsMode`.
- **FR-004** A torrent is an 011 item: a Put.io transfer plus its download. Both APIs list
  the same items. The category picks the folder, as in 011.
- **FR-005** `torrents/add` takes `urls` (one per line) and .torrent files as
  `multipart/form-data` (parts named `torrents` with a file name), up to 50 in all, with `category` and
  `savepath`. `savepath` must be inside the download folder (409 otherwise). Added or
  already there: 200 `Ok.`. Not a torrent: 415 `Fails.`. Nothing given: 400.
- **FR-006** The hash is the real v1 infohash. An add without one is refused; there is no
  fallback hash. `infohash_v2` is empty.
- **FR-007** States: failed → `error`; queued on Put.io → `queuedDL`; paused → `pausedDL`;
  on Put.io or downloading → `downloading`; files on disk but pipeline steps running →
  `downloading` at 0.99; done → `pausedUP`.
- **FR-008** Fixed answers for what Put.io has no value for: ratio 0, ratio limit 0,
  seeding time 0, seeding time limit -2, peers, seeds and leechers 0, uploaded 0,
  availability -1, tags empty, one tracker `https://put.io/` with status 2, ETA 8640000 when
  unknown. `completion_on` is the row's last change once done, else -1.
- **FR-009** `torrents/files` lists the files on disk (at most 5,000, 16 levels, links not
  followed); before any exist, one entry with the item's name and size.
- **FR-010** Pause and resume act on the local download (the queue's pause). Delete removes
  the item as 011 does; `deleteFiles` also deletes local files. `hashes=all` on delete is
  refused (400).
- **FR-011** `setCategory` to the item's own category is 200; another is 409. Categories
  made with `createCategory` are kept in memory (256 at most) and listed with the
  categories in use and those of the saved applications.
- **FR-012** Accepted and ignored: share limits, priorities, force start, reannounce,
  recheck, automatic management, speed limits, sequential and first/last piece, super
  seeding, and the tag calls. Any other path is 404.
- **FR-013** `sync/maindata` always sends a full update with `rid` + 1.

## Security

- **SR-001** The user name and password are compared in constant time, both always.
- **SR-002** Wrong sign-ins count against the Transmission RPC's limiter (5 per window per
  address). A throttled address gets 403 and `Retry-After`, with the right password too.
  A missing or stale cookie is not counted.
- **SR-003** The SID is 64 hex characters from two random UUIDs (244 random bits). The
  cookie is `HttpOnly; SameSite=Strict; Path=/api/v2; Max-Age=86400`, plus `Secure` behind
  an HTTPS proxy.
- **SR-004** A session ends after 1 hour unused, after 24 hours, on logout, on a restart
  (they are in memory only), and when the user name or password changes. At most 64 are
  kept; the oldest goes first.
- **SR-005** A change needs a POST (405 otherwise). A request whose `Origin` is another
  host is refused (403).
- **SR-006** The password and the SID are never logged or echoed. The listener's bind, host
  check and tailnet rules apply unchanged. Bodies are limited to 16 MiB for an add and
  1 MiB otherwise.

## Left out

- LunaSea: it has no torrent client module, so there is nothing to connect or test.
- Contract tests are **reconstructed from knowledge** of each tool's client, not captured
  from the tools. Radarr/Sonarr: fairly sure. autobrr: medium. cross-seed: medium to low.
  Phone apps (nzb360 is closed source): low.
- Incremental `sync/maindata`, `app/setPreferences`, rename, file priorities, move
  (`setLocation`), tags, RSS, search and the torrent creator.
- Linking and recheck for cross-seed: there are no local pieces to check or seed.
- The doctor has no line for the download client's address, so none was added.
