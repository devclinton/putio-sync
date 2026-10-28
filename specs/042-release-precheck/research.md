# Research: Release Pre-check (042)

Each entry gives the decision, why, and what else was considered. Sources were checked on
2026-10-28.

## R1. Where the code lives

**Decision**: a new crate, `putio-sync-precheck`. It owns:

- reading .torrent files: the info hash and the bencode reader, moved from
  `putio-sync-arr/src/infohash.rs`, plus the file list, `private` flag and tracker hosts;
- the rule: given a file list and a policy, pass or reject with a reason;
- getting the file list for a source: local bytes, a URL fetch, or a cache lookup for a magnet.

`putio-sync-arr` (Transmission adds) and `putio-sync-file-manager` (watch folder) both call it.

**Why**: two crates need it, and they can't depend on each other. `file-manager` depending on
`arr` would pull in `download` and the whole *arr service. Constitution I asks for one crate per
concern. `core` holds config and shared types, not network behaviour.

**Alternatives**:
- Put it all in `core`: it has `reqwest`, but a cache lookup and a URL fetch are behaviour, not
  shared utils.
- Keep it in `arr` and copy the parser into `file-manager`: duplicated bencode code.

## R2. Reading the file list from a .torrent

**Decision**: extend the existing hand-written bencode reader (`read_string`, `skip_value`) to
walk the `info` dictionary:

- single-file torrents: `name` + `length`;
- multi-file torrents: `files[]`, each with `length` and `path[]`. Its path is joined with `/`
  under `name`;
- BitTorrent v2-only torrents (`file tree`, no `files`): read `file tree` the same way. Hybrid
  torrents have both, and `files` is used;
- padding files (`attr` contains `p`, or a path starting `.pad`) are ignored.

The reader keeps its depth limit and refuses more than 100,000 files. Tracker hosts come from
`announce` and `announce-list`, and only host names are kept (as in 006).

**Why**: no new dependency (Constitution VII). The info-hash code already finds the `info` span,
so the file walk is about 80 lines.

**Alternatives**: the `lava_torrent` or `bendy` crates. Both are fine, but they add a dependency
for something the repo already half does.

## R3. Does Put.io show a transfer's files before it completes?

**Decision**: no; plan for it showing files only at completion.

- The transfer object has no file list. Its fields (go-putio `Transfer`) are `id`, `name`,
  `size`, `status`, `percent_done`, `downloaded`, `file_id`, `magneturi`, `torrent_link`,
  `source`, `type`, `is_private` and the speed/peer fields.
- `file_id` is null until Put.io has created the destination, which putioarr treats as "ready".
- No endpoint returns torrent metadata for a transfer in progress.

The check on Put.io (FR-015) therefore runs when the transfer is `COMPLETED`/`SEEDING` with a
`file_id`: it lists the tree read-only, runs the rule, and only then queues anything.

**Consequence**: for a magnet that falls back, Put.io's traffic is spent. The NAS's isn't. The
telemetry in FR-022 measures how much traffic this costs (bytes Put.io downloaded for fallbacks
that turned out fake), which answers the user's "do we need to fetch more often" question.

**To confirm live** (quickstart step 6): whether `file_id` is ever set while `DOWNLOADING`. If it
is, the same code checks earlier with no change, because it triggers on `file_id`, not on status.

**Sources**: github.com/putdotio/go-putio `types.go` and `transfers.go`; putioarr 0.5.2
`src/services/putio.rs`. The official API page (SwaggerHub) was not readable.

## R4. Cancelling and cleaning up on Put.io

**Decision**: when the check on Put.io rejects a transfer:

1. `POST /transfers/cancel` (`transfer_ids`), which is what 011 already does on removal;
2. if a `file_id` exists, delete it with `files/delete`;
3. remove the transfer from the list.

Delete failures are logged and retried at the next pass. This reuses the client calls 011 added.

**Why**: what a cancel leaves behind isn't documented. Doing both steps works either way.

## R5. Public .torrent caches

**Decision**: the cache list is a config list of URL templates, with `{HASH}` (upper-case hex)
or `{hash}` (lower-case). The default holds one entry:
`https://itorrents.net/torrent/{HASH}.torrent`.

