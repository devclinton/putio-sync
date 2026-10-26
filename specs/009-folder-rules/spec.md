# Feature Specification: Per-folder file rules

**Feature Branch**: `009-folder-rules`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "rules like deleting or not downloading specific types can be set per path (and its children inherit). I also would like some easy to set presets like don't download binary types, only video, only audio, only ebook, etc so it is easy to set those filters on each subfolder."

## Clarifications

### Session 2026-10-26

- Q: Which path is a rule attached to? → A: The folder path relative to the Put.io root (e.g. `Movies/Kids`). It is also the local path under the download folder, so one path names both.
- Q: What can a rule set? → A: File types (a preset plus extra allowed or blocked extensions), "skip this whole folder", "delete blocked files from Put.io", and per-folder "delete after download" and "skip existing".
- Q: Where are rules and presets kept? → A: In the config file, next to the other download settings, so they can be hand-edited and backed up. Saves from the web UI keep the file's comments.
- Q: How does today's global blocked-extensions list relate to folder rules? → A: It is the rule at the root. A folder rule can override it, for example to allow `exe` in a Software folder.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Only download the right kind of file in a folder (Priority: P1)

The user keeps movies in `Movies`, music in `Music` and books in `Books` on Put.io. Releases often carry extras the user doesn't want (samples, `.nfo`, `.txt`, executables). The user sets `Movies` to "Video only", `Music` to "Audio only" and `Books` to "Ebooks only". From the next sync, files below each folder that don't match its preset are not downloaded, and every subfolder follows its parent's rule without being set one by one.

**Why this priority**: This is the core of the request: type filtering per folder with inheritance and presets.

**Independent Test**: Put a folder rule in the config file, run a sync against a mocked Put.io with mixed file types, and check which files reach the queue.

**Acceptance Scenarios**:

1. **Given** `Movies` has the "Video only" preset, **When** a sync finds `Movies/Film (2020)/film.mkv` and `Movies/Film (2020)/info.nfo`, **Then** the `.mkv` is queued and the `.nfo` is not.
2. **Given** `Movies` is "Video only" and `Movies/Extras` has the "Everything" preset, **When** a sync finds `Movies/Extras/notes.txt`, **Then** it is queued (the deeper rule wins).
3. **Given** no folder rule anywhere, **When** a sync runs, **Then** behavior is exactly as today: only the global blocked extensions are refused.
4. **Given** `Software` has a rule that allows `exe`, **When** a sync finds `Software/tool.exe`, **Then** it is queued even though `exe` is in the global blocked list.
5. **Given** `Music` is "Audio only" with `jpg` added as allowed, **When** a sync finds `Music/Album/cover.jpg`, **Then** it is queued.

---

### User Story 2 - Set and clear a rule from the Put.io page (Priority: P1)

While browsing Put.io in the web UI, the user opens a folder and sees the rule in force there: the preset and extensions, whether files are deleted after download, and for each part whether it is set on this folder, inherited from a named parent folder, or the global default. The user picks a preset from a list, optionally adds allowed or blocked extensions and toggles the other options, and saves. The user can also clear the folder's own rule, after which the folder inherits again.

**Why this priority**: The user asked for presets that are "easy to set on each subfolder"; editing a config file by hand for each folder is not easy.

**Independent Test**: In the UI against the mock backend, open a folder, set "Video only", reload, and check the folder and its child both show the rule (the child as inherited).

**Acceptance Scenarios**:

1. **Given** the user is on Put.io folder `Movies`, **When** they choose "Video only" and save, **Then** the rule is written to the config file and the page shows it as set on `Movies`.
2. **Given** `Movies` has a rule, **When** the user opens `Movies/Kids`, **Then** the page shows "Video only" as inherited from `Movies`.
3. **Given** `Movies` has a rule, **When** the user clears it, **Then** the rule is removed from the config file and `Movies` shows the global default again.
4. **Given** the config file has comments, **When** a rule is saved from the UI, **Then** the comments are still in the file.
5. **Given** a rule is saved, **When** the next sync runs, **Then** it uses the new rule without a restart.

