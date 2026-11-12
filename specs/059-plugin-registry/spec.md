# Feature Specification: Plugin Links and a Plugin Registry

**Feature Branch**: `059-plugin-registry`

**Created**: 2026-11-10

**Status**: Draft

**Input**: User description: "plugins should be able to link to their own docs, github repo, author,
etc. also we should make a github repo that is registry for all plugins approved and have a
contribution guidelines with examples."

The brief is ROADMAP.md › "059 Plugin links and a plugin registry".

## Context

- A plugin is a folder with `plugin.toml` and `main.lua` (028, 031, 048). The manifest has a name,
  version, description, events, permissions, settings and actions. It has no way to say who wrote
  the plugin or where its docs, source or bug tracker are, and the manifest rejects unknown fields.
- Seven plugins ship with putio-sync. Any other plugin is installed by copying a folder into the
  plugins folder and pressing **Reload plugins**. There is no list of plugins others have written
  and no way to tell whether a plugin someone shares has been looked at by anyone.
- Enabling a plugin names the hosts it will contact and asks for approval; a manifest change that
  adds a host shows **Needs approval** again. This stays the trust boundary for every plugin,
  however it was installed.
- Bundled plugins share Lua helper files (`notifier.lua`, `mediaserver.lua`) put in front of their
  scripts. A plugin from anywhere else is one self-contained folder.
- 055 (plugin workbench) adds starter templates, `examples/plugins/` and a plugin-writing guide on
  the docs site. This feature links to that guide and reuses those starters as registry examples.

## Clarifications

### Session 2026-11-10

Asked of the user before the spec was written:

- Q: Should putio-sync browse and install plugins from the registry, or is the registry only a
  reviewed list people install by hand? → A: Browse and install in the app.
- Q: What is the registry repository called? → A: `devclinton/putio-sync-plugin-registry`, public.
- Q: How to proceed? → A: A roadmap brief and this spec, on its own branch.

Settled by the session from the request and the code (recommended options, not asked):

- Q: What does a registry entry point to? → A: A plugin's own repository and one release of it: a
  version, a download address of a `.tar.gz` or `.zip` archive, and that archive's SHA-256. The
  registry holds no plugin code; the plugin's author keeps it in their own repository.
- Q: When does putio-sync contact the registry? → A: Only when the Plugins page is opened (and on
  **Refresh**), with the list kept for 6 hours. Never in the background. The registry can be
  turned off or pointed at another address in Settings.
- Q: Is an installed registry plugin trusted more than a hand-copied one? → A: No. It runs in the
  same sandbox and needs the same host approval. "Reviewed" means a person read the code and
  manifest against the review checklist; the app says so in those words and no more.
- Q: What happens to a hand-made folder with the same name as a registry plugin? → A: It is left
  alone. The registry plugin shows **Name in use** and cannot be installed until the folder is
  renamed or removed. Install never overwrites a folder it did not install.
- Q: May a registry plugin use a bundled plugin's name? → A: No. The review checklist forbids it,
  and the registry's checks refuse it, so a registry install never replaces a bundled plugin.
- Q: Are updates installed on their own? → A: No. The app shows that an update is available and
  what changed in the manifest (version, new hosts, new settings); the operator presses
  **Update**.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See who made a plugin and where its docs are (Priority: P1) 🎯 MVP

As the operator, I want each plugin's tab to show its author, docs, source repository, issue
tracker and license, so I know who to ask and where to read before I turn it on.

**Why this priority**: It is the first part of the request, it is useful without a registry, and
the registry depends on these fields to link each entry to its source.

**Independent Test**: Add `author`, `homepage`, `docs`, `repository`, `issues` and `license` to a
folder plugin's `plugin.toml`, reload, and open its tab: each shows, the links open in a new tab.
Remove them all: the plugin still loads and the tab shows no empty labels.

**Acceptance Scenarios**:

1. **Given** a plugin whose manifest sets the new fields, **When** I open its tab or its line
   under **Available plugins**, **Then** I see the author and license as text and **Docs**,
   **Website**, **Source** and **Report a problem** as links that open in a new tab.
2. **Given** a manifest that sets none of the new fields, **When** plugins reload, **Then** the
   plugin loads as before and shows no metadata row.
3. **Given** a manifest whose link is not an `https` address (or `http` for a local address), or is
   longer than 300 characters, **When** plugins reload, **Then** the plugin is **Broken** with a
   `plugin.toml: docs: …` message in the same form as other manifest errors.
4. **Given** the bundled plugins, **When** I open their tabs, **Then** each links to its section of
   the docs site and to the putio-sync repository, with the putio-sync license.
5. **Given** a plugin with a `docs` link, **When** its tab shows a settings error or **Needs
   settings**, **Then** the message links to that plugin's docs.

