# Feature Specification: Command palette

**Feature Branch**: `061-command-palette`

**Created**: 2026-11-13

**Status**: Draft

**Input**: User description: "A command palette for the web UI, like Cmd+P in VS Code and other
apps: one shortcut from any page searches places (pages and settings), actions, queue items,
put.io files and local synced files, and picking a result goes there or runs it." The agreed
design, with the calls made for it, is in [design.md](design.md).

## Context

- The web UI has 16 pages behind a hash router, a Settings page with its own search box that
  only works on that page (it scans the settings panels that are loaded), and buttons for
  actions such as sync now, pausing and resuming local downloads, and clearing completed items.
- The Queue page can already be opened filtered to one file, and the Files page can already be
  opened at a folder inside a sync root. Nothing in the UI or the API searches put.io files or
  local files by name.
- The UI is translated through catalogs keyed by English text (051), and works at phone width
  and in light and dark themes.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Jump to a page, setting or action (Priority: P1)

From any signed-in page, the user presses Cmd+K (Ctrl+K off macOS), types a few letters such as
"subt" or "bandwidth", and presses Enter. The palette opens the matching page or settings card,
or runs the matching action, without the user clicking through menus.

**Why this priority**: this is the core palette pattern the user asked for, and it needs no new
server work.

**Independent Test**: on any page, open the palette, type part of a page name, a setting's
label and an action's name in turn, and check that each lands on or runs the right thing using
only the keyboard.

**Acceptance Scenarios**:

1. **Given** the user is on the Status page, **When** they press Cmd/Ctrl+K, type "queue" and
   press Enter, **Then** the Queue page opens and the palette closes.
2. **Given** the user is on the Queue page, **When** they open the palette and type
   "bandwidth", **Then** the matching settings card is listed, and picking it opens Settings at
   that card.
3. **Given** the palette is open, **When** the user types "subt", **Then** results whose names
   contain those letters in order, such as "Subtitles", are listed.
4. **Given** the palette lists "Clear completed", **When** the user picks it, **Then** a
   confirmation asks "Remove completed items?" and nothing is removed unless they confirm.
5. **Given** the palette lists "Sync now", **When** the user picks it, **Then** a sync starts
   with no confirmation, the same as the button.
6. **Given** the palette is open, **When** the user presses Esc, **Then** it closes and focus
   returns to where it was.

---

### User Story 2 - Find a queue item (Priority: P2)

The user remembers part of a file name that is waiting or downloading. They open the palette,
type part of the name, and pick the queue item, which opens the Queue page showing that item.

**Why this priority**: the queue is the list people check most, and it needs no new server
work.

**Independent Test**: with a few items in the queue, type part of one file name and check that
the item is listed and that picking it opens the Queue page filtered to that item.

**Acceptance Scenarios**:

1. **Given** the queue holds "Some.Show.S01E02.mkv", **When** the user types "s01e02", **Then**
   that item is listed under Queue.
2. **Given** a queue result is selected, **When** the user presses Enter, **Then** the Queue
   page opens showing only that item.

---

### User Story 3 - Find a put.io file (Priority: P2)

The user types part of the name of a file in their put.io account. After a short pause the
palette lists matching put.io files, and picking one opens the Put.io page in that file's
folder with the file marked.

**Why this priority**: it saves browsing put.io folder by folder, but it needs a new search on
the server.

**Independent Test**: against a mocked put.io account, type part of a file name and check that
matches are listed, and that picking one opens its folder with the file marked.

**Acceptance Scenarios**:

1. **Given** put.io holds "Movie (2024)/Movie.mkv", **When** the user types "movie" and pauses,
   **Then** the file is listed under Put.io files.
2. **Given** a put.io result is picked, **When** the Put.io page opens, **Then** it shows the
   file's folder with that file marked.
3. **Given** put.io is slow or fails, **When** the user is typing, **Then** the other groups
   still show and the Put.io group says it could not search, without blocking typing.

---

### User Story 4 - Find a local synced file (Priority: P3)

The user types part of the name of a file under their sync folders. The palette lists matching
local files, each showing which sync folder it is in, and picking one opens the Files page in
that file's folder with the file marked.

**Why this priority**: useful for finding where something landed, and it needs a bounded disk
search on the server.

**Independent Test**: with a temporary sync folder holding a few nested files, type part of a
name and check that the matches are listed with their sync folder, and that picking one opens
its folder with the file marked.

**Acceptance Scenarios**:

1. **Given** a sync folder holds "TV/Show/ep1.mkv", **When** the user types "ep1" and pauses,
   **Then** the file is listed under Local files with its sync folder's name.
2. **Given** a sync folder holds more matching files than the limit, **When** the search ends,
   **Then** the palette shows the first matches and says that more were not shown.
3. **Given** a sync folder holds hidden files or shortcuts (symlinks) to other places, **When**
   the user searches, **Then** hidden files are not listed and shortcuts are not followed.

---

### Edge Cases

- The shortcut is pressed on the login, setup or sign-in pages, or while a confirmation dialog
  is open: nothing happens.
- The shortcut is pressed while typing in a text field: the palette opens.
- The user types fewer than 2 letters: only actions, pages, settings and queue items are
  listed; no file search starts.
- The user keeps typing: results from an earlier, slower file search never replace results for
  the current text.