---

### User Story 3 - Delete unwanted files from Put.io (Priority: P2)

The user doesn't want blocked files left behind on Put.io. On a folder rule they turn on "delete blocked files from Put.io". Before saving, the UI shows a preview (a dry run) of the files in that folder and its subfolders that the rule would block, and which of them would be deleted. After saving, each sync deletes blocked files below that folder from Put.io.

**Why this priority**: The user named "deleting specific types" as a rule; it is destructive, so it needs a preview and comes after the non-destructive filter.

**Independent Test**: Against a mocked Put.io, preview a rule with delete on, check that nothing was deleted, then run a sync and check the mock received deletes for only the blocked files.

**Acceptance Scenarios**:

1. **Given** a rule on `Movies` with "Video only" and delete on, **When** the user previews it, **Then** they see the blocked files below `Movies` marked "would be deleted" and nothing is deleted.
2. **Given** that rule is saved, **When** a sync finds `Movies/Film/sample.txt`, **Then** the file is deleted from Put.io and not downloaded.
3. **Given** delete is off, **When** a sync finds a blocked file, **Then** it stays on Put.io.
4. **Given** a file is in the download queue or downloading, **When** a later sync blocks it with delete on, **Then** it is not deleted from Put.io.

---

### User Story 4 - Skip a folder, and per-folder delete-after-download and skip-existing (Priority: P2)

The user has a `Seeding` folder that must never be downloaded, so they mark it "skip this folder". They want `Movies` deleted from Put.io after download, but not `Music`, so they set "delete after download" per folder; the same for "skip existing".

**Why this priority**: Extends the same rule model with the other per-folder options the user asked for.

**Independent Test**: With rules for skip and delete-after-download, run a sync and a queue pass against the mock, and check what was queued and which files were deleted after download.

**Acceptance Scenarios**:

1. **Given** `Seeding` is skipped, **When** a sync runs, **Then** nothing below `Seeding` is queued, even files a type rule would allow.
2. **Given** `Seeding` is skipped and `Seeding/Keep` has a rule that turns skip off, **When** a sync runs, **Then** files below `Seeding/Keep` are queued.
3. **Given** the global setting is "keep on Put.io" and `Movies` sets "delete after download", **When** a file below `Movies` finishes downloading and is verified, **Then** it is deleted from Put.io, while a finished file in `Music` is kept.
4. **Given** the global "skip existing" is on and `Music` turns it off, **When** a file in `Music` already exists locally, **Then** it is downloaded again.

---

### User Story 5 - See all rules and edit presets in Settings (Priority: P3)

On the Settings page the user sees every folder rule in one list (folder, preset, extra extensions, options), can remove any of them, and sees the presets with their extension lists. The user can change a built-in preset's extensions (e.g. add `m2ts` to "Video only"), reset it to the built-in list, and create their own presets (e.g. "Comics": `cbz`, `cbr`) that then show up in the Put.io page's preset list.

**Why this priority**: Overview and tuning; the feature works without it.

**Independent Test**: In the UI against the mock backend, add a preset, check it is offered on the Put.io page, and check it appears in the config file.

**Acceptance Scenarios**:

1. **Given** three folder rules, **When** the user opens Settings, **Then** all three are listed with their folder paths.
2. **Given** the user adds `m2ts` to "Video only", **When** they save, **Then** the config file holds the edited list and later syncs use it.
3. **Given** an edited built-in preset, **When** the user resets it, **Then** the config file no longer overrides it and the built-in list applies.
4. **Given** a rule uses a user preset, **When** the user tries to delete that preset, **Then** the delete is refused with a message naming the folders that use it.

---

### User Story 6 - Know why a file was not downloaded (Priority: P2)

A file the user expected never arrived. The user can find it in a list of blocked files that says which rule blocked it (the folder path of the rule and the reason: extension not allowed, extension blocked, folder skipped) and whether it was deleted from Put.io. The same facts are in the logs, and the metrics count blocked, skipped and deleted files.

