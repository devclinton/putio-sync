# Feature Specification: What to Do When a File Already Exists

**Feature Branch**: `040-existing-file-handling`

**Created**: 2026-10-27

**Status**: Draft

**Input**: User description: "downloads from remote to local should default to resume download
for existing files. we can have it configurable to default to overwrite to, or rename new download
with number sequence"

## Context

- The sync first checks its download history. A Put.io file already recorded as downloaded is
  skipped by its Put.io id, wherever it is on disk. This feature is about the other case: a file
  the history does not list as done, whose local target path already has something in it.
- Today one on/off setting, "Skip existing" (`skip_existing`, on by default), decides that case.
  Folder rules from feature 009 can override it per folder.
  - On: any file at the target path is skipped, whatever its size. A half-copied or truncated
    file is treated as done and never fixed.
  - Off: the file is downloaded again from the start and replaces the old one.
- Interrupted downloads already resume. While a file downloads it is written to a hidden
  `.<name>.downloading` file beside the target. A later run continues it from where it stopped,
  then renames it into place once its size matches Put.io. This only covers the hidden file. A
  partial file at the real name is never resumed.
- A "Resume" setting (`resume_enabled`, CLI `--no-resume`) exists in the config, the Settings
  page and the CLI, but downloads ignore it today. Resuming the hidden file always happens.
- There is no way to keep both the old and the new file.
- Put.io gives most files a CRC32 checksum. The sync does not use it today. Feature 034 (download
  verification) plans to check finished downloads against it.

## Clarifications

### Session 2026-10-27

- Q: Does a file need a checksum match to count as complete? → A: Yes, when Put.io has a
  checksum for it. Otherwise size alone decides (FR-002).
- Q: In "resume", what if the local file is larger than the Put.io one? → A: Keep it, save the
  new download beside it under a numbered name, and log a warning (FR-007).
- Q: In "keep both", should a file that is already complete get a numbered copy? → A: No, skip
  it as "resume" does (FR-009).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A partial file is finished, a complete one is left alone (Priority: P1)

The operator does nothing: "resume" is the default. A file at the target path that is smaller
than the Put.io file is continued from where it stops, rather than skipped as it is today or
downloaded again. A file that is already complete is skipped with no download.

**Why this priority**: This is the new default and fixes the main problem. Today a truncated file
at the real name stays truncated forever.

**Independent Test**: Against a mocked Put.io and a temporary download folder, place a file with
the first half of a Put.io file's bytes at its target path, and a complete copy of a second file.
Run a sync. Check that the first file is now complete and that only its missing half was
fetched. Check that the second was not downloaded and is unchanged.

**Acceptance Scenarios**:

1. **Given** the default setting and a local file smaller than the Put.io file, **When** the sync
   reaches it, **Then** it fetches only the missing bytes, and the file ends with the same size
   as on Put.io.
2. **Given** a resumed file and a Put.io checksum for it, **When** the resume ends and the
   checksum does not match, **Then** the file is downloaded again from the start, and the log
   says why.
3. **Given** a local file the same size as the Put.io file, and a matching checksum when Put.io
   has one, **When** the sync reaches it, **Then** nothing is downloaded, the file is recorded
   as downloaded, and the log says it was already complete.
4. **Given** a hidden `.downloading` file from an interrupted run, **When** the sync reaches it,
   **Then** it resumes as it does today, whatever the existing-file choice, unless resume is
   turned off (FR-013).
5. **Given** a local file larger than the Put.io file, **When** the sync reaches it in "resume"
   mode, **Then** the local file is not changed, the new download is saved beside it under a
   numbered name, and a warning names both paths.
6. **Given** a local file the same size as the Put.io file but with a different checksum,
   **When** the sync reaches it in "resume" mode, **Then** it is handled like a larger file:
   kept, with the new download saved under a numbered name.

---

### User Story 2 - The operator chooses overwrite or keep both (Priority: P1)

The operator picks a different behaviour in Settings, or in the config file. "Overwrite"
downloads the file again from the start and replaces what is there. "Keep both" leaves the old
file alone and saves the new download beside it as `name (1).mkv`, then `name (2).mkv`. A local
file that is already complete is skipped, so no duplicate is made.