---

### User Story 2 - Find and install a reviewed plugin (Priority: P1)

As the operator, I want a list of plugins other people wrote that someone has reviewed, and to
install one from the Plugins page, so I can add a plugin without shell access to the NAS.

**Why this priority**: It is the second half of the request and the reason for the registry.

**Independent Test**: Point the registry address at a test registry served locally with one
plugin. Open the Plugins page: a **More plugins** section lists it with its description, author,
version and links. Press **Install**: the folder appears in the plugins folder, the plugin is
listed as available and off, and **Enable** asks to approve its hosts as today.

**Acceptance Scenarios**:

1. **Given** the registry is on, **When** I open the Plugins page, **Then** a **More plugins**
   section lists each registry plugin with name, version, description, author, the hosts it
   contacts, its links and **Install**, and says that these plugins are reviewed by people, run in
   the same sandbox, and still ask before contacting any host.
2. **Given** the list, **When** I type in its search box, **Then** it shows only plugins whose name,
   description or author contains the text, and I can filter by the events a plugin handles.
3. **Given** I press **Install**, **When** the download's SHA-256 matches the registry entry and the
   archive holds one `plugin.toml` whose name and version match the entry, **Then** the plugin is
   unpacked into `plugins/<name>/`, plugins reload, and it shows under **Available plugins**, off,
   marked "From the registry".
4. **Given** the download's SHA-256 does not match, or the archive's manifest differs from the
   entry, or the archive holds a path outside its folder, a link, or more than 5 MB unpacked,
   **When** I install, **Then** nothing is written to the plugins folder and the page says which
   check failed.
5. **Given** a folder already exists with that name that putio-sync did not install from the
   registry, **When** I view the entry, **Then** it shows **Name in use** instead of **Install**.
6. **Given** the registry cannot be reached, **When** I open the Plugins page, **Then** the
   installed plugins work and show as before, and **More plugins** says the registry could not be
   reached, with the last list kept (and its age) if there is one.
7. **Given** an entry needs a newer putio-sync than the one running, **When** I view it, **Then**
   **Install** is off and the entry says which version it needs.
8. **Given** the registry is turned off in Settings, **When** I open the Plugins page, **Then**
   putio-sync makes no request to the registry and **More plugins** says where to turn it on.

---

### User Story 3 - Keep registry plugins up to date (Priority: P2)

As the operator, I want to see when a plugin I installed from the registry has a newer version and
what the update changes, and update or remove it from the page.

**Why this priority**: Installing without updates leaves people on old, possibly broken versions,
but install is useful first.

**Independent Test**: Install version 1.0.0 from a local test registry, then change the registry to
list 1.1.0 with one more host. Refresh: the plugin's tab shows **Update to 1.1.0** and the new
host. Update: the plugin runs 1.1.0, its settings are kept, and it shows **Needs approval** for
the new host.

**Acceptance Scenarios**:

1. **Given** a registry plugin whose entry lists a newer version, **When** I open the Plugins page,
   **Then** the plugin's tab and its line show **Update available** with the new version and a
   link to its release notes when the entry has one.
2. **Given** an update, **When** I open it, **Then** I see what the new manifest changes: hosts
   added or removed, settings added or removed, and events added or removed.
3. **Given** I press **Update**, **When** the new archive passes the same checks as an install,
   **Then** the folder is replaced, the previous version is kept until the new one loads, the
   plugin's settings, secrets and store are kept, and new hosts need approval as today.
4. **Given** the new version fails to load, **When** the update finishes, **Then** the previous
   version is put back and the page says why the new one failed.
5. **Given** a registry plugin, **When** I press **Remove**, **Then** after I confirm, its folder is
   deleted, its settings are kept (as for any missing plugin), and it shows under **More plugins**
   again.
6. **Given** a registry entry is withdrawn (marked so in the registry), **When** I open the Plugins
   page, **Then** an installed copy keeps working, its tab shows the reason given, and the doctor
   reports it.
7. **Given** I edit the files of a plugin installed from the registry, **When** an update is
   available, **Then** the tab says the files were changed and **Update** asks before replacing
   them.

---

### User Story 4 - Contribute a plugin to the registry (Priority: P2)

As a plugin author, I want a registry repository with clear guidelines, an entry template, a
review checklist and examples, so I know what a plugin needs to be listed and how to submit it.

**Why this priority**: The registry is empty without contributors, and the review is only as good
as the checklist it is held to.

**Independent Test**: Follow CONTRIBUTING.md from a fresh copy of the registry repository: copy the
example plugin to a new repository, tag a release, add an entry with the template, and open a pull
request. The registry's checks pass for the correct entry and fail with a clear message when the
checksum, name or manifest is wrong.

