# Feature Specification: Usage Profile in the Setup Wizard

**Feature Branch**: `feat/056-usage-profile-wizard`

**Created**: 2026-11-26

**Status**: Draft

**Input**: User description: "Roadmap feature 056 'Usage profile in the setup wizard'. Goal: a
new user is set up and syncing in under 60 seconds: language, setup link with the code in the
URL fragment, Put.io device-code link, and 'how will you use putio-sync?' (kinds and arrival
methods), then a review screen with Start syncing; 'I'll set it up myself' keeps today's
step-by-step wizard." The full brief is the 056 entry in `ROADMAP.md`.

## Clarifications

### Session 2026-11-26

- Q: putio-sync supports only Radarr and Sonarr; how does 056 handle Lidarr? → A: Radarr and
  Sonarr only. Music arrives by hand or through the watch folder; Lidarr support is its own
  roadmap item.
- Q: Does re-running the profile from Settings ship in the same PR as the wizard? → A: Yes, one
  draft PR for the wizard and Settings › Usage profile.
- Q: Does "I'll set it up myself" keep metrics in its Extras step? → A: Yes; the manual path stays
  exactly as 013 has it. Only the profile path skips metrics.

## Context

- Feature 013 added setup mode: with no usable config the daemon prints a one-time setup code
  and serves a five-step wizard (Put.io, download folder, what to download, extras, review).
  Each step asks the user to decide something; a new user has to know what folder rules,
  presets and the release pre-check are before anything syncs.
- Feature 015 added the configuration assistant: previews that show what a change would do,
  and warnings for risky combinations such as deleting from Put.io while the download folder
  can be cleared.
- Feature 018 finds Radarr and Sonarr on the network; 049 sets them up (download client, path
  mapping) and lists Completed Download Handling and Import Extra Files as settings to turn on.
- Feature 042's release pre-check turns away releases whose largest file is under 50 MB by
  default, which also turns away music, ebook and software releases.
