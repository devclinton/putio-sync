# Contract: `putio-sync setup`

```text
putio-sync [--config <path>] setup
    [--download-dir <path>]
    [--preset <name>]
    [--watch-dir <path>]
    [--metrics | --no-metrics]
    [--no-dry-run]
    [--yes]
```

- **Put.io**: token from `PUTIO_SYNC_PUTIO_TOKEN`; else, with a terminal, the device link when an
  app ID is known (prints code and `https://put.io/link`, waits) or a paste prompt; without a
  terminal, the first line of standard input. Never a flag (would land in shell history / `ps`).
- **Prompts**: any value not given by a flag is asked for, with the suggestion as default
  (Enter accepts). `--yes`, or no terminal, accepts every suggestion and fails before writing
  when a required value has none, naming the flag.
- **Checks**: the folder check from the wizard; an unusable folder is an error with `--yes`,
  a re-prompt otherwise.
- **Dry run**: runs unless `--no-dry-run`; prints totals and up to 20 samples; then asks
  "Write config? [Y/n]" (skipped with `--yes`).
- **Write**: one `Config::save` (mode 0600), keeping other settings. Creates an API token if none
  is configured (file or `PUTIO_SYNC_API_TOKEN`) and prints it once.
- **Radarr/Sonarr**: skipped; a closing line points to Settings → Radarr/Sonarr.
- **Exit codes**: 0 written; 1 error (message on stderr); 2 cancelled by the user.
- A daemon waiting in setup mode notices the new config within 15 s and starts syncing.
