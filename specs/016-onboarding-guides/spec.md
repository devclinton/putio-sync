# Feature Specification: Onboarding guides

**Feature Branch**: `016-onboarding-guides`

**Created**: 2026-11-18

**Status**: Draft

**Input**: User description: "016 Onboarding guides. Use specs/016-onboarding-guides/design.md as the full source."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Set up from scratch on my platform (Priority: P1)

A new user has a put.io account and one box at home: an Unraid server, a TrueNAS SCALE box,
a Raspberry Pi, any machine running Docker, a Mac, a Windows PC or a Linux machine. They open
the docs, pick their platform, and follow a single page from install to the first finished
sync. They also learn how to keep it updated and where to go next. A Guides index sends them
to the right page.

**Why this priority**: every other guide assumes putio-sync is already running. Today the
install page says how to install, but not how to get from there to files arriving.

**Independent Test**: on a clean machine of that platform, follow the guide's steps as
written, without other docs. The reader ends with putio-sync syncing a put.io folder to a
local folder, and knows how it updates.

**Acceptance Scenarios**:

1. **Given** a reader on the Guides index who says "I'm new", **When** they choose their
   platform, **Then** they reach that platform's setup guide.
2. **Given** a clean Unraid server and a put.io account, **When** the reader follows the
   Unraid guide, **Then** putio-sync is running, signed in, and has downloaded a test file
   to the folder they chose.
3. **Given** a guide step that needs a setting's full explanation, **When** the reader
   reaches it, **Then** the step links to the reference page instead of repeating it.
4. **Given** a platform nobody has yet walked through on real hardware, **When** a reader
   opens its guide, **Then** the page says so plainly.

---

### User Story 2 - Add one more piece to a running setup (Priority: P2)

An existing user wants to add something: Radarr and Sonarr (with Prowlarr feeding them),
folder rules and presets, a watch folder, the media server, metrics with Grafana. Or they
want to work out why something isn't working, using the doctor and the Logs page. Each task
guide starts from "putio-sync is running" and ends with that piece working.

**Why this priority**: it covers the most common follow-up questions, and it builds on the
setup guides.

**Independent Test**: on a running install, follow one task guide. The piece works at the
end. For Radarr, a grabbed release goes through put.io, lands on disk and is imported.

**Acceptance Scenarios**:

1. **Given** putio-sync running with Radarr installed, **When** the reader follows the
   Radarr and Sonarr guide, **Then** Radarr lists putio-sync as a working download client
   and imports a test release.
2. **Given** a sync that isn't downloading, **When** the reader follows the troubleshooting
   guide, **Then** the doctor and the Logs page lead them to the cause, or to how to report
   it.

---

### User Story 3 - Switch from a local download client (Priority: P3)

A user who runs qBittorrent, Transmission, Deluge, SABnzbd or NZBGet wants to move to
put.io and putio-sync. Their guide explains what changes: put.io downloads the torrent and
putio-sync brings the finished files home, so there is no local seeding. It then moves
their watch folders, categories and Radarr/Sonarr download client across. Usenet users learn
whether putio-sync replaces their client or runs next to it.

**Why this priority**: valuable, but it assumes the setup and Radarr/Sonarr guides exist.

**Independent Test**: starting from a setup that uses that client with Sonarr, follow the
guide. Sonarr then sends new grabs to putio-sync and they arrive in the same library
folders as before.

**Acceptance Scenarios**:

1. **Given** Sonarr using qBittorrent with category `tv`, **When** the reader follows the
   qBittorrent guide, **Then** Sonarr uses putio-sync, and new episodes land where they did
   before.
2. **Given** a SABnzbd user, **When** they open the usenet guide, **Then** it states plainly
   whether put.io accepts NZBs, and what that means for keeping SABnzbd.

---

### User Story 4 - Find the right guide from inside the web UI (Priority: P4)

While using the web UI, a reader can reach the guide for what they are doing:
- The setup wizard links to the guide for the platform it is running on.
- Settings for Radarr/Sonarr, folder rules, watch folders, the media server and metrics
  link to their task guide.
- The Doctor and Logs pages link to troubleshooting.
- The command palette finds guides by name.

