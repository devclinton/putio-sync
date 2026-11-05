# Feature Specification: Radarr/Sonarr automatic setup including path mapping

**Feature Branch**: `claude/arr-auto-setup-645779`

**Created**: 2026-11-05

**Status**: Draft

**Input**: User description: "It would also be nice if for radar/sonarr it had some automatic setup in the app itself once it knows their urls (including the path mapping)"

## What already exists (features 011 and 018)

Audited before writing this spec, so it covers only the gap:

- **Discovery** finds Radarr and Sonarr without a key; **preview** and **setup** add or update
  putio-sync's Transmission download client in the app (test before write, update in place,
  undo on failure, category set to `radarr`/`sonarr` or one typed, optional Torrent Blackhole
  disable, optional Remember).
- **Path check**: Test and every setup ask the app, through its existing Remote Path Mappings,
  whether it can see putio-sync's download folder, and show "Folder visible" or "cannot see".
- **The gap**: when the app cannot see the folder, the operator is told to add a Remote Path
  Mapping by hand. putio-sync never works out what the mapping should be, never creates or
  fixes one, and the setup is several separate buttons rather than one flow. Settings in the
  app that imports depend on (Completed Download Handling, Import Extra Files for subtitles)
  are not looked at.

## Clarifications

### Session 2026-11-05

Answered with the starting session's stated defaults (this feature runs in a child session):

- Q: Should the guided setup turn on Completed Download Handling and Import Extra Files (srt) by itself, or only recommend them? → A: Recommend only; each has its own Turn on button and nothing changes without that click.
- Q: When the folder search finds the app's path, should the mapping be written at once or after the operator confirms? → A: After confirming; the proposal is shown first (FR-008).
- Q: Should the folder step and recommendations also be available without saving the application? → A: Yes; they use the typed address and key in "Set up automatically", and only a saved application keeps the result on its row.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Folder mapping worked out and created (Priority: P1)

An operator runs putio-sync and Sonarr in separate Docker containers on a NAS. putio-sync
downloads to `/downloads`; Sonarr has the same host folder mounted at `/data/downloads`. After
setup, Sonarr cannot see `/downloads`. putio-sync finds where Sonarr sees its download folder,
proposes a Remote Path Mapping (host `putio-sync`, remote `/downloads/`, local
`/data/downloads/`), and on confirmation creates it in Sonarr, then checks again and shows that
the folder is now visible.

**Why this priority**: this is the step operators get wrong most often, and the one the user
asked for by name. Without it, downloads finish but never import.

**Independent Test**: against a mock Sonarr whose filesystem shows putio-sync's folder under
another path, run the folder step; a mapping is proposed, created after confirming, and the
re-check passes.

**Acceptance Scenarios**:

1. **Given** an app that cannot see the download folder but has it mounted elsewhere, **When**
   the folder step runs, **Then** it proposes a mapping with the host the app's putio-sync
   client uses, putio-sync's folder as the remote path and the app's path as the local path,
   and changes nothing yet.
2. **Given** a proposed mapping, **When** the operator confirms, **Then** the mapping is created
   in the app, the folder is checked again, and the result says "Folder visible".
3. **Given** the app already sees the folder (same path or through a correct mapping), **When**
   the folder step runs, **Then** it says so and proposes nothing.
4. **Given** the folder is not mounted in the app at all, **When** the folder step runs,
   **Then** it says the folder could not be found, names where it looked, and says to mount the
   download folder in the app's container; nothing is written.

---

### User Story 2 - One guided setup with every step's result (Priority: P1)

Once an app's address and API key are known (a saved application, or the address and key typed
in "Set up automatically"), one **Set up** action runs the whole setup in order: the download
client, the category, the folder, and the recommended app settings. Each step shows its own
result line (done, already fine, needs a decision, or failed with the reason and next step),
so the operator sees exactly what was done.

**Why this priority**: the user asked for setup "in the app itself once it knows their urls";
a single flow is what makes it automatic rather than a set of separate tools.

**Independent Test**: run Set up for a saved application against a mock app with no putio-sync
client and a path mismatch; the steps show "Download client created", "Category radarr",
"Mapping proposed", then after confirming "Folder visible".

**Acceptance Scenarios**:

1. **Given** a saved application with an API key, **When** the operator clicks Set up on its
   row, **Then** the steps run in order and each shows its result.
2. **Given** a failed download-client step, **When** setup stops, **Then** the later steps are
   not run and the failure says why, as 018 does today.
3. **Given** "Set up automatically" with an address and key, **When** the setup succeeds,
   **Then** the same folder and settings steps follow, using that address and key.

---

### User Story 3 - Safe to run again (Priority: P2)

Running the guided setup again on an app that is set up changes nothing and says each step is
already fine. If something drifted (a mapping now points at the wrong place, the folder moved,
the client was edited), the rerun fixes that one thing in place and never adds a second
mapping or client.

**Why this priority**: operators rerun setup after container changes; duplicates in Radarr's
mapping list are confusing and Radarr refuses some of them.

**Independent Test**: run the folder step twice against a mock app; the second run makes no
write. With a wrong existing mapping for putio-sync's host, the proposal is an update of that
mapping, and confirming updates it rather than adding another.

**Acceptance Scenarios**:

1. **Given** everything set up, **When** setup runs again, **Then** no change is written and
   every step reads "already fine".
2. **Given** an existing mapping for putio-sync's host that points at the wrong local path,
   **When** the folder step runs, **Then** it proposes changing that mapping, and confirming
   changes it in place.
