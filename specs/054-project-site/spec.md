# Feature Specification: Project Website on GitHub Pages

**Feature Branch**: `054-project-site`

**Created**: 2026-11-09

**Status**: Draft (spec only; not started)

**Input**: "Our readme should be more polished as well and we might want to spec having a
github pages site." The README now opens with an overview (see CHANGELOG), but it is still a
2,800-line manual. Stores (052) and the release (053) link to the repository; a site gives
them, and search engines, a page made for someone deciding whether to install.

## Context

- The repository is public at `github.com/devclinton/putio-sync`, MIT licensed, with tagged
  releases, a snap, `.deb` files, installers, container images and home-server store files.
- User docs live in the repository as Markdown: README (overview and manual), INSTALLATION,
  PLUGINS, EVENTS, CHANGELOG and `packaging/STORES.md`. They must stay the single source;
  the site renders them and never holds a second copy.
- There are no screenshots of the web UI anywhere yet. Every store listing asks for them.
- GitHub Actions minutes were cut on pushes to main (5f56486); the site must not add a build
  to every push.
- putio-sync is not affiliated with Put.io; the site says so on every page and uses no
  Put.io logo or branding.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Decide and install from the landing page (Priority: P1)

Someone who found putio-sync in a store, a list or a search opens the site, sees what it does
and what it looks like, picks their platform and copies an install command.

**Why this priority**: it is the reason for the site; the README cannot be a landing page.

**Independent Test**: open `https://devclinton.github.io/putio-sync/` on a phone and a
desktop; in under a minute a new reader can say what it does and reach the install steps for
Docker, Unraid, snap, `.deb`, macOS and Windows.

**Acceptance Scenarios**:

1. **Given** the landing page, **When** it loads, **Then** it shows a one-line pitch, the
   "not affiliated with Put.io" line, three to five screenshots, the highlights and an
   install picker by platform, with the latest release version.
2. **Given** the install picker, **When** the reader picks a platform, **Then** it shows that
   platform's command or download link and a link to the full instructions.
3. **Given** a phone at 375 px wide, **When** the page loads, **Then** nothing scrolls
   sideways and every command can be copied with one tap.

### User Story 2 - Read the docs on the site (Priority: P1)

A user reads the manual, installation guide, plugin and event docs on the site, with
navigation and search, instead of one long README.

**Why this priority**: the docs are large; a sidebar and search make them usable, and the
store listings need stable doc links.

**Independent Test**: the docs section has a page per README chapter plus INSTALLATION,
PLUGINS, EVENTS and CHANGELOG; searching "watch folder" finds the watch folder section.

**Acceptance Scenarios**:

1. **Given** a change to README.md on main, **When** the site is next published, **Then**
   the docs show it with no hand edit to any site file.
2. **Given** a doc link in a store listing (for example `/docs/installation.html#unraid`),
   **When** the docs are rebuilt, **Then** the link still resolves.

### User Story 3 - Screenshots that stay current (Priority: P2)

The maintainer regenerates the screenshots with one command when the UI changes, and the
same images serve the site, the README and the store listings.

**Why this priority**: hand-taken screenshots go stale; stores (Umbrel, CasaOS, Unraid, the
Snap Store) all want them.

**Independent Test**: one command starts the daemon against fake data, captures the
Dashboard, Transfers, Watches, Settings and Stats pages in light and dark at desktop and
phone width, and writes them under `site/screenshots/`.

**Acceptance Scenarios**:

1. **Given** the command, **When** it runs, **Then** no real Put.io account or token is used
   and no real file names, IPs or hostnames appear in the images.

### User Story 4 - The landing page in the main languages (Priority: P3)

A reader whose browser prefers Spanish, Chinese, Hindi, French or Arabic gets the landing
page in that language (Arabic right to left); the docs stay English.

**Why this priority**: the web UI speaks 27 languages and the snap description has these
five; the landing page is short enough to translate, the manual is not.

**Independent Test**: `/es/` shows the Spanish landing page, with a language switch on every
landing page.

### Edge Cases

- A release is tagged while Pages is down: the release still finishes; the site is
  published again on the next run or by hand.
- A README heading is renamed: the old anchor breaks. Store listings link only to page
  level or to anchors listed in research R4, which the build checks.
- JavaScript is off: the page still shows every install option (the picker only filters).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The site MUST be served by GitHub Pages from the repository, built and
  deployed by a GitHub Actions workflow, not from a committed build output branch.
- **FR-002**: The workflow MUST run on release tags, on `workflow_dispatch`, and on pushes to
  main only when `site/`, the doc Markdown files or the workflow change.
- **FR-003**: The docs MUST be generated from the repository's Markdown files, with no copy
  of their text kept under `site/`.
- **FR-004**: The landing page MUST show the latest release version and link its downloads;
  the version MUST come from the build (tag or Cargo version), not be typed by hand.
- **FR-005**: Every page MUST say putio-sync is an independent project, not affiliated with
  Put.io, and MUST NOT use Put.io's logo or marks.
- **FR-006**: The site MUST load nothing from third parties: no analytics, trackers, web
  fonts or CDNs. Everything it needs is served from the Pages origin.
- **FR-007**: The site MUST work with light and dark color schemes, at phone width, and
  pass an automated accessibility check (no critical axe findings) and a link check.
- **FR-008**: Screenshots MUST be produced by a script from fake data (US3) and committed
  as optimized images; the README and `packaging/STORES.md` MUST use the same files.
- **FR-009**: The README MUST link to the site near the top, and the snap, Umbrel, CasaOS
  and Unraid listings MUST use the site as their website URL once it is live.
- **FR-010** (P3): The landing page MAY be translated into Spanish, Chinese, Hindi, French
  and Arabic, machine translation marked as such, following the 051 approach.

### Key Entities

- **Landing page**: hand-written HTML and CSS under `site/`, short, with screenshots.
- **Docs book**: generated from the Markdown files at build time, published under `/docs/`.
- **Screenshot set**: images under `site/screenshots/`, made by the capture script.

## Success Criteria *(mandatory)*

- **SC-001**: The landing page is under 300 KB transferred on first load, screenshots
  included (lazy-loaded below the fold), and scores 95+ for performance and accessibility in
  Lighthouse on mobile.
- **SC-002**: Publishing takes under 5 minutes of Actions time per run, and pushes to main
  that touch no docs run nothing.
- **SC-003**: Changing a doc needs no change under `site/`.
- **SC-004**: All store listings and the README point to the site.

## Assumptions

- The site lives at `https://devclinton.github.io/putio-sync/`. A custom domain can come
  later and needs only a `CNAME` and DNS.
- Pages is free for this public repository.
- English is the source language for all content.

## Out of Scope

- A blog, news feed or comments.
- Versioned docs per release (the docs track main; CHANGELOG covers older versions).
- A hosted demo of the web UI.