**Acceptance Scenarios**:

1. **Given** the registry repository, **When** I open its README, **Then** it says what the
   registry is, what "reviewed" does and does not mean, how putio-sync uses it, and links to
   CONTRIBUTING.md, the plugin-writing guide on the docs site and PLUGINS.md.
2. **Given** CONTRIBUTING.md, **When** I read it, **Then** it explains, with an example, how to
   prepare a plugin repository and a release archive, write an entry, compute the SHA-256, and
   submit a pull request, and lists the review checklist.
3. **Given** the review checklist, **When** a reviewer uses it, **Then** it covers at least: the
   manifest sets `author`, `repository` and `license` (an open source license); the hosts it
   contacts are the fewest needed and explained in its README; secrets are marked `secret`; it
   does not use a bundled plugin's name; it sends no data the README does not mention; it passes
   the putio-sync manifest check; and the release archive holds only the plugin folder.
4. **Given** the `examples/` folder, **When** I open it, **Then** it holds at least two complete
   example plugins (a webhook notifier and a plugin with settings, a store and an action), each
   with a manifest using every metadata field, a README and a matching example registry entry.
5. **Given** a pull request that adds or changes an entry, **When** the registry's checks run,
   **Then** they check the entry's form, that the name is unique and not bundled, that the archive
   downloads and matches its SHA-256, and that its manifest passes the putio-sync manifest check
   and matches the entry's name and version.
6. **Given** a merged change, **When** the registry's checks finish on the main branch, **Then** the
   single index file the app reads is rebuilt from the entries.

---

### Edge Cases

- A plugin's link uses `javascript:`, `data:` or another scheme: the manifest is refused, and the
  UI never makes such a link clickable.
- A registry entry lists an author or description with markup: it is shown as plain text.
- The registry index is larger than 2 MB, is not valid, or lists the same name twice: the app
  keeps the last good list and says the registry's list is invalid.
- The download address redirects (GitHub release assets do): redirects are followed, up to 5, to
  `https` addresses only.
- Two installs or updates of the same plugin at once: the second waits for the first or is refused.
- The plugins folder is read-only: install says so and writes nothing.
- A registry plugin is also enabled in `config.toml` before it is installed: its settings are used
  once the folder appears, as for any plugin that comes back.
- An archive whose top folder name differs from the plugin name: it is unpacked under the
  plugin's name; an archive with more than one top folder is refused.
- A plugin installed from the registry is later removed from the registry without a withdrawal
  reason: it keeps working and shows "No longer listed".

## Requirements *(mandatory)*

### Functional Requirements

**Plugin links (US1)**

- **FR-001**: `plugin.toml` MUST accept optional `author` (text, up to 100 characters), `license`
  (an SPDX license identifier or expression, up to 60 characters), and `homepage`, `docs`,
  `repository` and `issues` (each an `https` address, or `http` for a local address, up to 300
  characters). Manifests without them MUST load as before.
- **FR-002**: An invalid metadata value MUST make the plugin **Broken** with a `plugin.toml: <field>:
  <message>` error, like other manifest errors.
- **FR-003**: A plugin's tab and its **Available plugins** line MUST show the author and license as
  plain text and each link present as a link that opens in a new tab; fields not set MUST NOT
  show.
- **FR-004**: The bundled plugins MUST set the metadata fields, linking to their docs pages and
  to the putio-sync repository.
- **FR-005**: Settings errors and **Needs settings** on a plugin with a `docs` link MUST link to it.
- **FR-006**: The plugins API MUST return the metadata fields for each plugin.
- **FR-007**: A `putio-sync plugins check <folder>` command MUST check a plugin folder the same way
  a reload does (manifest, entry file present, Lua syntax) and print each error, exiting non-zero
  on any error, so the registry's checks and authors can use it without running the daemon.

**Registry repository (US4)**

- **FR-010**: A public repository `devclinton/putio-sync-plugin-registry` MUST hold one entry file
  per plugin under `plugins/`, an index file built from them, a README, CONTRIBUTING.md, a review
  checklist, an entry template, and `examples/`.
- **FR-011**: Each entry MUST give: name, description, author, repository, license, the events it
  handles, the hosts it contacts (fixed and from settings), and one or more releases, each with
  version, download address, SHA-256, the lowest putio-sync version it needs, and optional release
  notes link. An entry MAY be marked withdrawn with a reason.
- **FR-012**: The registry's checks MUST run on every pull request and refuse an entry that is
  malformed, uses a name already listed or bundled, has an archive that does not download or does
  not match its SHA-256, or whose manifest fails FR-007 or differs from the entry's name, version
  or hosts.