**Why this priority**: it only helps once the guides exist and are published.

**Independent Test**: on each of those pages, the guide link opens the published guide.
In the command palette, typing "grafana" offers the metrics guide.

**Acceptance Scenarios**:

1. **Given** putio-sync running in the Unraid container, **When** the user opens the setup
   wizard, **Then** its "Setup guide" link opens the Unraid guide.
2. **Given** an install whose platform can't be told, **When** the user opens the setup
   wizard, **Then** the link opens the Guides index.
3. **Given** the command palette is open, **When** the user types "unraid", **Then** the
   Unraid guide is offered and opens in a new tab.

### Edge Cases

- A reader on an unsupported setup (for example a 32-bit Raspberry Pi OS). The Pi guide
  says a 64-bit OS is needed before any install step.
- A platform installed in a way the wizard can't tell apart. The wizard falls back to the
  Guides index rather than guessing.
- A UI link to a guide that was renamed or removed. A check fails before release.
- A reader using the docs in another language. Guides are translated like the rest of the
  docs. Where a translation is missing, the English text shows.
- A switching reader who wants to keep seeding. The guide says putio-sync doesn't seed
  locally, and that put.io handles seeding.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The docs MUST have a Guides part, placed right after the introduction, with
  an index page that sends readers to setup, task or switching guides.
- **FR-002**: There MUST be a setup guide for each of: Unraid, TrueNAS SCALE, Raspberry Pi,
  Docker and Compose, macOS, Windows and Linux. Each MUST take the reader from install to a
  first finished sync, and say how the install is kept up to date.
- **FR-003**: There MUST be task guides for:
  - Radarr and Sonarr with Prowlarr
  - folder rules and presets
  - watch folders
  - the media server
  - metrics and Grafana
  - troubleshooting with the doctor and Logs page
- **FR-004**: There MUST be switching guides for qBittorrent, Transmission, Deluge, and
  SABnzbd/NZBGet.
- **FR-005**: Guides MUST link to the reference pages for settings details rather than
  repeating them. UI labels quoted in a guide MUST match the UI.
- **FR-006**: A guide not yet walked through on real hardware MUST say so on the page.
- **FR-007**: Guides MUST be translated into the docs site's languages and pass the site's
  link and accessibility checks.
- **FR-008**: The setup wizard MUST link to the guide for the detected platform, or to the
  Guides index when the platform can't be told.
- **FR-009**: The TrueNAS SCALE app MUST be detectable as TrueNAS, so FR-008 links its
  guide.
- **FR-010**: These settings MUST each link to their task guide:
  - Radarr/Sonarr
  - folder rules
  - watch folders
  - the media server
  - metrics
- **FR-011**: The Doctor and Logs pages MUST link to the troubleshooting guide.
- **FR-012**: The command palette MUST list guides, matched on their title in the UI
  language and in English, and open them in a new tab.
- **FR-013**: An automated check MUST fail when the UI links to a guide that doesn't exist.

### Key Entities

- **Guide**: a docs page with a kind (setup, task or switching), a title, a few search
  words, and whether it has been walked through on real hardware.
- **Platform**: a way putio-sync is installed, which the update checker already detects.
  Each maps to one setup guide.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new user following their platform's guide goes from nothing installed to a
  first file synced in under 15 minutes, not counting download time.
- **SC-002**: Every guide on a platform the maintainer or Claude can run has been walked
  through end to end before it merges. Its PR records who checked each step.
- **SC-003**: Every guide is reachable in two clicks from the docs home, and from the
  matching UI page in one.
- **SC-004**: No broken links: the site's link check and the UI-to-guide check both pass.

## Assumptions

- The docs site from feature 054, with its build, translation and checks, is where guides
  live. The roadmap's "`docs/`" means that site.
- Claude walks through macOS and Unraid, and the maintainer walks through Windows, Linux
  and TrueNAS in VMs. Raspberry Pi stays marked untested until someone tries it.
- Put.io does not take NZB files. Research confirms this before the usenet guide is
  written.
- CasaOS and Umbrel get only a mention on the index for now.
- Guides ship over several PRs: setup, then tasks, then switching. The UI links come last.