3. **Given** the app sees the folder at the same path but a mapping for putio-sync's host
   rewrites it somewhere the app can't see, **When** the folder step runs, **Then** it proposes
   removing that mapping, and confirming removes it.

---

### User Story 4 - Recommended app settings (Priority: P3)

The guided setup reads two app settings that imports from putio-sync depend on: Completed
Download Handling (the app imports finished downloads) and Import Extra Files with `srt`
(the app imports the subtitles putio-sync names next to videos). Each one that is off is shown
as a recommendation with a button to turn it on; nothing changes without that click.

**Why this priority**: helpful, but both are often already on, and turning settings on in
someone else's app should be the operator's choice.

**Independent Test**: against a mock app with Import Extra Files off, the step shows the
recommendation; clicking Turn on saves it with `srt` added, and a re-read shows it on.

**Acceptance Scenarios**:

1. **Given** both settings already on, **When** the step runs, **Then** it reads "Recommended
   settings on" and offers nothing.
2. **Given** Import Extra Files off, **When** the operator clicks Turn on, **Then** it is turned
   on and `srt` is added to the extensions (existing extensions kept).
3. **Given** an app version that has no such setting, **When** the step runs, **Then** that
   setting is skipped without an error.

### Edge Cases

- The app's putio-sync download client is missing: the folder step says to set up the download
  client first (the mapping host comes from it).
- putio-sync's download folder doesn't exist or can't be written: the folder step can't place
  its marker, says so, and writes nothing to the app.
- The search for the folder takes too long or the app has a very large filesystem: the search
  stops at its limits and reports "not found" with where it looked.
- The app refuses the mapping (its own validation): the app's message is shown; nothing else
  changes.
- The download folder appears at two places in the app (two mounts): the first found is
  proposed; the operator can still edit the mapping in the app.
- Windows paths in the app: compared and mapped with the app's separator; the search starts
  from the app's root folders.
- The API key and download-client password never appear in a result, a log line or an error.
- The temporary marker folder is removed after every search, found or not.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The Applications list MUST offer a **Set up** action on every saved application
  that has an API key, and "Set up automatically" MUST continue into the same steps after a
  successful download-client setup.
- **FR-002**: Set up MUST run, in order: download client (018 setup), category (the category
  the client now has), folder, recommended settings; and MUST show one result line per step.
- **FR-003**: A failed step MUST stop the steps that depend on it (the folder step needs the
  download client) and show the reason and the next thing to check.
- **FR-004**: The folder step MUST first run the existing path check. When the folder is
  visible, it MUST propose nothing.
- **FR-005**: When the folder is not visible, putio-sync MUST find where the app sees its
  download folder by placing a uniquely named temporary folder inside it and looking for that
  name through the app's own filesystem browser, starting from the app's root folders and the
  top of its filesystem, within fixed limits on folders listed and time.
- **FR-006**: The temporary folder MUST be removed after the search, whatever its outcome.
- **FR-007**: When found, putio-sync MUST propose exactly one change: create a mapping, change
  the existing mapping for putio-sync's host that covers the folder, or remove a mapping for
  putio-sync's host when the app sees the folder at the same path without it. The mapping's
  host MUST be the host in the app's putio-sync download client.
- **FR-008**: No mapping MUST be written until the operator confirms the proposed change.
- **FR-009**: After a confirmed change, putio-sync MUST run the path check again and show its
  result; for a saved application that result MUST replace the one its row shows.
- **FR-010**: Rerunning MUST NOT create a second mapping for the same host and remote path, or
  a second putio-sync download client.
- **FR-011**: The recommended-settings step MUST read Completed Download Handling and Import
  Extra Files (with `srt` in its extensions) and show each that is off as a recommendation
  with its own Turn on action; settings the app doesn't have MUST be skipped.
- **FR-012**: Turn on MUST change only that setting (and add `srt` to the extensions without
  removing others), keeping the app's other settings as they were.
- **FR-013**: Every failure MUST use the existing reasons (unreachable, unauthorized, refused,
  …) plus new ones for "no putio-sync client", "folder not found" and "marker could not be
  placed", each with a next step.
- **FR-014**: API keys and passwords MUST NOT appear in responses, logs or errors.
- **FR-015**: Mapping changes MUST be logged with the app, the change and the paths, and
  counted in a metric by result.

### Key Entities

- **Mapping proposal**: the change the folder step suggests: create / update / remove / none,
  with host, remote path (putio-sync's folder), local path (the app's), the existing mapping's
  id for update or remove, and where the search looked.
- **Recommendation**: a named app setting (`completed_download_handling`,
  `import_extra_files`), whether it is on, and whether the app has it.
- **Step result**: a step name, its status (done, fine, decide, failed) and its text.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For a Docker setup where the app mounts the download folder under another path,
  the operator goes from "cannot see" to "Folder visible" with one Set up and one confirmation,
  without opening the app.
- **SC-002**: Running Set up a second time on a set-up app writes nothing and shows every step
  as already fine.
- **SC-003**: The folder search finishes, found or not, in under 30 seconds.
- **SC-004**: No temporary folder is left in the download folder after any run.
- **SC-005**: No secret appears in any response or log line in the tests' secrets sweep.

## Assumptions

- The app reaches putio-sync's files through a mounted folder (Docker volume or network share);
  putio-sync can't fix a folder that isn't mounted at all, only say so.
- The mapping covers putio-sync's whole download folder; category subfolders follow it.
- The app's filesystem browser shows the folders the app can read, including the marker.
- Recommendations are offered, not applied by default (parent session's default).
- The UI additions stay small: the bundle cap is 416 KiB and the bundle is about 401 KiB.
- Plugin UI files are out of scope (feature 048 is changing them).
