# CLI: `putio-sync doctor`

```text
putio-sync [--config PATH] doctor [--json] [--fix FIX_ID [--dry-run] [--yes]]
```

The command works without the daemon. It reads the config and the database, and calls Put.io
read-only.

## Report mode (no `--fix`)

Human output is grouped by area, one line per check, with items indented underneath (at most
10 per check, then "… and N more"):

```text
Put.io
  ✓ Authentication     Token accepted for account "alice".
  ! Storage            Using 912 GB of 1 TB (91%).
                       → Delete or clean up finished downloads on Put.io.
  ✗ Transfers          2 transfers failed.
                       - Some.Linux.ISO (error: tracker unreachable)  [fix: transfers.retry]
...
Summary: 1 problem, 1 warning, 14 ok, 1 skipped (took 3.2s)
Fixes available: transfers.retry, dead_queue_rows.remove — run `putio-sync doctor --fix <id>`
```

`--json` prints exactly one `Report` JSON document to stdout. Progress and log lines go to
stderr only.

Exit codes:

| Code | Meaning |
|------|---------|
| 0 | Overall ok |
| 1 | Overall warning |
| 2 | Overall problem |
| 3 | The doctor could not run (for example, the config is unreadable) |

## Fix mode (`--fix FIX_ID`)

1. Previews the fix and prints its items. With no items, prints "Nothing to fix" and exits 0.
2. `--dry-run`: stops after the preview and exits 0.
3. A destructive fix without `--yes` asks `Apply <label> to N item(s)? [y/N]`. Any answer
   other than y or yes cancels (exit 0, nothing changed). A non-destructive fix runs without
   asking.
4. Applies the fix to the previewed items, prints the done, skipped and failed counts, and
   then prints the re-run check. Exits 1 if anything failed, otherwise 0.

An unknown fix id prints an error that lists the valid ids, and exits 3.

With `--json` in fix mode, the command prints the `FixPreview` (with `--dry-run`) or the
`FixOutcome` as JSON. Without `--yes`, a destructive fix is then refused with exit 3, because
it cannot prompt while writing JSON.
