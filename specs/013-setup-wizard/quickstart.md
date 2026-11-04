# Quickstart: validating the setup wizard

Use a scratch config and a port other than 15432.

## Automated

```bash
TMPDIR=$SCRATCH make ci
```

Covers: device-link client against wiremock; setup checks and `SetupChoices::apply`; the root
preset in `RuleSet::resolve`; the dry run against mocked listings; every `/setup/*` route through
`oneshot` (auth, 503 for other routes, finish writes once with 0600); the CLI non-interactive
path; the UI size cap (300 KB).

## Manual: web wizard

1. `PUTIO_SYNC_CONFIG=$SCRATCH/config.toml putio-sync daemon` with `[api] bind` overridden by
   env to `127.0.0.1:18432` (or write a config with only `[api] bind`).
2. Expect the console to print the setup code and URL; `GET /auth/status` → `setup_required: true`;
   `GET /api/status` → 503.
3. Open the URL, enter the code, paste a token (or link with an app ID), choose a folder under
   `$SCRATCH`, keep "No binaries", run the dry run, press "Start syncing".
4. Expect: config written (mode 600) with token, target, preset and an API token; the UI lands on
   Status; the daemon is syncing without a restart; `putio_sync_setup_required 0` when metrics on.

## Manual: CLI against a waiting daemon

1. Start the daemon as above (setup mode).
2. In another shell: `PUTIO_SYNC_PUTIO_TOKEN=… putio-sync --config $SCRATCH/config.toml setup
   --download-dir $SCRATCH/dl --yes`.
3. Expect the daemon to leave setup mode within 15 s.
