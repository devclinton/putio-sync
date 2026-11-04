# Feature Specification: What's New

**Feature Branch**: `017-whats-new` (built on `claude/nostalgic-dhawan-736be1`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "017 What's new. After an upgrade, the web UI shows the
CHANGELOG.md entries since the version the user last saw. The changelog is built into the
binary."

## Context

- `CHANGELOG.md` at the repository root lists user-facing changes in Keep a Changelog form:
  an `## [Unreleased]` section on top, then one `## [x.y.z] - date` section per release, each
  with `### Added`, `### Fixed`, `### Changed` (and other Keep a Changelog) groups of bullet
  entries. Every user-facing change adds an entry under Unreleased in the same commit.
- Builds from main (the edge snap, the `:main` Docker image) carry the Unreleased section; a
  release renames it to the version. Today there is no released section yet: every entry is
  under Unreleased, and an edge build can gain new entries without the version changing.
  "Since the version the user last saw" therefore cannot be tracked by version number alone.
- The Docker image already ships the file at `/usr/share/doc/putio-sync/CHANGELOG.md`, but
  nothing reads it, and other installs (snap, Homebrew, binary) do not ship it.
- The web UI shows the version and commit on the Status page. It has no way to tell the user
  what changed after an upgrade.
- 013 added setup mode: an unconfigured daemon serves only the setup wizard.

## Decisions

Made while writing the spec, from the brief and the code; none needed the user.

- Q: Is "last seen" kept per browser or per install? → A: Per install, on the server, so a
  user who reads the notes on their phone is not shown them again on their desktop.
- Q: What does an install see the first time it runs a build with this feature (a new
  install, or an upgrade from a build without it)? → A: Nothing pops up: every entry in that
  build counts as seen. The full changelog is still one click away. From then on each
  upgrade shows only the entries that build added.
- Q: How is an entry recognised as already seen, given edge builds add entries without a
  version bump? → A: By the entry itself (its text), not by its section, so an entry that
  moves from Unreleased to a release section on release is not shown again.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See what changed after an upgrade (Priority: P1)

After the daemon is upgraded, the next time the user opens the web UI (signed in) a "What's
new" panel lists the changelog entries the user has not seen, grouped by release and by kind
(Added, Fixed, Changed). Closing the panel marks them seen; it does not appear again until a
later upgrade brings new entries.

**Why this priority**: it is the feature: users learn about new pages, settings and fixes
without reading the repository.

**Independent Test**: run a build, open the UI (no panel: first run). Run a build whose
changelog has two more entries, open the UI: the panel lists exactly those two. Close it and
reload: no panel.

**Acceptance Scenarios**:

1. **Given** an install that has seen the entries of build A, **When** it runs build B whose
   changelog adds three entries, **Then** the next page load shows a panel with exactly those
   three entries, under their release heading and kind.
2. **Given** the panel is shown, **When** the user closes it, **Then** those entries are
   recorded as seen and a reload shows no panel.
3. **Given** the panel was closed in one browser, **When** the user opens the UI in another
   browser, **Then** no panel is shown.
4. **Given** no new entries, **Then** no panel is shown and the page loads as before.
5. **Given** a release renames Unreleased to `0.2.0` with the same entries, **When** the
   install upgrades to it, **Then** no panel is shown for entries already seen.
6. **Given** the first run of a build with this feature (no record of seen entries), **Then**
   no panel is shown and every entry of that build is recorded as seen.

---

### User Story 2 - Read the whole changelog any time (Priority: P2)

A "What's new" link (next to the version on the Status page, and in the panel) opens a page
with the full changelog built into the running binary, newest first, with the running
version named. Entries added since the last upgrade are marked new until they are seen.

**Why this priority**: lets users look back, and is the fallback for anyone who closed the
panel too quickly or was on a first run.

**Independent Test**: open the What's new page; it shows every release section of the
built-in changelog in order, with the Unreleased section labelled as the build's own
changes, and links and code in entries rendered.

**Acceptance Scenarios**:

1. **Given** the What's new page, **Then** every section and entry of the built-in changelog
   is shown, newest first, grouped by kind.
2. **Given** an entry with a link or inline code, **Then** the link is a working link and the
   code is shown as code; nothing in an entry is run as page markup.
3. **Given** the Status page, **Then** the version line links to the What's new page.

### Edge Cases

- An entry's wording is edited in a later build: it counts as a new entry and shows again.
  This is acceptable; edits to shipped entries are rare.
- An entry is removed from the changelog: it is never shown; its seen record does no harm.
- The changelog has no sections or no entries (a broken or empty file at build time): the
  panel never shows, the page says there are no notes, and nothing fails.
- The daemon is in setup mode (013): no What's new panel or page; the first run after setup
  records the entries as seen, as any first run does.
- Two browsers close the panel at the same moment: both record the same entries; the result
  is the same as one close.
- The panel would list many entries (for example after skipping many builds): it shows the
  newest 20 and says how many more there are, with a link to the full page.
- The seen record can't be read or written (database error): the UI shows no panel and the
  page still shows the changelog; the error is logged.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The binary MUST carry the repository's `CHANGELOG.md` as it was at build time,
  so every install (Docker, snap, Homebrew, binary) has the same notes as its build without
  any file on disk.
- **FR-002**: The system MUST read the built-in changelog into release sections (Unreleased
  and each version with its date), each with kinds (Added, Fixed, Changed, and any other
  `###` heading) holding entries, in file order. Entries that span several lines MUST be
  kept whole.
- **FR-003**: Each entry MUST have an identity derived from its text alone (not its section
  or position), so the same entry in a later build, or moved into a release section, has the
  same identity.
- **FR-004**: The install MUST keep a record of the entry identities the user has seen, in
  its own state that survives restarts and upgrades, shared by every browser.
- **FR-005**: When there is no record yet, the system MUST record every entry of the running
  build as seen and report nothing new.
- **FR-006**: The signed-in web UI MUST, on load, ask which entries are new (in the build,
  not in the record) and, if any, show a panel listing them grouped by release and kind,
  newest first, up to 20, with a count of any more and a link to the full page.
- **FR-007**: Closing the panel MUST record the entries the panel was opened for as seen.
- **FR-008**: The web UI MUST offer a What's new page with the full built-in changelog,
  newest first, naming the running version, with new entries marked.
- **FR-009**: Entries MUST be shown with their links, inline code and bold text rendered, and
  every other character shown as text; no part of an entry is interpreted as page markup.
- **FR-010**: The changelog and the new/seen calls MUST require the same sign-in as the rest
  of the UI's data, and MUST NOT be served in setup mode.
- **FR-011**: The Status page's version MUST link to the What's new page.
- **FR-012**: The Docker image build MUST include the changelog in the build context so the
  binary carries it (it already copies it into the image).

### Key Entities

- **Release section**: a heading of the changelog: Unreleased or a version, with an optional
  date; holds kinds in file order.
- **Entry**: one bullet of a kind, its text (which may hold links, code and bold), and its
  identity.
- **Seen record**: the set of entry identities this install has shown to the user, with when
  each was recorded.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After an upgrade that adds entries, the first page load shows exactly the added
  entries, and none of the earlier ones, in 100% of tested upgrade paths (edge to edge, edge
  to release, release to release).
- **SC-002**: After the panel is closed once, no browser shows it again for those entries.
- **SC-003**: A first run (new install or first build with the feature) shows no panel.
- **SC-004**: The full changelog is reachable in one click from the Status page.
- **SC-005**: Opening the UI with no new entries takes no visible extra time and shows
  nothing extra.

## Assumptions

- `CHANGELOG.md` keeps the Keep a Changelog layout described in Context; other text (the
  introduction, link references) is not shown as entries.
- The old per-crate changelogs (before October 2026) are not part of the built-in notes.
- One install has one user as far as "seen" goes (the UI has one sign-in today); 019's
  sign-in options may later make it per user, which is out of scope here.
- The panel and page are part of the existing no-build web UI and must keep its bundle under
  the size cap the UI test enforces.
- A CLI command for the notes is out of scope; the GitHub release notes are unchanged.