Probe results (2026-10-28):

| Cache | Result |
|---|---|
| itorrents.net | Alive. `application/x-bittorrent`, no challenge, either case of hex. |
| itorrents.org | 301 to itorrents.net, so not needed separately. |
| torrage.info, btcache.me | Return HTML pages, not .torrent files. Unusable. |
| zoink.ch | 404. |
| thetorrent.org, torrent.cd | No DNS. Dead. |
| btdig | A search engine, not a cache. |

How each lookup works:

- caches are tried in order, within a total time limit (default 10 s);
- each response is capped at 2 MB;
- it is accepted only if it parses and its info hash equals the magnet's (FR-009). A wrong hash
  counts as `wrong_hash`;
- requests send `Accept-Encoding: identity` and a plain `User-Agent`. A body starting with the
  gzip magic number or `<` (HTML) is counted as `error` and skipped.

**Why**:
- The time limit keeps the add under the *arr's timeout (SC-003).
- Asking for identity avoids turning on reqwest's `gzip` feature, which would add a dependency.
- With one live cache, the hit rate is unknown. FR-022's per-cache counters measure it before
  anyone adds more caches or other methods.

**Privacy**: a lookup sends only the hash in the path, with no cookies and no referer (FR-010).
It can be turned off.

## R6. URLs that redirect to a magnet link

**Decision**: the URL fetch uses a reqwest client with a custom redirect policy.

- It follows at most 5 http(s) hops.
- It stops when the next URL's scheme is `magnet`. The fetch then reads the 3xx response's
  `Location` header and returns a magnet, which is checked as in R5.

The fetch moves from `ArrService::fetch_torrent` into the precheck crate, keeping its 10 MB cap
and its rule that the URL is never logged.

**Time budget** (SC-003): each add has one 14 s deadline, set when the request arrives. That
leaves a second for the reply. The URL fetch gets the whole deadline, which replaces 011's 30 s
timeout. A magnet that comes from a redirect is looked up with whatever time is left, capped at
`lookup_timeout_secs`. When less than 1 s is left, the lookup is skipped with result `timeout`,
and the magnet goes to Put.io unchecked. A URL fetch that runs past the deadline fails as today
("invalid torrent").

**Why**: by default reqwest gives a redirect error for a non-http scheme, so today these adds
fail with "invalid torrent". Prowlarr and Jackett commonly answer this way for magnet-only
indexers.

## R7. Reporting a rejected *arr item

**Decision**: `torrent-add` answers `torrent-added`, with the id, name and hash, as for any add.
The item is inserted into `arr_items` with:

- `state = failed`;
- `transfer_id = NULL`;
- `error = "rejected before Put.io: …"`;
- `blocklist = pending` when a server is configured for the category.

The next refresh's `process_blocklist` does the rest, unchanged.

**Why**: in Radarr's `ProcessDownloadDecisions`, a `TransmissionException` from the add is a
generic error, so the release is skipped for that run. It is not blocklisted, and the next
RSS/search can grab it again. Only failures after the grab, seen on queue import, are
blocklisted. So refusing the add would loop on the same fake.

**Effects in the 011 code**:
- `refresh` must skip items with no transfer;
- `remove` must skip the Put.io calls when `transfer_id` is NULL. Remove already handles a
  missing file id, and the transfer id must be handled the same way;
- the 011 data model note "transfer_id is NULL only if the add failed" changes to "NULL for
  rejected items".

## R8. Unchecked magnets from the watch folder vs. the regular sync

**Problem**: an unchecked watch-folder transfer completes on Put.io, and the regular sync may
queue its files before the check on Put.io runs.

**Decision**:

- Every fallback writes a `release_checks` row with `state = pending` and its `transfer_id`
  (data-model.md).
- In `DownloadManager`'s tree enqueue, if any check is pending (one cheap `EXISTS` query per
  pass), it loads the transfers first. Files whose matching transfer has a pending check are
  skipped, with the decision `Known`: not queued, and no rule decision recorded.
- The check on Put.io runs:
  - for watch items, at the start of each watch scan (a timer, as for any watch folder);
  - for *arr items, in the 011 refresh, before `move_to_local`.
