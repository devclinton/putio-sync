# Feature Specification: Configuration Assistant

**Feature Branch**: `015-config-assistant` (built on `claude/objective-bell-6f6977`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "015 Configuration assistant. Settings explain what each option
does and what it affects. They validate values before saving, and preview the change against
the current config. They warn about risky combinations (for example deleting from Put.io after
download with no local copy). They can also reset a section to defaults and import or export
the config. Builds on 013 (setup wizard, config validation and writing in
putio-sync-core/src/setup.rs and Config::save_keeping_file_credentials) and the existing
Settings page groups; 013 covers first-run setup, 015 covers changing an existing config."

## Context

- The Settings page (004, extended by 009, 018, 040, 042) shows one card per config group:
  Download, Torrent watcher, Put.io cleanup, Logging, Notifications and Release check. Each
  field has a one-line hint. Save sends the whole group; the server validates the whole
  config and either refuses it (an error under the form) or writes it to the config file and
  replies with a message saying which changed fields apply now and which need a restart.
- Before a save, the user cannot see what the save will change, whether it will be refused,
  or which changes will need a restart. The hint says what a field does, not what it affects.
- Some combinations are valid but dangerous. The clearest is deleting files from Put.io after
  download while the download folder is somewhere the files will not survive (a temporary
  folder, or a container path that is not on a mounted volume): the only copy is lost.
- The doctor (010) already reports unknown keys and one risky value (`skip_existing = false`)
  in the config file.
- 013 added the first-run setup wizard and `putio-sync setup`. They create a config; 015 is
  about changing one that already works. 015 does not change the wizard.
- There is no way to back up the settings, move them to another install, or put a section
  back to its defaults from the UI.

## Clarifications

### Session 2026-11-03

- Q: Do import and export carry credentials? → A: No. The Put.io login, the API token, and
  the Radarr/Sonarr section (which holds passwords and API keys and has its own card) are
  never exported, and in an imported file they are ignored and listed as ignored.
- Q: Does an import replace the whole config or merge? → A: Merge: each setting in the file
  replaces the current one; settings the file leaves out stay as they are. A full export
  re-imported therefore restores every exported setting.
- Q: Does "reset to defaults" save at once? → A: No. It fills the card's form with the
  defaults and shows the preview; nothing changes until the user saves.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See what a change will do before saving (Priority: P1)

A user edits a settings card. As they change fields, the card shows a preview: each setting
that would change, its current and new value, and whether it applies now or after a restart.
If the values would be refused (for example a watch folder that does not exist while the
watcher is on), the reason shows before they press Save, and nothing is saved.

**Why this priority**: it is the core of the assistant: no surprise refusals, no surprise
restarts, and a clear record of what is about to change.

**Independent Test**: change Workers from 4 to 8 on the Download card; the preview lists
`workers 4 → 8, applies from the next download`. Enable the watcher with a missing folder;
the preview shows the refusal reason and the config file is unchanged.

**Acceptance Scenarios**:

1. **Given** the Download card, **When** the user changes Workers and Bandwidth limit,
   **Then** the preview lists exactly those two settings with old and new values and when
   each applies, and the config file is not changed.
2. **Given** a value the server would refuse, **When** the preview runs, **Then** the card
   shows the same reason a save would give, and nothing is written.
3. **Given** a form with no changes, **When** the preview runs, **Then** it says nothing
   would change.
4. **Given** a preview, **When** the user saves, **Then** the save behaves as today (same
   validation, same message), so a preview never changes what a save does.

---

### User Story 2 - Warnings about risky combinations (Priority: P1)

When settings combine in a way that can lose data or cause trouble, the user is warned, in
the preview of the change that causes it, at the top of the Settings page for the current
config, and in the doctor.

**Why this priority**: the tool deletes remote data; a warning before the only copy is lost
is worth more than any other help text.

**Independent Test**: point the download folder at a temporary folder and turn on "Delete
from Put.io"; the preview warns that files would be deleted from Put.io with no lasting local
copy. Save anyway; the Settings page and the doctor show the same warning.

**Acceptance Scenarios**:

1. **Given** delete-after-download is on and the download folder is in a temporary folder,
   **Then** a danger warning names both settings and says why.
2. **Given** the daemon runs in a container and delete-after-download is on, **When** the
   download folder is on the container's own filesystem rather than a mounted volume,
   **Then** a danger warning says the files are lost when the container is recreated.
3. **Given** a watch folder that is the download folder or inside it, **Then** a warning says
   downloaded `.torrent` files would be uploaded again.
4. **Given** an archive folder that is a watch folder or inside one, **Then** a warning says
   archived `.torrent` files would be uploaded again.
5. **Given** the daily Put.io cleanup is on with a minimum age of 0 hours, **Then** a warning
   says folders a transfer has just created can be removed before files arrive.
6. **Given** more than 10 workers, **Then** a warning says Put.io may throttle or refuse
   requests.
7. **Given** a logging level of trace or debug with the log file on, **Then** a warning says
   the log file grows quickly.
8. **Given** a risky combination, **Then** saving is still allowed: warnings never block.
9. **Given** the default config, **Then** there are no warnings.

---

### User Story 3 - Each setting says what it affects (Priority: P2)

Under each field's hint, the card says what the setting affects (for example "Download speed
and load on Put.io") and whether a change applies now or needs a restart, for this engine.

**Why this priority**: helps users choose values, but the preview already tells them the
outcome of a change.

**Independent Test**: open Settings; every field in the six cards shows an "Affects" line
and an "applies now" or "needs a restart" note that matches what a save says.

**Acceptance Scenarios**:

1. **Given** the Settings page, **Then** each field of the six groups shows what it affects.
2. **Given** a field the engine applies live, **Then** it is marked as applying now with the
   same wording the save message uses (for example "Applies from the next download"); other
   fields are marked as needing a restart.

---

### User Story 4 - Reset a section to its defaults (Priority: P2)

Each card has a "Defaults" button. It fills the form with the default values, and the
preview lists what would change. Nothing is saved until the user presses Save.

**Independent Test**: on the Logging card press Defaults; the form shows level `info` and
the preview lists each setting that differs from the defaults; the file is unchanged until
Save.

**Acceptance Scenarios**:

1. **Given** a card with non-default values, **When** the user presses Defaults, **Then** the
   form shows the defaults and the preview lists the differences.
2. **Given** the user presses Defaults and then reloads the page, **Then** the saved values
   are shown (nothing was saved).

---

### User Story 5 - Export and import settings (Priority: P3)

A "Backup and restore" card exports the settings as a TOML file, without credentials. Import
takes a TOML file (or pasted text), shows the preview of every setting it would change,
warnings for risky combinations, keys it does not know and parts it ignores, and applies it
only when the user confirms.

**Why this priority**: useful for backups and moving installs, but used rarely.

**Independent Test**: export, change Workers, import the exported file: the preview lists
`download.workers` going back; apply; Workers is restored. The exported file contains no
Put.io token, API token, or Radarr/Sonarr secrets.

**Acceptance Scenarios**:

1. **Given** any config, **When** the user exports, **Then** they get a TOML file with every
   setting except the Put.io login, the API token and the Radarr/Sonarr section.
2. **Given** a file with only a `[download]` section, **When** it is imported, **Then** only
   download settings change; every other setting stays.
3. **Given** a file with `[auth]`, `api.auth_token` or `[arr]`, **Then** the preview says those
   parts are ignored, and they are not applied.
4. **Given** a file with an unknown key, **Then** the preview lists it as unknown, as the
   doctor does.
5. **Given** a file that is not valid TOML or a config that would be refused, **Then** the
   reason is shown and nothing is written.
6. **Given** a previewed import, **When** the user applies it, **Then** the config file is
   updated keeping its comments, credentials and the Radarr/Sonarr section, and the message
   says which settings need a restart.

### Edge Cases

- No config file in use (settings held in memory only): preview, warnings, defaults and export
  work; applying an import changes the running settings only, as a save does today.
- The config file was edited by hand since startup: previews compare against the running
  settings, as saves do today.
- The Release check card has overrides per category and watch folder: Defaults clears them.
- A preview request arrives while another save is in progress: the preview reads a
  consistent copy and never writes.
- An import file over 1 MB is refused.
- The download folder does not exist yet: the temporary-folder and container checks look at
  the nearest existing parent.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: For each of the six settings groups the system MUST offer a preview that takes
  the same input as a save and returns, without writing anything: whether the save would be
  accepted (and the refusal reason if not), each changed setting with old and new value, and
  whether each applies now or needs a restart.
- **FR-002**: A preview MUST use the same validation as a save; a value a preview accepts is
  accepted by a save of the same input against the same config, and vice versa.
- **FR-003**: The Settings page MUST run the preview as the user changes a card and show the
  result in the card before Save.
- **FR-004**: The system MUST compute risk warnings for a config, each with a level (danger
  or warning), the settings it involves, and a plain-words explanation. At least the
  combinations in User Story 2 MUST be detected.
- **FR-005**: Warnings MUST appear in each preview (for the config the save would produce),
  at the top of the Settings page (for the running config), and in the doctor's config
  check.
- **FR-006**: Warnings MUST NOT block a save or an import.
- **FR-007**: The system MUST provide, for each group, a description of what each field
  affects and which fields apply without a restart under the running engine; the Settings
  page MUST show both under each field.
- **FR-008**: The system MUST provide each group's default values, and each card MUST have a
  Defaults button that fills the form with them without saving.
- **FR-009**: The system MUST export the settings as TOML, leaving out `[auth]`,
  `api.auth_token` and `[arr]`.
- **FR-010**: The system MUST preview an import (the settings it changes, warnings, unknown
  keys, ignored sections, or the refusal reason) without writing, and apply it only on a
  separate confirmed request.
- **FR-011**: An import MUST merge: settings in the file replace the current ones; settings
  it leaves out stay. Credentials and `[arr]` in the file MUST be ignored.
- **FR-012**: Applying an import MUST write the config file the way saves do today: only
  changed keys are edited, comments and unknown keys stay, and the credentials and `[arr]`
  on disk are untouched.
- **FR-013**: Every new endpoint MUST require the API token, like the other settings
  endpoints, and MUST NOT return tokens, passwords or API keys.

### Key Entities

- **Change**: a setting key (field name within a group, or dotted path for imports), its old
  value and its new value, and whether it applies now.
- **Risk warning**: an id, a level (danger or warning), the setting keys involved, and a
  message.
- **Field help**: per group, per field, what the field affects; per group, the fields that
  apply live and when they take effect.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For every field in the six groups, a user can see before saving whether the
  change would be accepted and whether it needs a restart.
- **SC-002**: Every combination listed in User Story 2 produces a warning in the preview,
  on the Settings page and in the doctor; the default config produces none.
- **SC-003**: Exporting and re-importing the settings on an unchanged install produces a
  preview with zero changes.
- **SC-004**: No exported file or new endpoint response contains a token, password or API
  key.
- **SC-005**: The web UI stays under its 300 KB size cap.

## Assumptions

- The "what it affects" texts live with the server so the web UI stays small and the texts
  can be checked against the live-field lists in tests.
- The temporary-folder check covers the system temp folder, `/tmp` and `/var/tmp`.
- The container check compares the download folder's filesystem with the container's root
  filesystem; it runs only on Unix and only in a container.
- The CLI gets the risk warnings through `putio-sync doctor`; no new CLI commands.
- Out of scope: the setup wizard (013), the guided tour and help panels (014), the
  Radarr/Sonarr card, and settings with no Settings card (`[api]`, `[metrics]`, `[sync]`,
  `[transfers]`, `[doctor]`), which import and export still carry.
