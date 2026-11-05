# Feature Specification: Settings Split into Sections

**Feature Branch**: `046-settings-sections`

**Created**: 2026-11-05

**Status**: Draft

**Input**: User description: "breaking settings into sub tabs or something like that. There are
too many settings in one place and hard to understand."

## Context

- The web UI's Settings page is one long page of about 20 cards, in this order: a warnings
  banner, Appearance, Credentials, Radarr / Sonarr, Download (23 fields plus a pattern preview
  and the bandwidth schedule), Torrent watcher, Put.io cleanup, Put.io storage, Put.io (app ID),
  Tailscale, Network, Event hooks, Plugins, Logging, Notifications, Release check, Folder rules,
  Presets, Recently blocked, and Backup and restore.
- Each settings group is saved on its own (one Save per card). Each card previews a save as
  the operator types, says what each field affects and whether a change applies now or after
  a restart, and has a Defaults button (feature 015). Settings set by an environment variable
  are shown read-only.
- The Download card alone mixes four different jobs: how downloads run, bandwidth, download
  verification, and which files are downloaded at all.
- Other places point into Settings only as "Settings › Plugins" in plain text (doctor
  suggestions, docs, history page) or link to the top of the page (status page, folder rules).
- An unconfigured daemon runs in setup mode, where only the setup wizard is reachable; that
  does not change.

## Clarifications

### Session 2026-11-05

Answered by the implementing session from the brief, taking the recommended option each time
(the session runs without the user at hand); the user can change any of them.

- Q: Does switching sections change the address and Back? → A: Yes. Each switch is a new
  address, so Back returns to the previous section, without reloading the cards.
- Q: Does search cover plugin, hook and Radarr / Sonarr fields, not just the fixed groups? →
  A: Yes: every labelled field on the page once its card has loaded, plus card titles.
- Q: Does a field with a non-default value stay visible while "Show advanced" is off? → A: No;
  the toggle alone decides, so a card looks the same every time. Links, search, unsaved edits
  and warnings on an advanced field turn the toggle on.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Find settings by topic (Priority: P1)

The operator opens Settings and sees a short list of sections (General, Put.io, Downloads,
Rules and filters, Integrations, Network and access, Advanced) instead of one long page. Picking
a section shows only its cards, with one or two plain sentences saying what the section is for.
On a phone the same sections are reachable from a row of buttons at the top of the page.

**Why this priority**: This is the user's request. It alone makes the page readable.

**Independent Test**: Open Settings, pick each section in turn at desktop and phone width, and
check that every card that exists today appears in exactly one section and still saves.

**Acceptance Scenarios**:

1. **Given** Settings is open, **When** the operator picks Downloads, **Then** only the
   Downloads cards are shown, the section's help text is shown above them, and Downloads is
   marked as the current section.
2. **Given** a phone-width screen, **When** Settings opens, **Then** every section can be
   reached without scrolling the page sideways, and the cards fit the width.
3. **Given** the operator has typed into a Downloads field without saving, **When** they switch
   to Put.io and back, **Then** the typed value is still there.
4. **Given** any card, **When** the operator saves it, **Then** saving, validation errors, the
   save preview, Defaults, the "applies now / needs a restart" notes and environment-override
   read-only fields work as before.

---

### User Story 2 - Link straight to a section and a setting (Priority: P1)

Each section has its own address. A link can also name one setting, and following it opens the
section, scrolls to that setting and highlights it. The doctor's suggestions, the warnings
banner, What's new notes, the status page and folder rules link this way, and the docs name
settings by their section.

**Why this priority**: With settings split up, a plain "Settings" link no longer lands near the
right place; links that name the section and setting replace scrolling.

**Independent Test**: Open an address naming the Verification setting; the Downloads section
opens with that setting in view and highlighted. Open a doctor suggestion that mentions
Settings › Integrations › Plugins and follow its link.

**Acceptance Scenarios**:

1. **Given** an address for a section, **When** it is opened (typed, bookmarked or reloaded),
   **Then** that section is shown.
2. **Given** an address naming a setting, **When** it is opened, **Then** its section is shown,
   the setting is scrolled into view, focused and briefly highlighted, even if it is an
   advanced setting.
3. **Given** the old address of the Settings page (no section), **When** it is opened, **Then**
   the General section is shown.
4. **Given** an unknown section or setting in the address, **When** it is opened, **Then**
   General (or the named section) is shown and nothing breaks.
5. **Given** a warning in the warnings banner, **When** the operator follows it, **Then** the
   setting it is about is shown.

---

### User Story 3 - Search for a setting (Priority: P2)

A search box at the top of Settings finds settings by name, help text or card title across all
sections. Picking a result opens its section and jumps to it.

**Why this priority**: Operators often know the word ("verify", "bandwidth", "Plex") but not
which section holds it.

**Independent Test**: Type "crc" and pick the result; the Verify downloads setting is shown.

**Acceptance Scenarios**:

1. **Given** the search box, **When** the operator types two or more characters, **Then** up to
   ten matching settings are listed, each with its section and card.
2. **Given** results, **When** the operator picks one (click, or Enter for the first), **Then**
   its section opens and the setting is shown and highlighted as in User Story 2.
3. **Given** no match, **Then** the list says nothing matched.
4. **Given** a search, **When** the operator presses Escape, **Then** the search clears.

---

### User Story 4 - See where changes and problems are (Priority: P2)

Each section in the list shows a mark when one of its cards has unsaved changes, and a count
when it has problems: a warning from the settings assistant about one of its settings, a card
that failed to load, or a save that was refused.

**Why this priority**: With sections hidden from view, an unsaved edit or a warning in another
section would otherwise be easy to miss.

**Independent Test**: Edit a Downloads field without saving, then open General; Downloads
shows the unsaved mark. Saving clears it.

**Acceptance Scenarios**:

1. **Given** an edited, unsaved card, **Then** its section shows "unsaved" and the card says it
   has unsaved changes; saving it or undoing the edit clears both.
2. **Given** an assistant warning about a setting, **Then** that setting's section shows a
   problem count.
3. **Given** unsaved changes anywhere in Settings, **When** the operator closes or reloads the
   browser tab, **Then** the browser asks before leaving.

---

### User Story 5 - Hide advanced options (Priority: P3)

Rarely changed settings (buffer and chunk sizes, retry delays, scan intervals, module filters
and the like) are hidden until the operator turns on "Show advanced" for that section. The
section says how many are hidden.

**Why this priority**: Fewer fields at once makes each card easier to read, but nothing is lost
if every field stays shown.

**Independent Test**: Open Downloads: Buffer (MB) is hidden; turn on Show advanced and it
appears; reload and the choice is kept in this browser.

**Acceptance Scenarios**:

1. **Given** a section with advanced settings, **Then** a "Show advanced (N)" toggle is shown and
   those settings are hidden until it is on.
2. **Given** a link or search result to an advanced setting, **Then** the toggle turns on.
3. **Given** a hidden advanced setting, **When** its card is saved, **Then** its value is saved
   unchanged.

### Edge Cases

- A card fails to load: its section shows a problem and the card offers Retry, as today.
- Two cards edit parts of the same settings group (the Download group is split into four
  cards): saving one must not undo a save just made in another, and must not save the other
  card's unsaved edits.
- Defaults on a card that holds part of a group fills in only that card's settings.
- A setting whose section is named in an old bookmark (`#/settings`) still opens Settings.
- A field link arrives before its card has loaded: the jump waits for it (up to a few seconds).
- Import from Backup and restore reloads every section.
- The parallel log viewer feature may add a log level setting; it belongs in Advanced ›
  Logging.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Settings MUST be divided into these sections, each card in exactly one:
  - **General**: Appearance, Credentials, Notifications.
  - **Put.io**: Put.io (app ID), Put.io storage, Put.io cleanup, Torrent watcher.
  - **Downloads**: Downloads (workers, folder, retries, order, existing files, delete after
    download), Bandwidth (limit, slow limit, schedule), Verification.
  - **Rules and filters**: File filters (only / never download, blocked extensions, pattern
    preview), Folder rules, Presets, Recently blocked, Release check.
  - **Integrations**: Radarr / Sonarr, Plugins, Event hooks.
  - **Network and access**: Network, Tailscale.
  - **Advanced**: Logging, Backup and restore.
