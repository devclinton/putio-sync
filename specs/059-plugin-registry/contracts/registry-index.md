# Contract: the registry index (`index.json`)

Written by `scripts/registry.py build` in `devclinton/putio-sync-plugin-registry` from
`plugins/*.toml`; read by putio-sync. Both sides MUST agree on this; the registry's `check`
enforces the entry rules and putio-sync re-checks them on read.

```json
{
  "schema_version": 1,
  "plugins": [
    {
      "name": "webhook-notifier",
      "description": "POST a small JSON message to a webhook when a download finishes or fails",
      "author": "devclinton",
      "repository": "https://github.com/devclinton/putio-sync-plugin-registry",
      "license": "MIT",
      "docs": "https://…",
      "issues": "https://…",
      "events": ["download.finished", "download.failed"],
      "hosts": { "fixed": [], "from_config": ["url"] },
      "releases": [
        {
          "version": "1.0.0",
          "url": "https://github.com/…/webhook-notifier-1.0.0.tar.gz",
          "sha256": "6b6f…b0cb",
          "min_putio_sync": "0.3.0",
          "notes": "https://…"
        }
      ]
    }
  ]
}
```

| Field | Required | Rule |
|-------|----------|------|
| `schema_version` | yes | `1`; putio-sync refuses others |
| `name` | yes | `[A-Za-z0-9_-]{1,40}`, unique (case-insensitive), not a bundled name |
| `description` | yes | 1–300 characters |
| `author` | yes | 1–100 characters |
| `repository` | yes | `https`, ≤ 300 |
| `license` | yes | SPDX-like, ≤ 60 |
| `docs`, `homepage`, `issues` | no | `https`, ≤ 300 |
| `events` | yes | non-empty list; equals the manifest's |
| `hosts.fixed`, `hosts.from_config` | no (default `[]`) | equal the manifest's `[permissions.http]` |
| `releases` | yes | non-empty, newest first, versions unique |
| `releases[].version` | yes | semver; equals the archive manifest's |
| `releases[].url` | yes | `https`, ends `.tar.gz`, `.tgz` or `.zip` |
| `releases[].sha256` | yes | 64 lowercase hex |
| `releases[].min_putio_sync` | yes | semver |
| `releases[].notes` | no | `https` |
| `withdrawn` | no | reason text, ≤ 300 |

Where the table says `https`, putio-sync also accepts `http` to a loopback or private address (the
manifest's link rule), for a registry served on the LAN or in tests; the public registry's `check`
accepts `https` only.

Unknown fields are ignored by putio-sync (refused by the registry's `check`), so the format can
grow without breaking older apps. A breaking change bumps `schema_version`.

## Archive rules (both sides)

One top folder; plain files and folders only; no absolute paths or `..`; ≤ 200 files; ≤ 5 MB
packed and unpacked; holds `plugin.toml` and its `entry` script. The top folder's name does not
matter; putio-sync installs under the entry's `name`.