**Why this priority**: Rules silently hiding files would be confusing; the explanation makes them trustworthy.

**Independent Test**: Run a sync with rules against the mock and check the blocked-files list, the log line, and the metric counters.

**Acceptance Scenarios**:

1. **Given** `Movies` is "Video only", **When** a sync blocks `Movies/Film/info.nfo`, **Then** the blocked-files list shows the file, the rule folder `Movies`, and "not allowed by preset Video only".
2. **Given** a blocked file is deleted from Put.io, **When** the user views the list, **Then** the entry says it was deleted.
3. **Given** the same file is blocked on every sync, **When** the user views the metrics, **Then** it is counted once, not once per sync.
4. **Given** a file was blocked and the user then changes the rule so it is allowed, **When** the next sync queues it, **Then** it is no longer listed as blocked.

### Edge Cases

- A rule names a folder that doesn't exist (yet) on Put.io: it is kept and applies when the folder appears.
- A folder is renamed on Put.io: its rule no longer matches; the Settings list still shows the old path so the user can remove or edit it.
- A rule names a preset that doesn't exist (hand-edited config): the config is refused at load and on save, with a message naming the rule and the preset.
- Two rules for the same path in the config file: refused at load, with a message naming the path.
- Paths are written with or without a leading or trailing `/`, or with doubled `/`: they are normalised; `..`, `.` and empty paths are refused (the root is the global settings, not a rule).
- Folder names with characters the download path replaces (e.g. `:`): the rule matches the folder as stored locally and as named on Put.io.
- Files with no extension: an allow-only preset (video, audio, ebooks) blocks them; block-list presets allow them.
- Extension matching is case-insensitive and uses the last extension (`a.mkv.exe` is `exe`), like today's blocklist.
- A "delete after download" rule never deletes before the local copy is verified, same as today.
- Queueing a single file or folder from the UI goes through the same rules as the sync, so a blocked file can't be queued by hand; the queue response reports it as blocked.
- Files already in the queue when a rule changes: the queue item is kept; the new rule's delete-after-download and skip-existing apply when it is downloaded.

## Requirements *(mandatory)*

### Functional Requirements

**Rules and inheritance**