- **FR-002**: The settings assistant's warnings banner MUST stay above all sections, and each
  warning MUST link to the first setting it names.
- **FR-003**: Desktop width MUST show the sections as a side list; phone width MUST show them as
  a row of buttons that scrolls sideways within itself, with no sideways scroll of the page.
- **FR-004**: Each section MUST have one or two sentences of plain-language help.
- **FR-005**: Each section MUST have an address `#/settings/<section>`; an address MAY add
  `?field=<group>.<setting>` (or the id of a field) to jump to one setting.
- **FR-006**: Switching sections MUST change the address (so Back returns to the previous
  section) and MUST NOT reload the cards or lose unsaved input.
- **FR-007**: Settings MUST offer a search over the labels and help text of every labelled field
  on the page (including plugin, hook and Radarr / Sonarr fields once loaded) and card titles,
  with up to ten results that jump to the setting.
- **FR-008**: The section list MUST mark sections with unsaved changes and count problems
  (assistant warnings on its settings, cards that failed to load, refused saves).
- **FR-009**: The browser MUST ask before closing or reloading the tab while any card has
  unsaved changes.
- **FR-010**: Each section with advanced settings MUST offer a "Show advanced" toggle that
  hides them by default; the choice MUST be remembered in this browser. A link or search to an
  advanced setting, or a warning about one, MUST turn it on.
- **FR-011**: Saving, validation, previews, Defaults, affects / applies-now notes, environment
  overrides shown read-only, and Backup and restore MUST behave as before.
- **FR-012**: A card holding part of a settings group MUST save only its own settings, on top
  of the latest saved values of the rest of the group.
- **FR-013**: The doctor's suggestions, the history page, the status page, folder rules and
  What's new notes MUST link to the right section (and setting where one is named).
- **FR-014**: The docs (README, PLUGINS, EVENTS, the example configs) and the CLI and doctor text
  MUST name settings as "Settings › Section › Card".
- **FR-015**: The page code MUST be split into one module per section plus shared parts, with
  no file over about 700 lines.
- **FR-016**: The whole UI MUST stay under the bundle cap, raised from 384 KB to 416 KB for
  this feature (see plan.md, Complexity Tracking).

### Key Entities

- **Section**: id, title, help text, its cards.
- **Card**: a settings group (or part of one) with a title and its fields; some fields advanced.
- **Field address**: `<group>.<setting>`, matching the config key, e.g. `download.verify`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: No section shows more than 6 cards or more than 25 fields with advanced hidden;
  the longest section today (the whole page) has about 20 cards.
- **SC-002**: From any page, an operator reaches a named setting in one click from a link, or
  by typing at most one word into search and picking the first result.
- **SC-003**: Every card that existed before is reachable and saves as before (checked card by
  card against a local daemon).
- **SC-004**: The page works at 375 px wide with no sideways page scroll.
- **SC-005**: The UI bundle stays under 416 KB.

## Assumptions

- The Download settings group stays one group on the server; only the page shows it as four
  cards. No server API changes are needed beyond wording in doctor and CLI messages.
- All sections load when Settings opens, as all cards do today, so search and the change marks
  cover every section.
- Leaving Settings for another page in the app with unsaved changes is not blocked (as today);
  only closing or reloading the tab asks.
- Which settings count as advanced is decided in the plan from today's fields; it can change
  later without changing this spec.
- Setup mode is unchanged: Settings is not reachable until setup is done.
