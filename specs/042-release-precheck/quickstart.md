# Quickstart: validating Release Pre-check (042)

## Automated (no network, no Put.io: Constitution III)

```bash
make ci
```

The new tests to look for:

- `putio-sync-precheck/tests/metainfo.rs`: synthetic .torrents (single file, multi-file, v2
  `file tree`, hybrid, padding files). Checks the file lists and sizes, and that the info hash
  matches 011's.
- `putio-sync-precheck/tests/rule.rs`:
  - an `.exe` only, a `.7z` only, `.r00` parts, a 12 MB `.mkv`, a 2 GB `.mkv` + `.exe`, a season
    pack, `Episode.mkv.exe`;
  - with the video default, a 009 "audio" rule, and `allow_archives`.
  - Expected: only the real releases pass (SC-006).
- `putio-sync-precheck/tests/lookup.rs`, with mockito caches: found, 404, HTML body, gzip body,
  wrong hash, a slow cache past the time limit, and lookup off (no request made). Also a URL that
  302s to `magnet:`.
- `putio-sync-arr/tests/service.rs`, new cases:
  - a fake .torrent: no `transfers/add` call, the item is failed and blocklisted once;
  - a fake magnet found in a cache: the same;
  - a magnet missed by the cache: a transfer is created and a pending check is written;
  - that transfer completing with only a `.7z`: it is cancelled and deleted, no file is queued,
    and it is blocklisted.
- `putio-sync-file-manager/tests/watch_precheck.rs`: the fake is renamed `.rejected` and not
  uploaded, and the real one is uploaded.
- `putio-sync-download` test: a pending check stops the sync from queueing that transfer's files.
- `putio-sync-api/tests/releases.rs`:
  - flag, undo, and fail-and-blocklist through the router;
  - the CSV/JSON export;
  - `/stats/releases` totals;
  - `tests/secrets_sweep.rs` extended: no indexer URL or magnet appears in a response, report or
    log.

## UI against the mock backend

```bash
cargo run -p putio-sync-api --example mock_ui
```

1. Stats: the "Rejected releases" and "Magnet lookups" tiles show the seeded week, and the
   by-stage, by-reason and per-cache splits add up.
2. Queue: a seeded rejected *arr item shows its reason. Flag it "Not fake", then undo.
3. Put.io page: flag a folder "Fake" with a note. It shows "Flagged", and the export lists it with
   its types.
4. Settings, Release check card: the `lidarr` category shows audio types "from rule lidarr".
   Change the minimum size and save, and the config file keeps its comments.

## On the NAS (manual, once deployed)

5. Copy one of the real fakes' .torrent files (the 1.4 GB `.exe`/`.scr`/`.7z` "episodes") into
   the watch folder. It becomes `.torrent.rejected` within one scan. Put.io shows no new transfer.
6. Add a magnet that itorrents.net doesn't know (any fresh release) from Sonarr. Then watch it:

   ```bash
   curl -s -H "Authorization: Bearer $PUTIO_TOKEN" https://api.put.io/v2/transfers/list
   ```

   Note whether `file_id` is ever set while `status` is `DOWNLOADING` (research R3). Write the
   answer into research.md R3.
7. After a week, compare the Stats page's "bytes avoided" with the earlier figure of about 60 GB
   (SC-005), and read the fallback share (SC-007).