- **FR-013**: The index file MUST be rebuilt from the entries on the main branch and published at a
  fixed address.
- **FR-014**: CONTRIBUTING.md MUST walk through submitting the example plugin end to end and MUST
  state the review checklist and that a maintainer approves each pull request.

**Browse and install (US2)**

- **FR-020**: Settings MUST have a registry switch (on by default) and a registry address
  (defaulting to the published index of FR-013), also in `config.toml` and `config.example.toml`.
- **FR-021**: putio-sync MUST fetch the index only when the Plugins page is opened or **Refresh**
  is pressed, at most once per 6 hours otherwise, and MUST NOT contact the registry when the
  switch is off.
- **FR-022**: The Plugins page MUST list registry plugins not installed under **More plugins**, with
  search and an events filter, and state what "reviewed" means.
- **FR-023**: Install MUST download the latest release the running version supports, verify its
  SHA-256 before unpacking, refuse archives with paths outside the folder, links, more than one top
  folder, more than 200 files or more than 5 MB unpacked, check that the manifest matches the
  entry, and only then write `plugins/<name>/`. On any failure it MUST write nothing.
- **FR-024**: Install MUST NOT overwrite a folder it did not install, and MUST NOT enable the
  plugin or approve any host.
- **FR-025**: putio-sync MUST record which plugins it installed from the registry, with the version
  and SHA-256 installed, in the plugin's folder, and MUST show "From the registry" on them.
- **FR-026**: Registry text MUST be shown as plain text; only `https` links from the registry MUST
  be made clickable.
- **FR-027**: Install, update and remove MUST require the same API authentication as other
  settings changes.

**Updates (US3)**

- **FR-030**: A registry plugin with a newer listed release that the running version supports MUST
  show **Update available** with the version and the manifest changes in hosts, settings and
  events.
- **FR-031**: **Update** MUST pass the FR-023 checks, keep settings, secrets, store and approvals,
  require approval of new hosts as today, and put the previous version back if the new one fails
  to load.
- **FR-032**: If the installed files differ from what was installed, **Update** MUST ask before
  replacing them.
- **FR-033**: **Remove** MUST ask first, delete the folder, and keep the plugin's settings.
- **FR-034**: Withdrawn and no-longer-listed registry plugins MUST keep working, show the reason,
  and be reported by the doctor.
- **FR-035**: Metrics MUST count registry fetches, installs, updates and failures by outcome, with
  no plugin names as labels.

**Docs**

- **FR-040**: PLUGINS.md and the docs site MUST document the metadata fields, the registry, its
  settings, what "reviewed" means, and link to the registry repository's CONTRIBUTING.md.

### Key Entities

- **Plugin metadata**: author, license, homepage, docs, repository and issues on a plugin's
  manifest.
- **Registry entry**: one plugin in the registry: what it is, who wrote it, what it contacts, its
  releases, and whether it is withdrawn.
- **Release**: one version of a registry plugin: version, archive address, SHA-256, lowest
  putio-sync version, release notes link.
- **Registry index**: the single file built from all entries that putio-sync reads.
- **Install record**: kept in an installed plugin's folder: registry address, version and SHA-256
  installed, and when.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An operator can find and install a registry plugin from the Plugins page in under one
  minute, with no shell access.
- **SC-002**: 100% of archives whose checksum or manifest does not match their entry are refused
  with nothing written to the plugins folder, in automated tests.
- **SC-003**: Every existing plugin manifest (bundled and examples) loads unchanged after the
  metadata fields are added.
- **SC-004**: With the registry off, putio-sync makes no request to the registry, checked in
  automated tests.
- **SC-005**: A first-time author can follow CONTRIBUTING.md to a passing pull request for the
  example plugin without asking a question.
- **SC-006**: An update never loses a plugin's settings, secrets or store, and a failed update
  leaves the previous version running, in automated tests.

## Assumptions

- The registry is published from GitHub (index served from the repository's GitHub Pages or raw
  file address); a self-hosted copy works by changing the registry address.
- Plugin authors host their own code and release archives (normally GitHub releases). The registry
  repository holds no plugin code except `examples/`.
- Review is done by the maintainer through pull requests; there is no automated trust score,
  signing key or author account in this feature.
- Only Lua plugins are listed; 029 WebAssembly plugins can be added to the same entry form later
  with a runtime field.
- Tests use a local test registry and local archives; no test contacts GitHub.
- Archive formats are `.tar.gz` and `.zip`.

## Dependencies

- 055 plugin workbench: its starter plugins and writing guide are reused as registry examples and
  linked from the registry docs. This feature can ship first; the links then point to PLUGINS.md.
- Creating the public registry repository is done once by the maintainer, outside the putio-sync
  repository.
