# Research: Plugin Links and a Plugin Registry

Decisions for [plan.md](plan.md). Each has what was chosen, why, and what else was looked at.

## R1. Where the registry code lives

- **Decision**: a `registry` module in `putio-sync-plugins` (index, fetch, archive, install). The
  API crate only adds routes; the CLI only adds `plugins check`.
- **Rationale**: the plugins crate already owns discovery, the manifest, the plugins folder and
  `reqwest`. Principle I puts behaviour in the crate that owns the concern.
- **Alternatives**: a new `putio-sync-plugin-registry` crate. Rejected: it would need most of the
  plugins crate's types and add a crate for about 1 000 lines.

## R2. Index address and format

- **Decision**: the default address is
  `https://raw.githubusercontent.com/devclinton/putio-sync-plugin-registry/main/index.json`.
  The file is the JSON that `scripts/registry.py build` writes: `{"schema_version": 1,
  "plugins": [entry, …]}`, each entry the TOML entry as JSON (see
  [contracts/registry-index.md](contracts/registry-index.md)). An unknown `schema_version`
  is refused with "registry needs a newer putio-sync".
- **Rationale**: a committed file needs no GitHub Pages set-up, is cached by GitHub's CDN and can
  be mirrored by copying one file. Using the build output as-is keeps one source of truth.
- **Alternatives**: GitHub Pages (one more thing to set up; can be switched to by changing the
  default later); the GitHub API listing `plugins/` (rate-limited, many requests).

## R3. When the index is fetched, and caching

- **Decision**: `GET /plugins/registry` fetches when the cache is older than 6 hours or
  `refresh=true`, never otherwise. The last good index is kept in
  `plugin-data/registry-index.json` with its fetch time, so a restart or an unreachable registry
  still shows the last list (FR-021, US2 #6). The daemon never fetches on its own, and the doctor
  reads only the cached copy. One fetch runs at a time; callers wait for it.
- **Rationale**: no background traffic, works offline, the doctor stays network-free for the
  registry.
- **Alternatives**: a daily background check (rejected in the spec: no background contact).

## R4. HTTP limits

- **Decision**: one `reqwest` client for the registry: 30 s timeout, up to 5 redirects, each to
  `https` (or `http` on a loopback or private address, for tests and self-hosting on a LAN), index
  read up to 2 MB and archives up to 5 MB, stopping the read at the limit. `User-Agent:
  putio-sync/<version>`.
- **Rationale**: GitHub release assets redirect to `objects.githubusercontent.com`; the limits
  match the spec and the registry's own checks.

## R5. Archive formats and checks

- **Decision**: `.tar.gz` through `flate2` + `tar`, `.zip` through `zip` (deflate only, no
  default features). The SHA-256 (`sha2`) of the downloaded bytes is checked before the archive is
  opened. Entries are read into memory (5 MB cap) and refused when: a path is absolute or holds
  `..`, an entry is a link or anything but a file or folder, there is not exactly one top folder,
  there are more than 200 files, or the unpacked total passes 5 MB. Then `plugin.toml` is parsed
  with `Manifest::parse` and must match the entry's name, version, events and hosts, and the
  entry script must be present. Nothing touches disk until all pass.
- **Rationale**: the same rules as `scripts/registry.py`, so an entry that passes CI installs.
  In-memory checks make "write nothing on failure" (FR-023) simple.
- **Alternatives**: `.tar.gz` only (drops a dependency, but `.zip` is what Windows authors make);
  unpacking with the crates' own `unpack` (follows their rules for links and paths, not ours).

## R6. Writing the folder, updates and rollback

- **Decision**: files are written to `plugins/.install-<name>/`, then renamed to `plugins/<name>/`.
  For an update the current folder is first renamed to `plugins/.previous-<name>/`; after the swap
  that one plugin is reloaded, and if it comes back **Broken** the folders are swapped back and the
  error returned (FR-031, US3 #4). `.previous-*` is removed after a good update. Dot-folders are
  already skipped by discovery. Leftover `.install-*` and `.previous-*` folders are cleaned on
  start, restoring a `.previous-<name>` whose `<name>` is missing.
- **Rationale**: a rename in one folder is atomic on the filesystems putio-sync runs on, and keeps
  the old version until the new one is known to load.

## R7. Install record and "changed" detection

- **Decision**: `plugins/<name>/.registry.json` holds the registry address, version, archive
  SHA-256, install time, and the SHA-256 of each installed file. "Changed" (FR-032) means any file
  added, removed or different from that list, ignoring dot-files. A folder without a record is not
  a registry install (FR-024: **Name in use**, never overwritten or removed by the app).
- **Rationale**: kept with the folder, so it moves and is deleted with it; no new config section.
- **Alternatives**: a list in `config.toml` (gets out of step when folders are copied by hand).

## R8. Versions

- **Decision**: the `semver` crate for comparing release versions and `min_putio_sync` with the
  running `CARGO_PKG_VERSION`. The newest release whose `min_putio_sync` is met is the one offered.
- **Rationale**: pre-release ordering is easy to get wrong by hand; the crate is small and has no
  dependencies.

## R9. Manifest metadata

- **Decision**: six optional `String` fields on `Manifest` with `#[serde(default)]`, checked in
  `Manifest::check`: `author` ≤ 100 characters, no control characters; `license` matches
  `[A-Za-z0-9.+() -]{1,60}` (SPDX ids and expressions without a full SPDX parser); `homepage`,
  `docs`, `repository`, `issues` ≤ 300 characters and `https://` with a host, or `http://` with a
  loopback or private host. Returned on `PluginStatus` as `links`.
- **Rationale**: `deny_unknown_fields` stays, so typos are still caught. Older putio-sync refuses
  manifests with these fields; the registry's `min_putio_sync` handles that.

## R10. `plugins check`

- **Decision**: `putio-sync plugins check <folder>` runs `discover::from_folder` (made `pub`) on the
  folder, then compiles the script in a fresh sandbox state with `Lua::load(..).into_function()`,
  which parses without running it. Errors print one per line as `file: field: message` or
  `main.lua:12: …`; exit 1 on any. It needs no config, token or daemon.
- **Rationale**: the same code as a reload, so the registry's CI and authors see what the app
  would say.

## R11. UI and languages

- **Decision**: **More plugins** is a new block in `ui/settings/plugins.js` below **Available
  plugins**; pure helpers (search/filter, link check, update diff text, entry state) go in
  `ui/plugins.js` with node tests. Links are drawn only when they start with `https://` (or the
  local-address `http://` rule), with `rel="noopener noreferrer"` and `target="_blank"`. All
  registry text goes through `textContent`. New strings go into `ui/i18n/en.json` and are
  translated with `scripts/i18n-translate.mjs` like 051.

## R12. Metrics

- **Decision**: `putio_sync_plugin_registry_fetches_total{result="ok|error|cached"}` and
  `putio_sync_plugin_registry_changes_total{kind="install|update|remove", result="ok|error"}`. No
  plugin names as labels (FR-035, principle V).
