# Quickstart: check the log viewer (047)

## Automated

```bash
cargo test -p putio-sync-core log_
cargo test -p putio-sync-api --test logs
cargo test -p putio-sync-api --test ui_assets      # 384 KB cap
TMPDIR=$PWD/.scratch CARGO_PROFILE_RELEASE_STRIP=none make ci
```

## Manual, against a local daemon

1. Use a scratch config with a token and `[api] auth_token`, then start `putio-sync server`
   (or `daemon`) on a free port.
2. Sign in to the UI and open **Logs**. Startup lines are shown, newest last.
3. Set level filter `warn`: only WARN and ERROR stay. Type `config` in search: matches only.
   Reload: the filters are still in the address.
4. Live tail: run a sync from Status. Its lines appear within 2 s. Press Pause and run
   another; the waiting count goes up. Press Resume: they are added in order.
5. Level: set `putio_sync_core` to debug for 30 minutes. Debug lines from it appear, other
   modules stay at info, and the controls show the end time. Press Revert: back to base.
6. Run the doctor with an override on: the "Log level" check warns. After the revert it
   passes.
7. Copy and Download: the text matches the shown lines.
8. Secrets: search for the API token or Put.io token value. Nothing matches, and no
   `Authorization` value appears at trace.
9. `curl` `/logs` without a token: 401.