**Why this priority**: This is the choice the user asked for. Some operators want Put.io to win;
others never want a local file touched.

**Independent Test**: With a temporary download folder holding `Movie.mkv` and `Movie (1).mkv`,
run a sync in "keep both" mode for a Put.io file named `Movie.mkv`. Check that it is saved as
`Movie (2).mkv` and that both old files are unchanged. Repeat in "overwrite" mode and check that
`Movie.mkv` now matches the Put.io file byte for byte.

**Acceptance Scenarios**:

1. **Given** "overwrite" and a local file at the target path, **When** the sync reaches it,
   **Then** the whole file is downloaded again and replaces the old one only once the new copy
   is complete. Until then the old file stays readable.
2. **Given** "keep both" and a `name.ext` that differs from the Put.io file, **When** the sync
   downloads `name.ext`, **Then** it is saved as `name (1).ext`, or the lowest free number when
   `(1)` is taken.
3. **Given** "keep both" and a file with no extension, or with several dots
   (`Show.S01E01.1080p.mkv`), **When** it is saved, **Then** the number goes before the last
   extension only (`Show.S01E01.1080p (1).mkv`), or at the end when there is none.
4. **Given** "keep both" and an interrupted download of `name (1).ext`, **When** the sync runs
   again, **Then** it resumes into `name (1).ext` and does not start `name (2).ext`.
5. **Given** "keep both" and a `name.ext` that is already complete, **When** the sync reaches
   it, **Then** it is skipped, and no numbered copy is made.
6. **Given** "keep both", **When** a file is saved under a numbered name, **Then** the download
   history records the numbered path, so later syncs, History and the Put.io page use it.

---

### User Story 3 - Per-folder choice with folder rules (Priority: P2)

The operator sets the global choice in Settings and overrides it for one folder in its folder
rule, for example "overwrite" for a folder of nightly backups. A folder with no override uses the
global choice.

**Why this priority**: Folder rules (009) already let each folder override "Skip existing". That
override must keep working under the new setting.

**Independent Test**: Set the global choice to "resume" and a rule for `backups/` to
"overwrite". Place an existing file in each. Run a sync and check that only the file under
`backups/` was downloaded again. Check that the rules preview names the rule that decided each.

**Acceptance Scenarios**:

1. **Given** a folder rule with its own choice, **When** a file under that folder already exists,
   **Then** the rule's choice applies, and the nearest rule wins as with other rule settings.
2. **Given** a folder rule with no choice, **When** a file under it already exists, **Then** the
   global choice applies.
3. **Given** the rules preview and the Put.io page, **When** the operator looks at a folder,
   **Then** they see the choice in force there and where it comes from (global or which rule).

---

### User Story 4 - Radarr/Sonarr items keep a predictable path (Priority: P2)

Items sent by Radarr or Sonarr through the download client (011) report an output path,
`<reported folder>/<item name>`, which the *arr imports from. That path must always hold the new
download, so the *arr never imports an old file or misses the new one.

**Why this priority**: A numbered name would make the *arr import the old file and leave the new
one behind.

**Independent Test**: With "keep both" set globally, add an *arr item whose target file already
exists on disk with different content. Let it finish. Check that the reported output path holds
the new download, that no numbered copy was made, and that the log says why.

**Acceptance Scenarios**:

1. **Given** "keep both" and an *arr item whose target already exists, **When** its files are
   downloaded, **Then** they use "overwrite" for that item, and the reported output path holds
   the new files.
2. **Given** "resume" or "overwrite", **When** an *arr item's file already exists, **Then** that
   choice applies as for any other file.
3. **Given** any choice, **When** an *arr item is reported finished, **Then** every file under
   its output path is complete as defined in FR-002.

---

### User Story 5 - The operator can see what happened (Priority: P3)

When a file already existed, the operator can find out what the sync did with it, without
digging through debug logs.

**Why this priority**: Resuming, skipping and renaming are silent today. A file that is "already
there" but wrong is hard to track down.

**Independent Test**: Run a sync that hits each outcome (resumed, skipped as complete,
overwritten, saved under a new name, restarted after a checksum mismatch). Check that the daemon
log and the web UI's live log have one line for each, and that History shows the outcome and the
path the file was saved to.

**Acceptance Scenarios**:

1. **Given** any outcome for an existing file, **When** it happens, **Then** one info-level log
   line names the file, its path, the choice in force and the outcome.
2. **Given** a file saved under a numbered name, **When** the operator opens History, **Then**
   the entry shows the name it was saved under.

### Edge Cases

- The local file is a different file of the same size. With a Put.io checksum this is caught
  (FR-007a). Without one it cannot be seen, and the file is treated as complete.
- Checking a large complete file reads it once in full. The sync shows the check as in progress
  for that file, so a long check on a big file is not mistaken for a stall.
- Put.io has no checksum for the file. A resume is then checked by size only.
- The local path is a folder, not a file. The file is not downloaded, and an error is logged with
  the path. No choice deletes a folder.
- The local file cannot be read or written (permissions). The file fails with the reason, as
  other disk errors do today, and nothing else is changed.
- The disk fills during an overwrite. The old file stays in place, because it is only replaced
  once the new copy is complete.
- Two Put.io files map to the same local path in one run (for example after name cleanup). In
  "keep both" each gets its own number. In the other modes they are handled one after another,
  never at the same time.
- "Keep both" and a lost download history. Files already on disk and complete are skipped
  (FR-009), so only files that differ get a numbered copy.
- A numbered copy (`name (1).ext`) is itself removed from Put.io later. Empty-folder cleanup and
  the doctor treat it like any other local file.
- An old config with `skip_existing` and no new setting. See FR-011.

## Requirements *(mandatory)*

### Functional Requirements

**The setting**

- **FR-001**: There MUST be one setting, "When a file already exists", with three values:
  "resume" (default), "overwrite" and "keep both". It applies only when a Put.io file is not
  already recorded as downloaded and something exists at its local target path.

**Telling complete from partial**

- **FR-002**: A local file MUST count as complete when its size equals the Put.io size and,
  when Put.io has a checksum for the file, the checksum matches. When Put.io has no checksum, size
  alone decides. A same-size file whose checksum does not match is a different file (FR-007a).
- **FR-003**: A local file smaller than the Put.io file MUST count as partial.

**Resume (default)**

- **FR-004**: In "resume", a complete file MUST be skipped and recorded as downloaded. A partial
  file MUST be continued from its current size, fetching only the missing bytes.
- **FR-005**: After a resume, when Put.io has a checksum, the whole file MUST be checked against
  it. On a mismatch the file MUST be downloaded again from the start, once. A second mismatch
  fails the file with the reason.
- **FR-006**: A resume MUST write into a hidden working copy and put the file in place only when
  it is complete, so a half-resumed file never sits at the real name.
- **FR-007**: In "resume", a local file larger than the Put.io file MUST NOT be changed. The
  new download MUST be saved beside it under a numbered name, as in FR-009, and a warning logged
  with both paths. Radarr/Sonarr items follow FR-016 instead.
- **FR-007a**: In "resume", a local file of the same size whose checksum does not match MUST be
  handled the same way as a larger one: left unchanged, with the new download saved under a
  numbered name and a warning logged.

**Overwrite**

- **FR-008**: In "overwrite", the file MUST be downloaded from the start into a hidden working
  copy. The old file MUST be replaced only when the new copy is complete, so it stays readable
  until then and survives a failed download.

**Keep both**

- **FR-009**: In "keep both", the new download MUST be saved as `<name> (<n>).<ext>`. `n` is the
  lowest number from 1 up whose name is free. The number goes before the last extension, or at
  the end when there is none. The old file MUST NOT be touched. When the local file is already
  complete by FR-002, "keep both" MUST skip it and record it as downloaded, as "resume" does, so
  no duplicate copy is made.
- **FR-010**: The numbered name MUST be chosen once, when the download starts, and kept for its
  resume after an interruption. The download history MUST record the path the file was saved to.

**Compatibility**

- **FR-011**: `skip_existing = true` MUST be read as "resume", and `false` as "overwrite", when
  the new setting is absent, both globally and in folder rules. A settings save from the web UI
  MUST write the new setting. The example config and the README MUST mark `skip_existing` as
  replaced.
- **FR-012**: Resuming the hidden `.downloading` file of an interrupted run MUST keep working in
  every mode while resume is on (FR-013).
