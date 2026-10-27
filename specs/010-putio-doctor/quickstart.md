# Quickstart: validating the Put.io Doctor

Prerequisites: a Rust toolchain. No Put.io account is needed.

## 1. Automated gate

```bash
TMPDIR=<your scratch dir> make ci
```

The gate passes, including:

- `putio-sync-doctor` unit tests: one seeded fault per check, plus each fix's preview, apply
  and skip path;
- `putio-sync-api/tests/doctor.rs`: auth required, run, single flight, preview and apply,
  destructive without confirm gives 400, empty-folder list changed gives 409;
- the CLI exit-code and `--json` tests;
- the UI asset budget test (under 150 KB).

## 2. Web UI against the mock backend

```bash
cargo run -p putio-sync-api --example mock_ui -- 8899
```

Open `http://127.0.0.1:8899/`, sign in with the printed token, and go to **Doctor**. The page
should show:

- a summary with at least one problem and several warnings, from the faults the mock seeds:
  - a failed transfer and a stalled transfer;
  - a dead queue row;
  - an orphan partial file;
  - a duplicate pair;
  - an orphaned `.torrent`;
  - storage at 93%;
  - a config with an unknown key;
- **Requeue** on failing downloads, which works at once and updates the check;
- **Remove** on dead queue rows, which opens a confirmation listing the row. Cancelling
  changes nothing; confirming removes the row and the check turns OK;
- **Clean up** on empty folders, which shows the dry-run list before confirming;
- **Run now**, which refreshes the "ran at" time.

## 3. CLI

Point the CLI at a temporary config whose Put.io base URL is a mock, or use the CLI tests:

```bash
cargo test -p putio-sync-cli doctor
```

`putio-sync doctor --json | jq .overall` prints a status. The exit code follows the
[CLI contract](contracts/cli.md).

## 4. Metrics

In the daemon tests (`cargo test -p putio-sync-daemon doctor`), a scheduled run sets
`putio_sync_doctor_check_status{check="dead_queue_rows"} 1`.
