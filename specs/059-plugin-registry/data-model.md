# Data Model: Plugin Links and a Plugin Registry

## Plugin links (manifest, US1)

New optional fields at the top level of `plugin.toml`. All default to empty, meaning not set.

| Field | Rule | Shown as |
|-------|------|----------|
| `author` | 1–100 characters, no control characters | text |
| `license` | `[A-Za-z0-9.+() -]{1,60}` | text |
| `homepage` | link rule | **Website** |
| `docs` | link rule | **Docs**; also linked from settings errors |
| `repository` | link rule | **Source** |
| `issues` | link rule | **Report a problem** |

Link rule: at most 300 characters; `https://` with a host, or `http://` with a loopback or private
host. Anything else is a manifest error (`plugin.toml: docs: must be an https address`).

`PluginStatus` gains `links: {author, license, homepage, docs, repository, issues}` (empty
strings left out) and `registry: Option<InstallState>` (below).

## Registry index (read from the registry, US2)

See [contracts/registry-index.md](contracts/registry-index.md) for the exact format.

- **Index**: `schema_version` (1), `plugins: [Entry]`. Parsed with unknown fields ignored, so the
  registry can add fields.
- **Entry**: `name`, `description`, `author`, `repository`, `license`, optional `docs`,
  `homepage`, `issues`, `events`, `hosts {fixed, from_config}`, `releases: [Release]` newest
  first, optional `withdrawn` (reason).
- **Release**: `version`, `url`, `sha256`, `min_putio_sync`, optional `notes`.

An index is refused as a whole (last good one kept) when it is over 2 MB, not JSON, of an unknown
`schema_version`, or lists a name twice. A single entry that breaks the rules (bad name, bundled
name, no usable release, bad link) is dropped and counted in `skipped`.

## Cached index (stored)

`plugin-data/registry-index.json`: `{url, fetched_at, index}`. Used when it is younger than 6 hours
and its `url` matches the configured one; shown with its age when the registry cannot be reached.

## Install record (stored, per plugin)

`plugins/<name>/.registry.json`:

| Field | Meaning |
|-------|---------|
| `registry` | The index address it came from |
| `version` | The installed release |
| `sha256` | The archive's SHA-256 |
| `installed_at` | RFC 3339 |
| `files` | `{relative path: sha256}` of every installed file, dot-files excluded |

## Registry state of a plugin (derived)

Worked out for each index entry and each installed plugin when `GET /plugins/registry` or
`GET /plugins` runs.

| State | When | Offers |
|-------|------|--------|
| `available` | In the index, no folder and no bundled plugin by that name | **Install** |
| `needs_newer` | No release whose `min_putio_sync` this version meets | — (says the version) |
| `name_in_use` | A folder or bundled plugin by that name without an install record | — |
| `installed` | Install record present, its version is the newest offered | **Remove** |
| `update` | Install record present, a newer release is offered | **Update**, **Remove** |
| `withdrawn` | Entry has `withdrawn` | **Remove** (installed); not listed for install |
| `unlisted` | Install record present, name not in the index | **Remove** |

`InstallState` on `PluginStatus`: `{version, installed_at, state, changed, update?: {version,
notes, diff}, withdrawn?: reason}`, where `changed` is FR-032's check and `diff` is
`{hosts_added, hosts_removed, settings_added, settings_removed, events_added, events_removed}`
worked out from the entry's hosts and events and, for settings, the new archive's manifest once
**Update** is opened (see the API contract).

## Settings

```toml
[plugin_registry]
enabled = true
url = "https://raw.githubusercontent.com/devclinton/putio-sync-plugin-registry/main/index.json"
```

`url` follows the link rule. Saved through the existing settings API; changing `url` drops the
cache.
