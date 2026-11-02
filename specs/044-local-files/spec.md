# Feature Specification: Local Files Page

**Feature Branch**: `044-local-files`

**Created**: 2026-11-01

**Status**: Built (all four stories); checked in a browser against local folders, NAS quickstart steps left

**Input**: User description: "Add a 'Local files' page to the web UI where the operator can see
and manage files on the NAS side: the download target directory (including per-category
subfolders like sonarr/ and radarr/) and each configured watch folder. Show only roots that are
enabled/configured. List folders and files with size and modified time; show partial
`.downloading` files and watch-folder `.rejected` files clearly. Manual actions: at least delete,
and consider rename and re-queueing a `.rejected` watch file. Every action stays inside the
configured roots (reject path traversal and symlinks that escape), requires the API token, and
asks for confirmation in the UI before deleting."

## Context

- putio-sync downloads into one folder (`[download] target_directory`, `/downloads` on the NAS),
  with Radarr/Sonarr items in a subfolder per category (`radarr/`, `sonarr/`). Downloads are
  written as hidden `.<name>.downloading` files and renamed when complete.
- With watching on, it picks up `.torrent` and `.magnet` files from each `[watch] paths` folder
  (`/watches/radarr`, `/watches/sonarr`). A file that fails the release check (042) is renamed
  `<file>.rejected` and left there.
- Fixing stuck imports today means a shell on the NAS: removing a stuck partial, clearing a
  `.rejected` fake, deleting a leftover Sonarr did not import. The web UI shows Put.io's files
  but nothing on the NAS side.
- `GET /local/files` and `DELETE /local/files/*path` exist but only list the top of the download
  folder and check paths by string prefix, so `..` can leave it. No UI uses them. This feature
  replaces them.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See what is on the NAS (Priority: P1)

The operator opens "Local files" and sees one section per root: Downloads, then each watch folder
when watching is on. Each lists its folders and files with size and modified time. They open a
folder (for example `sonarr/`) to see inside it, and a path bar takes them back up. Partial
downloads are marked "Partial" and rejected watch files "Rejected", so a stuck item stands out
without reading file names.

**Why this priority**: Seeing the files is the base for every fix, and alone already replaces
`ls` over SSH.

**Independent Test**: Put files, a subfolder, a `.x.mkv.downloading` and a `y.torrent.rejected`
in temp roots; the listing returns each with size, time and kind, folders first.

**Acceptance Scenarios**:

1. **Given** watching is off, **When** the page loads, **Then** only Downloads is shown.
2. **Given** watching is on with two paths, **When** the page loads, **Then** Downloads and both
   watch folders are shown, each labelled with its path.
3. **Given** a folder with a partial download, **When** it is listed, **Then** the entry is
   marked partial and shows the name it will have when done.
4. **Given** a root that does not exist on disk, **When** the page loads, **Then** that root
   says it is missing instead of failing the page.

---

### User Story 2 - Delete a file or folder (Priority: P1)

The operator deletes a stuck partial, a rejected fake, or a leftover folder. The UI names what
will be deleted (and for a folder, how many files and how much space) and asks first. After the
delete the listing refreshes.

**Why this priority**: Delete is the manual fix asked for most often.

**Independent Test**: Delete a file and a folder through the API; both are gone from disk, and
paths outside the roots are refused with nothing deleted.

**Acceptance Scenarios**:

1. **Given** a file in a root, **When** the operator confirms delete, **Then** it is removed
   and the listing no longer shows it.
2. **Given** a folder, **When** delete is confirmed, **Then** the folder and its contents are
   removed.
3. **Given** the operator cancels the confirmation, **Then** nothing changes.
4. **Given** a partial a download is writing right now, **When** delete is asked, **Then** it is
   refused with a message to cancel the download in the Queue first.
5. **Given** a request for the root itself, `..`, an absolute path, or a symlink pointing out of
   the root, **Then** it is refused and nothing outside the root is touched.

---

### User Story 3 - Re-queue a rejected watch file (Priority: P2)

A watch file was rejected, and the operator thinks the check was wrong or has changed the rules.
"Re-check" renames it back to its original name and makes the watcher treat it as new, so the
next scan checks it again and uploads it if it now passes.

**Why this priority**: Without it, a wrong rejection needs a shell and knowledge of the
processed-file records.