- It sets the row to `passed` or `rejected`, and the next sync then queues or ignores the files.

*arr items get the same rows, so the regular sync can't race them either.

**Why**: it reuses the transfer matching already done in that loop (`matching_transfer`). The
cost is one indexed query per sync when nothing is pending.

**Alternatives**: hold watch-folder transfers in a separate Put.io staging folder and move them
after the check. This needs a folder move per item and confuses 009 folder rules.

## R9. The policy: what counts as wanted

**Decision**: the policy is resolved per check, from `RuleSet::resolve(<put.io folder path>)`.

- Wanted types:
  - when the folder's filter is an allow list (`filter.only` is `Some`), its allowed types minus
    its blocked ones;
  - otherwise the built-in `video` preset list (`rules::builtin("video")`).
- When `allow_archives` is on, `7z`, `zip`, `rar` and the split-part patterns are added.
- Minimum size: from `[precheck]`, overridden per category or watch path.

**Archive detection**: by last extension. The list is `7z, zip, rar, tar, gz, bz2, xz, zst, cab,
iso, img`, plus split parts: `\.r\d\d$`, `\.\d{3}$` and `\.part\d+\.rar$`. Executables are
`blocked_extensions` plus `scr, pif, com`.

These lists only make the reason clearer ("found .exe, .7z"). Pass or fail depends only on
whether a wanted file meets the minimum size.

**Why**: this follows the spec's clarification, and `RuleSet` already answers "what does this
folder allow". With no 009 rule, the default is video.

## R10. Where the rejection data and fallback data are stored

**Decision**: two tables in `putio-sync-db`:

- `release_checks`: one row per check, small. It feeds the fallback stats and the sync gate (R8).
- `release_reports`: one row per rejection or flag, with the traits from FR-025. The file-type
  summary is a JSON text column.

Rows in both are pruned after 400 days (FR-027 asks for at least 1 year). The repo has no
retention job yet, so the daemon prunes at startup and then once a day, with one `DELETE … WHERE
checked_at < ?` per table.

**Why**: Stats needs fallback rates (R5, FR-022) across restarts, which Prometheus counters alone
can't give the UI. Reports are a separate table because flags exist without a check (a plain-sync
release flagged by hand).

## R11. Flagging from the UI

**Decision**: a flag targets one of three things:

- an `arr_items` hash;
- a Put.io `file_id`, from the Put.io page;
- a `history` row, which resolves through `download_sources.transfer_id`.

The server gathers the traits:

- the transfer from `transfers/list`: name, magnet hash, tracker host, private flag;
- the file tree from a read-only walk of the transfer's `file_id`, or from the local history rows
  when the transfer is gone.

Missing fields stay empty (spec assumption). "Fail and blocklist" applies only to *arr items. It
calls the same `fail` path as a rejection on Put.io, then deletes the item's Put.io files. Undo
deletes the flag's report row. It does not undo a blocklist (the UI says so).

**Why**: the operator clicks a row they can see. Working out the transfer and traits is the
server's job, so the UI sends only the target.

## R12. Metrics

**Decision**: the new families, all in `putio-sync-telemetry/src/precheck.rs`:

| Metric | Type | Labels |
|---|---|---|
| `putio_sync_precheck_total` | counter | `source` = torrent/url/magnet/watch_torrent/watch_magnet, `stage` = before/putio, `outcome` = passed/rejected/unknown |
| `putio_sync_precheck_avoided_bytes_total` | counter | `stage` |
| `putio_sync_precheck_cache_lookups_total` | counter | `cache` = host name from config, `result` = found/not_found/timeout/wrong_hash/error |
| `putio_sync_precheck_cache_lookup_seconds` | histogram | none |
| `putio_sync_precheck_fallback_total` | counter | `result` = passed/rejected |
| `putio_sync_precheck_fallback_putio_bytes_total` | counter | none; bytes Put.io downloaded for fallbacks that were rejected |
| `putio_sync_release_flags_total` | counter | `kind` = fake/not_fake |

Every label set is bounded. The `cache` label is bounded by the config list. No names or hashes
appear in labels (Constitution V, FR-017).