- **FR-013**: The `resume_enabled` setting and the CLI's `--no-resume` flag, ignored today,
  MUST start working. When off, partial files and hidden `.downloading` files are downloaded
  again from the start instead of resumed. Complete files are still skipped in "resume".

**Folder rules (009)**

- **FR-014**: The setting MUST be available globally and per folder rule, replacing the rule's
  "Skip existing" override. The nearest rule with a value wins, as for other rule settings.
- **FR-015**: The rules preview and the Put.io page MUST show the choice in force for a folder
  and whether it comes from the global setting or a rule.

**Radarr/Sonarr items (011)**

- **FR-016**: For files of a Radarr/Sonarr download-client item, every case that would save the
  new download under a numbered name MUST overwrite instead: "keep both", and the larger or
  differing local file in "resume" (FR-007, FR-007a). The reported output path then always holds
  the new download. The log line for such a file MUST say so.
- **FR-017**: An item MUST NOT be reported finished while any of its files is partial by FR-003,
  or still being resumed or checked.

**Web UI, CLI and logs**

- **FR-018**: The Settings page MUST replace the "Skip existing" toggle with a "When a file
  already exists" choice of the three values. Each value has one line of help, and "keep both"
  shows an example name. The folder-rule editor MUST offer the same choice, plus "use global".
- **FR-019**: The CLI's `download` command MUST accept the choice as an option, overriding the
  config for that run. The existing `--skip-existing` flag MUST keep working as "resume".
- **FR-020**: Each existing-file outcome MUST be logged once at info level, with the file name,
  local path, choice in force and outcome: skipped (complete), resumed (from how many bytes),
  restarted (checksum mismatch after a resume), overwritten, saved as (new name, and why: "keep
  both", larger local file, or differing checksum), or refused (path is a folder).
- **FR-021**: History MUST show the saved path for each download, and mark files that were
  resumed, overwritten or saved under a new name.
- **FR-022**: The daemon MUST count existing-file outcomes in its metrics, by outcome only, with
  no file names or paths in labels.
- **FR-023**: The setting MUST be documented in the example configuration and the README.

### Key Entities

- **Existing-file choice**: "resume", "overwrite" or "keep both". Set globally, optionally per
  folder rule, and optionally per CLI run.
- **Existing-file outcome**: what happened to one file: skipped, resumed, restarted, overwritten,
  saved as, or refused. Logged, counted, and kept with the file's download record.
- **Download record**: the existing history entry, now holding the path the file was really
  saved to and its existing-file outcome.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With default settings, a local file truncated to any size below the Put.io size
  ends complete after one sync, fetching no more than the missing bytes plus one retry.
- **SC-002**: In the automated tests, no mode ever leaves a partial file at a file's real name,
  and no local file is changed in "keep both" mode.
- **SC-003**: In "overwrite", a download that fails halfway leaves the old file unchanged in
  every automated test.
- **SC-004**: Every Radarr/Sonarr item reported finished has its new files at the reported
  output path in every mode.
- **SC-005**: For every existing-file outcome, the operator can find what happened and the saved
  path from the live log or History alone, in under a minute.
- **SC-006**: Operators who never change the setting and had "Skip existing" on see only these
  changes: partial files at the real name are now finished, and a local file that is larger than
  the Put.io file or fails its checksum gets the new download beside it under a numbered name.

## Assumptions

- This feature checks an existing file's checksum only when it decides what to do with that
  file, and after a resume. Checking finished downloads in general, and the "verify everything"
  run, stay in feature 034, which can reuse the same check.
- The choice applies per file, not per folder. When a Put.io folder lands on a local folder that
  already exists, the files are merged, and each file that collides follows the choice.
- The download history keeps working as today: a file recorded as downloaded is skipped by its
  Put.io id, whatever this setting says. Re-downloading a recorded file stays a manual action.
- The watch folder (blackhole) path is unchanged. It only uploads to Put.io, and the files come
  back through the normal sync, so this setting covers them.
- Numbered names use the ` (n)` style that browsers and file managers use. Other styles are out
  of scope.
- Feature 038 (processing pipeline) has subtitle and hook steps work on the file at its saved
  path. A numbered name is the saved path for those steps.
- Deleting old copies, or merging duplicates afterwards, is out of scope.