- Nothing matches: the palette says nothing matched.
- Sign-in is Tailscale-only: "Sign out" is not listed.
- There is no database (dev mode) or the queue cannot be read: the Queue group is left out and
  the rest still works.
- No sync folders are set up: the Local files group is left out.
- The window is phone width: the palette fits the screen and can be used by touch.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The UI MUST open the palette from any signed-in page with Cmd+K on macOS and
  Ctrl+K elsewhere, including while focus is in a text field, and MUST NOT open it on the
  login, setup or sign-in pages or while a confirmation dialog is open.
- **FR-002**: The palette MUST close on Esc, on picking a result, and on clicking outside it,
  and MUST return focus to where it was.
- **FR-003**: The palette MUST list results in groups in this order: Actions, Pages, Settings,
  Queue, Put.io files, Local files, and MUST hide groups with no results.
- **FR-004**: Matching on names MUST be loose: results whose names contain the typed letters in
  order MUST match, case-insensitively, with closer matches listed first.
- **FR-005**: The user MUST be able to move through results with the arrow keys, pick one with
  Enter, and pick one by clicking or tapping it.
- **FR-006**: The Pages group MUST include every page in the main navigation; picking one MUST
  open it.
- **FR-007**: The Settings group MUST find settings cards and fields from any page, not only on
  the Settings page; picking one MUST open Settings at that card or field. The Settings page's
  own search MUST use the same list, so the two never disagree.
- **FR-008**: The Actions group MUST include at least: Sync now, Pause downloads, Resume
  downloads, Clear completed, Check for updates, Toggle theme and Sign out. Each MUST do the
  same as the existing control for it.
- **FR-009**: "Clear completed" MUST ask for confirmation ("Remove completed items?") before
  running; the other listed actions MUST run without asking, as their controls do.
- **FR-010**: "Sign out" MUST NOT be listed when sign-in is Tailscale-only.
- **FR-011**: "Check for updates" MUST open the Update page and start a check there.
- **FR-012**: The Queue group MUST match queue items by file name; picking one MUST open the
  Queue page filtered to that item.
- **FR-013**: The Put.io files group MUST search the user's put.io files by name once 2 or more
  letters are typed and typing pauses (about a quarter of a second), MUST return at most 50
  results, and MUST leave out files in the put.io trash.
- **FR-014**: Picking a put.io file MUST open the Put.io page in that file's folder with the
  file marked.
- **FR-015**: The Local files group MUST search file and folder names under all configured sync
  folders once 2 or more letters are typed and typing pauses, and MUST show each result's sync
  folder name.
- **FR-016**: The local search MUST stop at 50 matches or 2 seconds, whichever comes first,
  across all sync folders together, and the palette MUST say when it stopped early.
- **FR-017**: The local search MUST NOT follow shortcuts (symlinks), MUST skip hidden files and
  folders, and MUST NOT return anything outside the configured sync folders.
- **FR-018**: Picking a local file MUST open the Files page in that file's folder with the file
  marked.
- **FR-019**: A slow or failed file search MUST NOT block typing or the other groups, and
  results for older text MUST NOT replace results for the current text.
- **FR-020**: Both file searches MUST require the same sign-in as the rest of the API.
- **FR-021**: All palette text MUST be translatable through the web UI's language catalogs.
- **FR-022**: The palette MUST work at phone width, in both themes, and with a screen reader
  (it is announced as a dialog with a search box and a list of options, and the selected
  option is announced).

### Key Entities

- **Palette result**: one row the user can pick: its group, the name shown, an optional detail
  (a settings section, a sync folder name, a folder path), and what picking it does (go to a
  place or run an action).
- **Settings entry**: one findable setting: its section, its card, its field key (if any) and
  its label, as one list shared by the palette and the Settings page.
- **File match**: a file or folder found by a search: its name, where it lives (a put.io folder,
  or a sync folder and path), whether it is a folder, and its size.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From any signed-in page, a user reaches any page, settings card or action with one
  shortcut, a few letters and Enter, without using the mouse.
- **SC-002**: Results from actions, pages, settings and the queue appear as the user types, with
  no noticeable delay for a queue of 1,000 items.
- **SC-003**: File results appear within 1 second of a pause in typing on a normal home
  connection, and a local search never takes longer than 2 seconds, even over a sync folder with
  hundreds of thousands of files.
- **SC-004**: Typing in the palette never freezes or stutters, even while a file search is slow
  or fails.
- **SC-005**: No action that removes data runs on a single Enter press without a confirmation.

## Assumptions

- Cmd/Ctrl+K is chosen over Cmd+P because the browser uses Cmd+P to print. Other browser
  shortcuts are left alone.
- "Pause downloads" and "Resume downloads" mean the local downloads pause, not the bandwidth
  pause, and are listed as two entries, so the palette needs no state when it opens.
- Picking a queue item opens the Queue page's existing filtered view; a highlighted row in the
  full list is not needed.
- Only the first page of put.io's search results is used.
- The local search walks the sync folders on disk, the same places the Files page shows, rather
  than the download history, so files the user put there themselves are found.
- Out of scope: running queue actions (pause, retry, skip) on an item from the palette,
  searching file contents, history, logs or transfers, user-defined commands or shortcuts, and
  a recent-items list.
