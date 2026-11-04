# Feature Specification: First-Run Setup Wizard

**Feature Branch**: `013-setup-wizard` (built on `claude/great-turing-febaee`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "013 First-run setup wizard. When the daemon has no Put.io token
or no usable download folder, the web UI opens a step-by-step setup instead of the dashboard:
1. Connect Put.io (OAuth, without copying a token by hand). 2. Choose the download folder, with
checks that it is writable and how much space is free. 3. Choose what to download, using the
folder rule presets from 009. 4. Optional: a watch folder, Radarr/Sonarr, metrics. 5. A dry-run
sync that shows what would be downloaded before anything starts. The CLI gets the same flow as
`putio-sync setup`, for headless installs. It works the same in Docker, the snap and a plain
binary."

## Clarifications

### Session 2026-11-03

- Q: How does the wizard connect Put.io without a hand-copied token? → A: Put.io's device-link
  flow: the wizard shows a short code, the user enters it at put.io/link, and the daemon picks
  up the token. It needs a public Put.io app ID built into the binary; the maintainer supplies
  it later, and config/env can override it. With no app ID the wizard falls back to pasting a
  token.
- Q: How is first-run setup protected when no API token exists yet? → A: The daemon prints a
  one-time setup code to its console output (docker logs, the snap's journal, the terminal);
  the wizard asks for it first. A configured API token also unlocks the wizard. The wizard then
  creates the API token.
- Q: The web UI is near its 260 KB size cap; what about the wizard's size? → A: Raise the cap
  to 300 KB and report the new size.

## Context

- Today a new install has to be configured by hand: run `putio-sync login` (which needs a
  Put.io OAuth client ID, secret and redirect URI in a `.env` file), edit `config.toml` for the
  download folder, set an API token, then start the daemon. `daemon` exits at once when there is
  no Put.io token ("Please run 'putio-sync login' first"), so in Docker the container
  restart-loops and the web UI is never reachable.
- The web UI and API need an API token (`[api] auth_token` or `PUTIO_SYNC_API_TOKEN`); with none,
  every request is refused and the sign-in page says to set one and restart.
- Feature 009 added type presets (Video only, Audio only, Ebooks only, No binaries, Everything)
  that folder rules use. Rules attach to a folder; the account root uses `[download]`'s blocked
  extensions.
- Feature 018 added Radarr/Sonarr discovery (find the *arr apps putio-sync can reach) and
  auto-setup (add putio-sync as their download client, tested before anything is written).
- The doctor (010) can walk the whole Put.io account read-only, with a deadline.
- Install targets: the Docker image (config at `/config/config.toml`, downloads at
  `/downloads`, watch folder at `/watches`), the snap (config under `$SNAP_COMMON`, daemon
  started by the user), and a plain binary (config at `~/.putiosync/config.toml`).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The daemon starts and offers setup instead of exiting (Priority: P1)

A user starts putio-sync for the first time with no Put.io token (or with a download folder that
is missing and cannot be created, or is not writable). Instead of exiting, the daemon starts in
setup mode: it serves the web UI, prints a one-time setup code and the address of the web UI to
its console, and does not sync, watch or download anything. Opening the web UI shows the setup
wizard, which first asks for the setup code (or the API token, when one is configured).

**Why this priority**: Without this, nothing else in the feature is reachable: the daemon exits
and the container restart-loops.

**Independent Test**: Start the daemon with an empty config directory. It keeps running, prints
the code, and the web UI shows the wizard's unlock step; a wrong code is refused, the right one
opens the wizard.

**Acceptance Scenarios**:

1. **Given** no config file, **When** the daemon starts, **Then** it stays running in setup
   mode, prints "Setup needed", the web UI address and a one-time setup code to its console
   output, and logs (without the code) why setup is needed.
2. **Given** a config with a Put.io token and an unusable download folder, **When** the daemon
   starts, **Then** it enters setup mode and the reason names the folder and the problem.
3. **Given** setup mode and no API token configured, **When** a browser opens the web UI,
   **Then** the wizard asks for the setup code; a wrong code is refused with a message and
   counts toward the existing failed-sign-in limit.
4. **Given** setup mode and an API token configured, **When** the user enters the API token in
   the unlock step, **Then** the wizard opens (the setup code also works).
5. **Given** setup mode, **When** any normal API route (files, queue, settings…) is called,
   **Then** it answers "setup needed" and does nothing; health stays available.
6. **Given** a fully configured install, **When** the daemon starts, **Then** it starts
   normally and the wizard is never shown.

---

### User Story 2 - Connect Put.io without copying a token (Priority: P1)

In the wizard's first step the user connects their Put.io account. The wizard shows a short code
and a link to put.io/link; the user opens it (on any device), signs in to Put.io and enters the
code. Within a few seconds the wizard shows "Connected as <username>" with the account's used and
free space. If the device link is unavailable (no app ID built in or configured, or Put.io's link
service fails), the wizard instead offers to paste a token, with a link to where Put.io shows one,
and checks it before going on.

**Why this priority**: The Put.io connection is the core of first-run setup; the roadmap brief
calls out "without copying a token by hand".

**Independent Test**: Against a mocked Put.io, start the link step, have the mock return a token
for the code, and see the account name; paste a bad token and see it refused.

**Acceptance Scenarios**:

1. **Given** the Put.io step, **When** it opens, **Then** it shows a link code and the put.io/link
   address, and waits for Put.io to confirm the code.
2. **Given** the user enters the code on Put.io, **When** Put.io confirms it, **Then** the
   wizard shows the account name and space within 10 seconds, without the user copying anything.
3. **Given** the code expires or Put.io stops answering, **When** the wizard notices, **Then**
   it says so and offers a new code and the paste option.
4. **Given** no app ID is available, **When** the step opens, **Then** only the paste option is
   offered, with a note that the device link needs an app ID.
5. **Given** a pasted token Put.io rejects, **When** the user continues, **Then** the wizard says
   the token was not accepted and stays on the step.
6. **Given** a Put.io token is already configured and valid, **When** the step opens, **Then** it
   shows the connected account and lets the user keep it or connect another one.

---

### User Story 3 - Choose a download folder that works (Priority: P1)

The user picks where downloads go. The wizard suggests a folder (the configured one, else
`/downloads` when it exists, as in Docker, else a folder in the user's home), and checks the
chosen folder: it exists or can be created, files can be written to it, and how much space is
free. It shows free and total space, compares the free space with what is on Put.io, and warns
(without blocking) when space is low.

**Why this priority**: Downloads cannot start without a writable folder, and a wrong folder
inside a container is the most common first-run mistake.

**Independent Test**: Check a writable temporary folder (ok, with space), a read-only one
(refused, with the reason), and a path that does not exist under a writable parent (offered to
be created).

**Acceptance Scenarios**:

1. **Given** a writable folder, **When** the user checks it, **Then** the wizard shows it is
   writable with free and total space.
2. **Given** a folder that is not writable, **When** checked, **Then** the wizard says why
   (permission denied, read-only, not a folder) and does not let the user continue with it.
3. **Given** a missing folder whose parent is writable, **When** checked, **Then** the wizard
   says it will be created; it is created when the user continues.
4. **Given** free space smaller than the space used on Put.io, **When** checked, **Then** a
   warning says the account does not fit, and the user may still continue.
5. **Given** the daemon runs in a container, **When** the step is shown, **Then** it explains
   that the path is the one inside the container (for example `/downloads`).
6. **Given** the check leaves a test file, **When** the check ends, **Then** the test file is
   removed.

---

### User Story 4 - Choose what to download with presets (Priority: P2)

The user picks what kind of files to download for the whole account with one of the 009
presets (Video only, Audio only, Ebooks only, No binaries, Everything), with "No binaries" as the
default. Optionally, they see the top-level folders of their Put.io account and can give any of
them a different preset or skip it.

**Why this priority**: Most users only want media; without this step the first sync could pull
everything, including binaries.

**Independent Test**: Choose "Video only" and give one top-level folder "Skip"; the saved config
has the account-wide preset and one folder rule, and the dry run reflects both.

**Acceptance Scenarios**:

1. **Given** the step, **When** it opens, **Then** it lists the presets with their labels and a
   short description of what each downloads, with "No binaries" selected.
2. **Given** a chosen account-wide preset, **When** setup finishes, **Then** every file not
   covered by a folder rule is filtered by that preset.
3. **Given** the top-level folder list, **When** the user gives a folder another preset or
   "Skip", **Then** setup saves a folder rule for it, the same rule the Settings rules editor
   would create.
4. **Given** existing folder rules in the config, **When** setup finishes, **Then** they are
   kept; the wizard shows them and changes only the folders the user changed.

---

### User Story 5 - See what would be downloaded before anything starts (Priority: P2)

Before finishing, the wizard runs a dry-run sync: it lists what the daemon would download with
the chosen folder and presets — number of files and total size, what is skipped by the presets
or folder rules, and what already exists locally — with a sample of file names. Nothing is
downloaded, deleted or changed on Put.io. The user then chooses "Start syncing" (or goes back to
change a step).

**Why this priority**: The roadmap asks for it, and it is the user's last chance to catch a wrong
folder or preset before a large download starts.

**Independent Test**: With a mocked account of known files and a preset, the dry run reports the
exact counts and sizes, and the mock Put.io sees no write calls.

**Acceptance Scenarios**:

1. **Given** the review step, **When** the dry run runs, **Then** it shows files and bytes to
   download, files and bytes skipped (by reason), and files already present locally.
2. **Given** a large account, **When** the walk takes longer than its time limit, **Then** the
   result says it is partial and shows what was counted so far.
3. **Given** the dry run, **When** it runs, **Then** no download starts, nothing is written to
   the download folder (apart from the earlier folder check) and Put.io receives only read
   requests.
4. **Given** the user chooses "Start syncing", **When** the config is saved, **Then** the
   daemon leaves setup mode and starts syncing without a restart, and the web UI goes to the
   Status page signed in.
5. **Given** the user goes back from review, **When** they change a step, **Then** the dry run
   runs again with the new choices.

---

### User Story 6 - Optional extras: watch folder, Radarr/Sonarr, metrics (Priority: P3)

An optional step offers three extras, each skippable: a watch folder for `.torrent`/magnet files
(checked readable, `/watches` suggested in Docker), Radarr/Sonarr (using 018's discovery and
auto-setup: found apps are listed, the user pastes an API key and putio-sync is added as their
download client), and Prometheus metrics (on/off, address).

**Why this priority**: Useful but not needed to start syncing; each already has a Settings page.

**Independent Test**: Enable a watch folder and metrics in the step; the saved config has them.
With a mocked Radarr, discovery lists it and auto-setup adds the client, as in 018.

**Acceptance Scenarios**:

1. **Given** the extras step, **When** the user skips it, **Then** nothing about watch, *arr or
   metrics changes.
2. **Given** a watch folder path, **When** checked, **Then** the wizard says whether it exists
   and is readable, and saves it with watching turned on.
3. **Given** a discovered Radarr and its API key, **When** the user runs auto-setup, **Then** the
   wizard uses 018's preview and setup (the download client is turned on with a generated
   password when needed) and shows 018's result and failure messages.
4. **Given** metrics turned on, **When** setup finishes, **Then** the metrics endpoint starts with
   the daemon.

---

### User Story 7 - The same setup from the command line (Priority: P2)

On a headless install (SSH, `docker exec`, the snap), `putio-sync setup` walks through the same
steps in the terminal: connect Put.io (device link code, or paste a token), choose and check the
download folder, pick a preset, optional extras (watch folder, metrics), a dry run, then save.
Flags answer steps up front (download folder, preset) and `--yes` accepts the defaults for the
rest, so it can run unattended apart from the Put.io link. A daemon already waiting in setup mode
notices the saved config and starts syncing.

**Why this priority**: The roadmap requires it for headless installs; it shares the engine with
the web wizard.

**Independent Test**: Run `putio-sync setup --download-dir <tmp> --preset video --yes` against a
mocked Put.io with a pasted token; the config is written with the folder, preset and token, and
the dry-run summary is printed.

**Acceptance Scenarios**:

1. **Given** an interactive terminal, **When** `putio-sync setup` runs, **Then** it asks each
   question with the same defaults and checks as the web wizard.
2. **Given** `--download-dir` and `--preset`, **When** setup runs, **Then** those steps are not
   asked.
3. **Given** no terminal and a missing answer with no default (such as the Put.io connection
   without a link code), **When** setup runs, **Then** it stops with a message naming the flag or
   step needed, and writes nothing.
4. **Given** an existing config, **When** setup runs, **Then** it starts from the current values
   and keeps every setting it does not ask about.
5. **Given** a daemon in setup mode using the same config file, **When** the CLI saves a complete
   config, **Then** the daemon leaves setup mode within 15 seconds and starts syncing.
6. **Given** the CLI creates the API token, **When** it finishes, **Then** it prints the token
   once so the user can sign in to the web UI (unless the token comes from the environment).

---

### Edge Cases

- The config file exists but cannot be parsed: setup mode starts, the reason says the file is
  invalid, and the wizard does not overwrite it without saying so (it shows that the file will be
  replaced and keeps a backup copy next to it).
- `PUTIO_SYNC_API_TOKEN` (or the snap's `api-token`) is set: the wizard skips creating an API
  token and says the token comes from the environment.
- The API listener is turned off (`[api] listen = false`): the daemon cannot offer the web
  wizard; it prints that setup is needed and to run `putio-sync setup`, and waits for the config
  as in US7.
- Two browsers open the wizard at once: both may unlock; the one that finishes first wins, and
  the other's "Start syncing" is told setup is already done.
- The setup code is entered wrong repeatedly: the existing failed-sign-in limit (5 per minute per
  address) applies.
- The daemon is restarted mid-setup: a new setup code is printed; nothing was saved, so the
  wizard starts over (the browser keeps no state that skips steps).
- The Put.io token is configured but Put.io rejects it at startup: setup mode, with "Put.io
  token rejected" as the reason; the Put.io step starts empty.
- Put.io is unreachable at startup with an otherwise complete config: the daemon starts normally
  (as today) and retries; setup mode is only for missing or rejected credentials.
- The download folder is on a network share that reports no free space: the wizard shows the
  space as unknown and does not warn.
- The device-link code is entered on Put.io after the user switched to pasting a token: the
  first token received wins; the other attempt is cancelled.

## Requirements *(mandatory)*

### Functional Requirements

**Setup mode**

- **FR-001**: The daemon MUST decide at startup whether setup is needed: no config file, a config
  that cannot be parsed, no Put.io credentials, credentials Put.io rejects, or a download folder
  that does not exist and cannot be created, or is not writable.
- **FR-002**: When setup is needed, the daemon MUST stay running in setup mode and MUST NOT sync,
  watch, download or delete anything until setup completes.
- **FR-003**: In setup mode the daemon MUST serve the web UI and the setup routes on the
  configured API address (default `0.0.0.0:15432`); every other protected route MUST answer
  "setup needed" without acting. Health MUST stay available.
- **FR-004**: When no API token is configured, the daemon MUST generate a one-time setup code at
  each start in setup mode and print it, with the web UI address, to its console output. The
  code MUST NOT be written through the logger (log files, telemetry archive) and MUST stop
  working when setup completes.
- **FR-005**: Setup routes MUST require the setup code or the configured API token. Wrong values
  MUST count toward the existing failed-authentication limiter.
- **FR-006**: When setup completes (from the web wizard or from the CLI saving a complete config
  file), the daemon MUST leave setup mode and start normally in the same process, without a
  restart, on the same address.
- **FR-007**: While in setup mode, the daemon MUST re-check the config file at least every 10
  seconds and leave setup mode when it has become complete (US7).
- **FR-008**: When the API listener is turned off, setup mode MUST say on the console that setup
  is needed and to run `putio-sync setup`, and wait for the config (FR-007).

**Wizard flow (web)**

- **FR-009**: The web UI MUST show the setup wizard instead of the sign-in page and dashboard
  whenever the daemon is in setup mode, and MUST NOT show the wizard when it is not.
- **FR-010**: The wizard MUST have these steps in order: unlock, Put.io, download folder, what to
  download, extras (optional), review (dry run) and finish. The user MUST be able to go back to
  any earlier step; later steps MUST be reachable only once the required earlier ones are done.
- **FR-011**: The wizard MUST NOT write the config file before "Start syncing"; choices are kept
  by the daemon for the setup session.
- **FR-012**: On "Start syncing", the daemon MUST save, in one write, the Put.io token, download
  folder, account-wide preset, folder rules, extras and the API token (when it creates one),
  keeping every other existing setting. File permissions MUST stay owner-only.
- **FR-013**: When no API token is configured (in the file or environment), finishing MUST create
  a random API token, save it, show it once to the user, and sign the browser in with it.

**Put.io**

- **FR-014**: The Put.io step MUST offer the Put.io device link: request a code for the app ID,
  show the code and the put.io/link address, and wait for Put.io to confirm it, checking at most
  every 3 seconds, until the code expires or the user cancels.
- **FR-015**: The app ID MUST come from config (`[putio] app_id`), else the environment
  (`PUTIO_SYNC_APP_ID`), else a value built into the binary. With none, only the paste option is
  offered.
- **FR-016**: The step MUST offer pasting a token, with a link to Put.io's page that shows one.
- **FR-017**: Any token (linked or pasted) MUST be checked against Put.io's account endpoint
  before it is accepted; the step MUST show the account name and its used and free space.
- **FR-018**: Put.io tokens, the setup code and the API token MUST NOT appear in logs, metrics or
  error messages; API responses MUST NOT return the Put.io token. The API token is returned only
  once, in the finish response that creates it.

**Download folder**

- **FR-019**: The folder step MUST suggest a folder: the configured one when set and not the
  built-in default, else `/downloads` when it exists, else `putio-downloads` in the home folder.
- **FR-020**: Checking a folder MUST report: whether it exists, whether it is a folder, whether a
  file can be created and removed in it (or, when missing, whether its nearest existing parent
  is writable so it can be created), and free and total space when the filesystem reports them.
- **FR-021**: The folder check MUST warn, without blocking, when free space is less than the
  space used on Put.io, or less than 10 GiB.
- **FR-022**: A folder that is not writable (and cannot be created) MUST block continuing, with
  the reason.
- **FR-023**: The folder step MUST say that paths are as the daemon sees them, with a container
  note when the daemon runs in a container.

**What to download**

- **FR-024**: The step MUST list the 009 presets (built-in and user-defined) by label with a
  short description, defaulting to "No binaries".
- **FR-025**: The chosen preset MUST apply to the whole account: files not covered by a folder
  rule with its own preset are filtered by it, in place of `blocked_extensions`. A config with no
  account-wide preset MUST behave exactly as today (`blocked_extensions` at the root).
- **FR-026**: The step MUST list the top-level folders of the Put.io account and let the user set
  a preset or "Skip" for each; each choice MUST be saved as a 009 folder rule. Existing rules
  MUST be kept and shown.

**Extras**

- **FR-027**: The extras step MUST be skippable as a whole and per item.
- **FR-028**: Watch folder: the step MUST check that the folder exists and is readable, suggest
  `/watches` when it exists, and save it with watching turned on.
- **FR-029**: Radarr/Sonarr: the step MUST use 018's discovery, preview and auto-setup, with
  018's result and failure messages, including turning on the download client with a generated
  password when needed. *arr changes made here happen at once (as in 018), not at finish.
- **FR-030**: Metrics: the step MUST let the user turn metrics on and set the address, with the
  current default shown.

**Dry run**

- **FR-031**: The review step MUST walk the Put.io account read-only and report, using the same
  rules the download engine uses: files and bytes that would be downloaded, files and bytes
  skipped by preset, folder rule or blocked extension, and files already present locally.
- **FR-032**: The dry run MUST stop at a time limit (60 seconds) and say the result is partial.
- **FR-033**: The dry run MUST show up to 20 sample file paths that would be downloaded, largest
  first.
- **FR-034**: The dry run MUST NOT download, delete, move or write anything, locally or on
  Put.io.

**CLI**

- **FR-035**: `putio-sync setup` MUST run the same steps in the terminal (Put.io, folder, preset,
  extras for watch folder and metrics, dry run, save), using the same checks and defaults as the
  web wizard.
- **FR-036**: `putio-sync setup` MUST accept `--download-dir`, `--preset`, `--watch-dir`,
  `--metrics`/`--no-metrics`, `--no-dry-run` and `--yes`; answered steps are not asked, and `--yes`
  takes the default for the rest.
- **FR-037**: Without a terminal, `putio-sync setup` MUST fail before writing anything when a
  required answer is missing and has no default, naming the flag or step. The Put.io token MAY
  be given through `PUTIO_SYNC_PUTIO_TOKEN` or a pasted value on standard input; it MUST NOT be a
  command-line flag.
- **FR-038**: `putio-sync setup` MUST start from the existing config and keep every setting it
  does not ask about; it MUST print the created API token once when it creates one.

**Observability**

- **FR-039**: The daemon MUST log entering and leaving setup mode with the reason (structured
  fields, no secrets), and MUST expose whether it is in setup mode as a metric when metrics are
  on.

### Key Entities

- **Setup state**: whether setup is needed and why (a list of reasons: no config, invalid config,
  no Put.io token, Put.io token rejected, folder problem), the app ID availability and whether an
  API token exists and where it comes from.
- **Setup session**: the choices made in the wizard so far (Put.io token and account, download
  folder, account-wide preset, folder choices, extras); kept in the daemon's memory, never sent
  back with secrets, discarded on restart.
- **Setup code**: a one-time random code printed at start in setup mode; valid until setup
  completes or the daemon stops.
- **Device link**: a Put.io link code, when it was issued and its state (waiting, linked,
  expired, failed).
- **Folder check**: path, exists, is a folder, writable or creatable, free and total bytes,
  warnings.
- **Dry-run result**: counts and bytes to download, skipped by reason, already present; sample
  paths; complete or partial; duration.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A new user can go from starting a fresh Docker container to syncing in under 5
  minutes, without editing a file or copying a Put.io token.
- **SC-002**: A fresh install never exits or restart-loops because configuration is missing: in
  100% of starts without a usable config the web UI (or, with the listener off, the console)
  shows how to set it up.
- **SC-003**: The dry-run counts match what the first real sync downloads for the same account
  and choices (same file count and total bytes) when nothing changes on Put.io in between.
- **SC-004**: An unwritable download folder is caught by the wizard 100% of the time, before
  setup can finish.
- **SC-005**: The CLI and the web wizard write the same config for the same answers.
- **SC-006**: No secret (Put.io token, setup code, API token) appears in log files or metrics in
  any automated test run.
- **SC-007**: The web UI stays under its size cap (raised to 300 KB for this feature).

## Assumptions

- The Put.io device link works with a public app ID and no client secret (Put.io's OOB link
  flow). The maintainer supplies the built-in app ID later; until then a build without one
  offers paste-only unless config or env sets an app ID.
- The wizard is only shown in setup mode. Re-running setup on a configured install is done with
  `putio-sync setup`; a "run setup again" button in Settings is out of scope.
- The `server` dev command is unchanged: it still needs a config and does not offer setup mode.
- The snap keeps its daemon disabled until the user starts it; its README says to start it and
  open the web UI (or run `sudo putio-sync setup`). The snap's `api-token` setting counts as an
  environment API token.
- `putio-sync login` stays for users who prefer it; the setup CLI is the documented path.
- The account-wide preset is a new `[download]` setting naming a preset; with none set the
  account uses `blocked_extensions` as today, so existing configs behave the same.
- "Start syncing" begins the normal sync loop; the first sync downloads what the dry run showed
  (subject to changes on Put.io in between).
- Radarr/Sonarr in the CLI flow is out of scope (the CLI prints how to do it from the web UI);
  018's routes stay the only *arr setup engine.