- **FR-001**: A folder rule MUST be attached to a folder path relative to the Put.io root, and MUST apply to that folder and everything below it.
- **FR-002**: For each setting, the rule on the nearest folder (the deepest one on the file's path) that sets it MUST win; a setting no rule on the path sets MUST come from the global download settings.
- **FR-003**: A rule MUST be able to set any of: a file-type preset; extra allowed extensions; extra blocked extensions; skip folder (on or off); delete blocked files from Put.io (on or off); delete after download (on or off); skip existing (on or off). Every setting MUST be optional.
- **FR-004**: The file-type filter MUST be built from the root down: the global blocked-extensions list is the start; a rule with a preset replaces the inherited filter with the preset; a rule's allowed extensions are then added (and removed from the blocked ones); a rule's blocked extensions are then added and win over allowed ones. A rule with only allowed or blocked extensions edits the inherited filter.
- **FR-005**: When no folder rules exist, which files are downloaded, skipped or deleted MUST be exactly as before this feature.

**Presets**

- **FR-006**: The system MUST provide built-in presets: "Video only", "Audio only", "Ebooks only", "No binaries" (blocks executables and installers), and "Everything" (blocks nothing).
- **FR-007**: A preset MUST be either an allow list (only these extensions) or a block list (everything except these extensions).
- **FR-008**: The user MUST be able to override a built-in preset's extensions in the config file and from Settings, reset it to the built-in list, and add their own presets with a name of their choosing.
- **FR-009**: Deleting a preset that a rule uses MUST be refused, naming the rules that use it.

**Storage**

- **FR-010**: Rules and user presets MUST be kept in the config file and documented in the example config.
- **FR-011**: Saving rules or presets from the web UI MUST keep the config file's comments and leave its credentials untouched.
- **FR-012**: An invalid rule or preset (unknown preset, duplicate path, bad path, bad extension) MUST be refused at load and on save, with a message naming it; a refused save MUST change nothing.
- **FR-013**: Rule and preset changes MUST apply from the next sync and the next download without a restart and without stopping downloads in progress.

**Sync behavior**

- **FR-014**: The sync and manual queueing MUST NOT queue a file its effective rule blocks or skips.
- **FR-015**: When the effective rule has "delete blocked files" on, a file blocked by its type filter MUST be deleted from Put.io, except files that are in the download queue. Files under a skipped folder MUST NOT be deleted by this setting.
- **FR-016**: Delete after download and skip existing MUST use the file's effective rule when the file is downloaded.

**Preview**

- **FR-017**: The user MUST be able to preview a rule (saved or not) on a folder before saving: the files below that folder it would block or skip, and which would be deleted. A preview MUST NOT change anything.

**Web UI**

- **FR-018**: The Put.io page MUST show, for the open folder, the effective rule and, for each setting, where it comes from (this folder, a named parent folder, or the global settings).
- **FR-019**: The Put.io page MUST let the user set, change and clear the open folder's own rule, choosing the preset from a list that includes user presets.
- **FR-020**: The Settings page MUST list all folder rules with a way to remove each, and list presets with their extensions, with ways to edit, reset and add presets.

**Explaining decisions**

- **FR-021**: Each file a rule blocks, skips or deletes MUST be recorded with the file, its path, the folder of the rule that decided it, the reason, whether it was deleted, and when; the record MUST be removed when the file is later queued.
- **FR-022**: The web UI MUST show the recorded blocked files, newest first.
- **FR-023**: Each decision MUST be logged once per file per process with the file id, name, rule folder and reason.
- **FR-024**: The metrics MUST count files blocked, skipped and deleted by rules, each file counted once per process, without file names or folder paths as labels.

### Key Entities

- **Folder rule**: a folder path plus optional settings (preset, allowed extensions, blocked extensions, skip folder, delete blocked, delete after download, skip existing).
- **Preset**: a name and either an allow list or a block list of extensions; built-in or user-defined; a user entry with a built-in name overrides it.
- **Effective rule**: the settings that apply to one folder after inheritance, each with the folder (or "global") it came from.
- **Rule decision**: a record that a file was blocked, skipped or deleted, with the rule folder and reason.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The user can put a preset on a folder from the Put.io page in under 30 seconds and 4 clicks or fewer.
- **SC-002**: With rules on `Movies`, `Music` and `Books`, a sync against a mixed test library queues 100% of the matching files and 0 of the non-matching ones.
- **SC-003**: With no rules, a sync over the same library queues exactly the same files as before this feature.
- **SC-004**: For every file a rule kept from downloading, the user can find the deciding rule's folder and reason in the UI.
- **SC-005**: A preview never deletes anything; a sync with delete on deletes only files the preview listed as "would be deleted" (given no Put.io changes in between).
- **SC-006**: Saving a rule from the UI keeps 100% of the config file's comments.

## Assumptions

- Folder paths are matched exactly, case-sensitively, segment by segment, after the same character replacement the download path uses.
- The one-off `download` command keeps using its own glob filters; folder rules apply to the sync, the queue and manual queueing from the UI (daemon and server).
- Built-in preset lists: Video only: mkv, mp4, m4v, avi, mov, wmv, mpg, mpeg, ts, m2ts, webm, flv, plus subtitles srt, sub, idx, ass, ssa, vtt. Audio only: mp3, flac, m4a, m4b, aac, ogg, opus, wav, wma, alac, ape, aiff, cue. Ebooks only: epub, mobi, azw, azw3, pdf, cbz, cbr, djvu, fb2. No binaries: today's global list plus dll, so, bin, deb, rpm, appimage, run, sh, app. Everything: nothing blocked. All are editable.
- A rename on Put.io is not followed; the rule keeps its old path.
- No per-rule target subfolder in this feature.
- The blocked-files list keeps the latest decision per file. An entry is removed when the file is queued; entries for files later removed from Put.io by other means stay until the list is cleared from the UI.