**Independent Test**: A `x.magnet.rejected` whose content is recorded as processed: after
re-queue, `x.magnet` exists, `.rejected` is gone, and the record no longer marks it processed.

**Acceptance Scenarios**:

1. **Given** `x.torrent.rejected` in a watch folder, **When** re-queue is chosen, **Then**
   `x.torrent` exists, the watcher no longer remembers it as processed, and its next scan
   checks it again.
2. **Given** `x.torrent` already exists next to it, **When** re-queue is chosen, **Then** it is
   refused and both files stay.
3. **Given** a `.rejected` file in the Downloads root, or any file not ending in `.rejected`,
   **Then** re-queue is not offered and the API refuses it.

---

### User Story 4 - Rename (Priority: P3)

The operator renames a file or folder in place, for example to fix a name Radarr cannot parse.

**Why this priority**: Useful but less common than delete.

**Acceptance Scenarios**:

1. **Given** a file, **When** it is renamed to a free name, **Then** it has the new name in the
   same folder.
2. **Given** a new name containing `/`, `\`, being `.` or `..`, or empty, **Then** it is refused.
3. **Given** the new name is taken, **Then** it is refused and nothing is overwritten.

### Edge Cases

- A symlink inside a root is listed as a link and not followed. Acting on a link acts on the link
  itself (delete removes the link, not its target). A path whose parent folders include a link
  that leads outside the root is refused.
- A root that is itself a symlink (common with Docker or Unraid shares) is allowed: the root is
  resolved once and paths are checked against the resolved root.
- Large folders: a listing returns at most 2,000 entries and says when it was cut short.
- A file deleted between listing and action gives "not found" and the page refreshes.
- Names that are not valid UTF-8 are shown with replacement characters and cannot be acted on.
- Watch folders nested inside the download folder are still listed as their own root.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST list the configured roots: the download folder always, and each
  watch path only when watching is enabled. Each root says whether it exists.
- **FR-002**: The system MUST list one folder of a root at a time, given the root and a path
  relative to it, with each entry's name, kind (file, folder, link), size, modified time, and
  marks for partial downloads and rejected watch files. Folders sort first, then by name.
- **FR-003**: Every listing and action MUST resolve the target inside its root and refuse
  absolute paths, `..` components, the root itself (for actions), and any path whose parent
  folders resolve outside the root.
- **FR-004**: The system MUST delete a file, link or folder (with contents) on request.
- **FR-005**: The system MUST refuse to delete a partial download that a running download is
  writing, or a folder containing one.
- **FR-006**: The system MUST rename an entry within its own folder to a new single-segment name
  that is not already taken.
- **FR-007**: The system MUST re-queue a `.rejected` file in a watch root: rename it to its name
  without `.rejected` (only if that name is free and ends in `.torrent` or `.magnet`) and forget
  it as processed, so the watcher's next scheduled scan checks it again.
- **FR-008**: All of these routes MUST require the API token, like other protected routes.
- **FR-009**: The UI MUST ask for confirmation before any delete, naming the entry and, for a
  folder, its size.
- **FR-010**: The old `/local/files` routes MUST be replaced by these, so no unchecked path
  route remains.
- **FR-011**: Each action MUST be logged with the root and relative path.

### Key Entities

- **Root**: a folder the page may show and act in. Key (`downloads`, `watch-0`, `watch-1`, …),
  label, path, whether it exists, and whether it is a watch folder.
- **Entry**: a name in a listed folder, with kind, size, modified time, and flags: partial (with
  the final name) or rejected (with the original name), and for folders, whether they hold an
  active download.

## Success Criteria *(mandatory)*

- **SC-001**: The operator can find and remove a stuck partial download from the browser in under
  a minute, without SSH.
- **SC-002**: No request, however formed, reads, renames or deletes anything outside the
  configured roots (covered by tests for `..`, absolute paths, encoded separators and escaping
  symlinks).
- **SC-003**: A wrongly rejected watch file can be checked again in one click and one scan.
- **SC-004**: The web UI stays within its existing size budget.

## Assumptions

- Only one operator uses the UI; there is no locking between two browsers acting at once.
- Category subfolders are shown by browsing into Downloads, not as roots of their own.
- The watch archive folder is not a root in this feature.
- Re-queue does not bypass the release check; it only lets it run again. Marking a release "Not
  fake" stays in History (042).
- Upload, move between folders and file download from the browser are out of scope.
