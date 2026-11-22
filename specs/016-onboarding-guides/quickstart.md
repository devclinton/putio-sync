# Quickstart: validating the onboarding guides (016)

## Build and check the site locally

```bash
site/tools/build.sh
```

```bash
lychee --offline --include-fragments --root-dir "$PWD/target/site" --exclude-path 'docs/([a-zA-Z-]+/)?404\.html' 'target/site/**/*.html'
```

```bash
cd site/tools && npm ci && npm run a11y
```

Expected: the build lists the Guides part, lychee reports 0 errors, and a11y reports no
violations. Open `target/site/docs/guides/index.html` and check every guide is two clicks
from the docs home (SC-003).

## Refresh translations

```bash
ATRANSLATE_URL=http://localhost:8400 PATH="$HOME/dev/atranslate/.build/debug:$PATH" site/tools/translate-docs.py
```

Expected: each `site/i18n/*.json` gains the new blocks. A rebuilt `docs/de/guides/unraid.html`
shows German text with the machine-translation notice.

## Walk-throughs (per guide PR)

For each guide, follow it from the top on the named machine, as written, and tick each step
in the PR's checklist with who checked it:

| Guide | Where | Who |
|-------|-------|-----|
| macOS | this Mac, Homebrew, after `brew uninstall` and a fresh config folder | Claude |
| Unraid | Mnemosyne, a second container from the template on another port and config path | Claude |
| Docker and Compose | this Mac's Docker, or Mnemosyne | Claude |
| Windows | the maintainer's Windows machine | maintainer |
| Linux | a VM (snap, then .deb, then the binary) | maintainer |
| TrueNAS SCALE | a VM | maintainer |
| Raspberry Pi | none: the page carries the untested note | — |

Task guides run on a working install: Radarr/Sonarr and the media server on Mnemosyne,
folder rules and watch folders on the Mac, and metrics/Grafana against the Mnemosyne
endpoint. A walk-through passes when the guide's last step shows the promised result (for
example a test file on disk, or Radarr importing a release) with no step needing other docs.
Timing the setup guides checks SC-001 (under 15 minutes, not counting download time).

Never print or paste the NAS API token or config contents while doing this.

## UI links (PR D)

```bash
make ci
```

Expected: `guides.test.mjs` passes, and fails if a slug in `ui/guides.js` is renamed without
its chapter. Then, against a local build:

1. Open the setup wizard. "Setup guide" points at `guides/index`, since a dev build is
   `unknown`. Run with `PUTIO_SYNC_INSTALL=unraid` and it points at `guides/unraid`.
2. On each settings section in the contract, the "Guide" link opens the right page.
3. In the command palette, typing "grafana" shows the metrics guide under Guides, and Enter
   opens it in a new tab.
