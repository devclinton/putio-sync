# Quickstart: Checking the Processing Pipeline

See [data-model.md](data-model.md) and [contracts/api.md](contracts/api.md).

## 1. Code checks

```bash
make ci
```

The `putio-sync-pipeline` tests must pass:
- stage order;
- restart;
- hook input and exit codes (including skip code 10);
- required holds and optional give-up;
- staging (one rename; cross-filesystem refused).

The `arr` view test must pass too: 99% while a file-changing step is due.

## 2. Local run with a slow hook

Add a post-processing hook `["/bin/sh", "-c", "sleep 20; touch \"$PUTIO_SYNC_ITEM_DIR/.tidied\""]`.
Download a small item, then:
- The Active row shows "Post-processing: <name>" for about 20 s.
- The timeline shows Downloading done, then the hook running, then done with about a 20 s
  duration.
- The Transmission view (`torrent-get` through the *arr endpoint) shows `percentDone` < 1
  until the hook ends, then 1.

## 3. Failure paths

- Change the hook to `exit 3` with `required = true`: the item shows "Needs you". Retry after
  fixing the hook: the item completes. Skip: the item completes, and the skip shows on the
  timeline.
- With `required = false` and `optional_retries = [1, 1, 1]`: after 3 failures the item
  completes "with warnings" and a `pipeline.completed` event lists the step.

## 4. Watch-folder staging

Drop a `.torrent` in a watch folder with a post-processing hook configured. Until the hook is
done, `ls <category dir>` never shows the item; then it appears whole.

## 5. Restart

Kill the daemon while the slow hook runs, then start it again. The step shows interrupted,
then runs again, and the Downloading step does not run again.

## 6. Done when

- Steps 1–5 pass.
- With no steps configured, Radarr imports as fast as before (one item, timed).
- `CHANGELOG.md`, EVENTS.md and `config.example.toml` are updated.
