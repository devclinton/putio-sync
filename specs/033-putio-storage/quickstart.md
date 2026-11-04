# Quickstart: Put.io Storage Management

All checks run against mocks; never against a real Put.io account.

## Automated

```bash
cargo test -p putio-sync-storage
cargo test -p putio-sync-api --test storage
cargo test -p putio-sync-db storage
cargo test -p putio-sync-doctor storage
cargo test -p putio-sync-api --test ui_assets
TMPDIR=<scratchpad> make ci
```

Expected: the planner picks only verified downloads, oldest first, stopping at the target;
free deletes only plan files and records the run and History rows; stale plans give 409;
setup mode gives 503; the bundle is under 384 KB.

## Manual (mocked backend)

1. Start the API against a mock Put.io (as the other UI checks do) with an account 95%
   full and a few completed downloads whose local files exist.
2. Open `#/putio`: the storage card shows used/free/total, the top items and a low-space
   warning. `#/` (Status) shows the notice.
3. Click **Free up space**: the preview lists the oldest downloaded files; cancel → no
   delete request reaches the mock.
4. Click again and confirm: the mock receives `/files/delete` with exactly the listed ids;
   the card's recent runs shows the run; History shows the files as deleted.
5. Settings → Storage: change the levels; a target at or below the warning level is
   refused.

See [contracts/storage-api.md](contracts/storage-api.md) for bodies.
