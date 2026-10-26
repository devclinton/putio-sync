# Quickstart: Validate Empty-Folder Cleanup

No real Put.io account is used anywhere below.

## Automated

```bash
TMPDIR=<scratch> make ci
```

Key tests:

- `cargo test -p putio-sync-client cleanup` — walk/decide/delete against `mockito`
  (nested empties, files, protected, shared, too new, unknown age, transfer targets,
  list/delete errors, `cursor`, dry run == real run).
- `cargo test -p putio-sync-daemon schedule` — `is_due` (crossing, no catch-up, midnight,
  DST gap/overlap, disabled).
- `cargo test -p putio-sync-core cleanup config` — defaults, validation, guard, metrics.
- `cargo test -p putio-sync-api --test cleanup` — 401, 400 without `dry_run`, dry run,
  real run, 409 while running, settings GET/PUT keeps comments.
- `cargo test -p putio-sync-telemetry` — new families in the contract test.

## Manual (mock backend)

```bash
cargo run -p putio-sync-api --example mock_ui
```

Open the printed URL, go to **Put.io**, click **Remove empty folders**: the dialog shows the
mocked count; Cancel deletes nothing; Delete shows the result toast. On **Settings**, change
the cleanup run time and save.

## CLI (mock)

`files clean-empty` is covered by the client tests; to try it by hand, point a config at a
mock Put.io server (`PutioApiClient::with_base_url` is used in tests only), so the manual
check is the web UI above.
