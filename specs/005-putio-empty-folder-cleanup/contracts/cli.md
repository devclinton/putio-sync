# CLI Contract: `putio-sync files clean-empty`

```text
putio-sync files clean-empty [--dry-run] [-y|--yes]
```

- Uses `[cleanup] min_age_hours` and the protected set from the config file; talks to
  Put.io directly (daemon not needed).
- `--dry-run`: print the folders that would be removed and the summary; delete nothing.
- Without `--dry-run`: first a dry run is printed; if there is anything to remove, asks
  `Delete N empty folder(s) from Put.io? [y/N]` unless `--yes`; then runs for real and
  prints the result.
- Output: one line per folder (`id`, `path`, `age`), then
  `Scanned S folders: removed R, kept K protected, T too new, F failed`.
- Exit code: 0 on success (including nothing to do), non-zero when the run fails or any
  delete fails.