- Feature 051 translated the web UI; the language is chosen per browser.
- Feature 025 fetches subtitles; Put.io's own subtitles need no key.
- Hot reload (#83–#86) applies most settings without a restart.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Set up from four answers (Priority: P1)

A new user opens the setup link the daemon printed, picks a language, links Put.io with the
device code, ticks what they download and how it arrives, checks the review screen and presses
**Start syncing**. The daemon saves a config built from those answers and starts syncing.

**Why this priority**: this is the feature: a working setup without knowing putio-sync's
settings.

**Independent Test**: start a daemon with no config against a mocked Put.io whose account has
top-level `Movies` and `TV` folders; answer the four screens; press Start syncing; the saved
config has a video rule on each folder, the pre-check, subtitles and delete settings the
profile sets, and the daemon leaves setup mode.

**Acceptance Scenarios**:

1. **Given** the daemon is in setup mode, **When** the user opens the printed setup link,
   **Then** the wizard is unlocked without typing the code, and the code no longer shows in
   the address bar.
2. **Given** the first screen, **When** the user picks a language, **Then** the wizard
   switches to it at once, the choice is kept as the UI language, and every later screen has a
   language switcher.
3. **Given** a linked Put.io account with top-level folders `Movies`, `TV Shows` and `Music`,
   **When** the usage screen opens, **Then** Movies, TV and Music are ticked and each says
   which folder it was found in.
4. **Given** the usage screen, **When** the user presses Next, **Then** a review screen lists
   each setting the profile will set with a one-line reason, and each can be changed or turned
   off there.
5. **Given** the review screen, **When** the user presses Start syncing, **Then** the config
   is saved, the API token is shown once (as today), and the daemon starts.
6. **Given** a detected download folder that passes the 013 check, **When** the user goes
   through the wizard, **Then** no download folder step is shown; when it fails the check, the
   folder step is shown before the review.
7. **Given** the review screen, **When** the user opens the dry-run link, **Then** the 013 dry
   run report is shown there, without a separate step.

---

### User Story 2 - Radarr and Sonarr are found and set up (Priority: P1)

A user whose downloads arrive from Radarr or Sonarr sees them found and ticked on the usage
screen, enters each app's API key, and at Start syncing each is given putio-sync as its
download client. After start, the folder mapping check and the two recommended settings are
shown for each app, to fix or turn on with one click.

**Why this priority**: *arr users are the main users of the daemon, and a wrong download client
or path mapping is the most common setup failure.

**Independent Test**: with mocked Radarr and Sonarr reachable, the usage screen lists both as
found and ticked; with API keys entered, Start syncing saves `[arr]` on with a generated
password and both apps remembered, and each mock received a download client.

**Acceptance Scenarios**:

1. **Given** Radarr is reachable, **When** the usage screen opens, **Then** Radarr is listed
   as found, ticked, with a field for its API key, and Movies is ticked.
2. **Given** Radarr is ticked with a working key, **When** Start syncing is pressed, **Then**
   `[arr]` is on with a generated password, Radarr has putio-sync as its download client, and
   Radarr is saved as an application.
3. **Given** a ticked app whose key is wrong, **When** the review screen is shown, **Then** it
   says the app can't be set up and why, before anything is saved.
4. **Given** the daemon has started after a profile with Radarr, **When** the start screen
   finishes, **Then** it shows Radarr's folder mapping result and Completed Download Handling
   and Import Extra Files, each with a button to fix or turn on; nothing is changed in Radarr
   without that click.

---

### User Story 3 - The profile picks settings that fit what is downloaded (Priority: P1)

What the user ticks decides folder rules, pre-check sizes, subtitles, the watch folder and
deleting from Put.io, so that music, ebook and software users get downloads at all and video
users get subtitles.

**Why this priority**: today's defaults block software and turn away small releases; a profile
that keeps those defaults leaves some users with nothing downloaded.

**Independent Test**: unit tests of the profile: given answers, the resulting config is as the
table in FR-010 to FR-016 says.

**Acceptance Scenarios**:

1. **Given** Software is ticked and found in `/Software`, **When** the profile is applied,
   **Then** `/Software` has the Everything preset, so `exe`, `msi`, `dmg`, `pkg`, `apk` and
   `jar` files download there, and the pre-check is off for what arrives for software.
2. **Given** Music and Ebooks are ticked with a watch folder, **When** the profile is applied,
   **Then** the watch folder's pre-check minimum is the smallest of the ticked kinds' sizes
   (1 MB), not 50 MB.
3. **Given** an *arr app imports the files, **When** the review screen opens, **Then** "Delete
   from Put.io after download" is recommended on, shown with the 015 warning when the folder
   is risky, and empty-folder cleanup is on with it.
4. **Given** no *arr app, **When** the review screen opens, **Then** delete after download is
   recommended off ("Put.io is your library").
5. **Given** a video kind is ticked and the UI language is German, **When** the profile is
   applied, **Then** subtitles want German then English, download-only, and the music, ebook
   and software folders are excluded from subtitles.

---

### User Story 4 - "I'll set it up myself" (Priority: P2)

A user who wants to decide each setting chooses "I'll set it up myself" on the usage screen
and gets today's step-by-step wizard.

**Why this priority**: keeps the existing path for experienced users; small to build.

**Independent Test**: choosing it shows the 013 steps (download folder, what to download,
extras, review), and finishing saves the config as 013 does, with no profile settings applied.

**Acceptance Scenarios**:

1. **Given** the usage screen, **When** the user chooses "I'll set it up myself", **Then** the
   013 step-by-step wizard is shown, starting at the download folder.

---

### User Story 5 - Run the profile again from Settings (Priority: P3)

Later, the user opens the usage profile in Settings, changes the answers (for example ticks
Music), sees the changes it would make as 015 previews do, and applies them. Settings the user
changed by hand since are left as they are and listed as kept.

**Why this priority**: useful after setup, but not needed to reach the 60-second goal.

**Independent Test**: save a profile, change a profile-set value by hand, run the profile
again with one more kind; the preview lists the new kind's settings, lists the hand-changed one
as kept, and applying leaves it unchanged.

**Acceptance Scenarios**:

1. **Given** a config saved from a profile, **When** the user opens Settings › Usage profile,
   **Then** the saved answers are shown.
2. **Given** the user changed a pre-check size the profile set, **When** the profile is run
   again, **Then** the preview lists that setting as kept and applying does not overwrite it.
3. **Given** an applied change, **Then** it applies live where the setting allows, and the
   preview says which ones wait for a restart.

### Edge Cases

- The setup link's code has expired or is wrong: the wizard shows the code field with
  "That code is not right" and removes the bad code from the address.
- A kind is ticked but no top-level Put.io folder matches: its rule goes on the folder its
  arrivals use (the *arr category folder) or on a new folder named after the kind (for example
  `/Software`), and the review says "Put files for this in /Software".
- Two kinds match the same folder (`Media`): the folder is given to neither by name; the user
  picks on the review screen.
- "Other" is ticked: no rule is added; the account keeps the default blocked extensions.
- No kind is ticked: Next is disabled with "Tick at least one".
- Discovery is slow or finds nothing: the usage screen shows without it after a few seconds and
  lists found apps as they come; apps can be added by address.
- Put.io has no app ID for the device link: the 013 app-ID and paste-token fallbacks stay.
- The Tailscale socket is found: the review mentions Tailscale identity sign-in, with a link to
  Settings; nothing is turned on.
- A video kind is ticked: the review mentions DLNA, with a link to Settings; nothing is turned
  on.
- Lidarr: not set up by 056 (Radarr and Sonarr only); music arrives by hand or the watch
  folder.

## Requirements *(mandatory)*

### Functional Requirements

**Wizard flow**

- **FR-001**: The first wizard screen MUST be the language choice, listing the 051 languages,
  starting on the browser's language; a choice MUST switch the wizard at once and be saved as
  the UI language. Every later screen MUST keep a language switcher.
- **FR-002**: The daemon's console MUST print a setup link with the code in the URL fragment
  (`http://host:port/setup#code=…`), also when the code is replaced. Opening it MUST unlock the
  wizard without typing the code and MUST remove the code from the address bar and history.
  The code MUST NOT be sent in a query string or logged.
- **FR-003**: The Put.io step MUST be the 013 device-code link, with the 013 fallbacks.
- **FR-004**: The usage screen MUST ask what is downloaded (movies, TV, music, ebooks and
  audiobooks, software, other) and how it arrives (each Radarr or Sonarr found by 018 discovery,
  added by hand on Put.io, a watch folder). Kinds MUST be pre-ticked from the account's
  top-level folder names; found *arr apps MUST be pre-ticked and tick the kind they serve; the
  watch folder MUST be pre-ticked when the detected one (`/watches` in a container) exists.
- **FR-005**: The usage screen MUST offer "I'll set it up myself", which shows today's 013
  steps unchanged (download folder, what to download, extras with metrics, review).
- **FR-006**: After the usage screen the wizard MUST show a review screen listing every
  setting the profile sets, each with a one-line reason, each changeable or switchable off,
  ending with **Start syncing**.
- **FR-007**: The download folder step MUST be shown only when the detected folder fails the
  013 check; the dry run MUST be a link on the review screen.
- **FR-008**: The review screen MUST mention DLNA for video profiles and Tailscale identity
  sign-in when a Tailscale socket is found, as links to Settings, turning nothing on.
- **FR-009**: Metrics, DLNA, Tailscale, notifications, hooks and plugins MUST NOT be asked in
  the profile path. The manual path (FR-005) keeps its 013 Extras step, metrics included.

**What the answers set**

- **FR-010 Folder rules**: each ticked kind MUST get a folder rule with its type preset (movies
  and TV: video; music and audiobooks: audio; ebooks: ebooks; software: everything) on the
  Put.io folder the kind was found in. A software rule's preset MUST let the default blocked
  extensions through.
- **FR-011 Radarr/Sonarr**: when an app is ticked, `[arr]` MUST be on with a generated
  password, and each ticked app MUST be set up through the 018/049 flow (download client, saved
  as an application). After start, the 049 folder mapping and the Completed Download Handling
  and Import Extra Files recommendations MUST be shown for each, changed only on the user's
  click.
- **FR-012 Release pre-check**: sizes MUST be set per *arr category and per watch folder for
  each kind: video 50 MB (the default), music 5 MB, ebooks and audiobooks 1 MB, software off.
  A watch folder serving several kinds MUST use the smallest ticked size, and off when software
  is ticked.
- **FR-013 Delete from Put.io after download**: MUST be recommended on when an *arr app imports
  the files and off otherwise, with the reason. Turning it on MUST show the 015 warnings for
  the chosen folder and MUST turn empty-folder cleanup on with it.
- **FR-014 Subtitles**: when a video kind is ticked, subtitles MUST want the UI language, plus
  English when it differs, in download-only mode; the music, ebook and software folders MUST be
  excluded. OpenSubtitles, transcription and translation MUST stay off.
- **FR-015 Watch folder**: when ticked, MUST be on with the detected folder (or the one the
  user enters).
- **FR-016 Defaults**: workers, verify, existing files, retries, bandwidth, stall checks,
  storage free-up, scheduled doctor, logging and the release check MUST stay at their defaults.

**Saving and running again**

- **FR-017**: Start syncing MUST save the config in one write, as 013 finish does, including
  the answers.
- **FR-018**: The answers MUST be saved with the config, so Settings can show them.
- **FR-019**: Settings MUST let the user run the profile again with changed answers, preview
  the differences as 015 does, and apply them, without overwriting settings changed by hand
  since the profile set them. Applied changes MUST go through the normal settings save, so they
  apply live where hot reload allows.
- **FR-020**: All new UI text MUST be translatable (051) and present in every locale.

### Key Entities

- **Usage profile (answers)**: the kinds ticked, how files arrive (*arr apps, by hand, watch
  folder), the UI language, and the values the profile set, so a later run can tell which
  settings the user changed by hand.
- **Recommendation**: one setting the profile would set, with its config key, value, a
  one-line reason, and whether the user switched it off.
- **Kind**: movies, TV, music, ebooks and audiobooks, software, other; each with a type preset,
  pre-check size, whether it gets subtitles, and the folder names it is recognised by.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new user with a container install and detected folders goes from opening the
  setup link to syncing in under 60 seconds, on four screens plus the review.
- **SC-002**: The profile path asks for no setting outside the four questions; every other
  setting is detected or recommended.
- **SC-003**: For each kind, the saved config lets a typical release of that kind download (no
  blocked extension or pre-check minimum turns it away).
- **SC-004**: Running the profile again changes no setting the user changed by hand.

## Assumptions

- Kind folder names are matched case-insensitively against common names: Movies/Films;
  TV/TV Shows/Series/Shows; Music; Books/Ebooks/eBooks/Audiobooks; Software/Apps/Games; and the
  *arr category folders (`radarr`, `sonarr`).
- Audiobooks share the ebooks kind (one tick, "Ebooks and audiobooks"); their folder gets the
  audio preset when its name says audiobooks, else the ebooks preset.
- The UI language stays a per-browser choice (051); the wizard also sends it to the daemon so
  subtitle languages can use it.
- Start syncing is a setup-mode save before the daemon starts, so it needs no live apply; the
  Settings re-run (FR-019) uses the hot-reload save path.
- The folder mapping and recommendations (049) run after start, against the normal API, because
  the 049 checks need the running daemon's download client.
