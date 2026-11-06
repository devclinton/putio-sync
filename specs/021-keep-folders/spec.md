# Feature Specification: Keep folders out of empty-folder cleanup

**Feature Branch**: `021-keep-folders`

**Created**: 2026-11-05

**Status**: Draft

**Input**: Roadmap 021: "A way to tell the empty-folder cleanup and the doctor to leave a
folder alone: a "keep" toggle on the Put.io page and in folder rules, plus ignore patterns
in Settings. Kept folders show in the cleanup preview with the reason."

## Context

- The empty-folder cleanup (feature 005) deletes every empty Put.io folder except a fixed
  set: the root, the watch upload folder, the sync folder, shared folders, save folders of
  unfinished transfers, and folders younger than the minimum age. There is no way to keep
  an empty folder the user wants, such as an empty `Movies/` or `TV/` that Radarr, Sonarr
  or the user saves into later.
- The doctor (feature 010) flags empty folders by running the same cleanup as a dry run,
  and its fix runs the cleanup. So whatever the cleanup keeps, the doctor leaves alone too.
- Folder rules (feature 009) already attach settings to a Put.io folder path, inherited by
  everything below it, and are edited from the rule card on the Put.io page and listed in
  Settings. Feature 020 added a pattern language for file names and paths.
- The cleanup report only counts kept folders (`kept_protected`, `kept_too_new`); it does
  not list them or say why.

## Clarifications

### Session 2026-11-05

- Q: Does keeping a folder also keep the folders inside it? → A: Yes. Keep is a folder
  rule setting like the others: it applies to the folder and everything below it, and a
  deeper rule can set it back to "no".
- Q: What do keep patterns match? → A: Folders, with the feature 020 pattern language: a
  term without `/` is tested against the folder name, a term with `/` against the folder
  path from the Put.io root (e.g. `Movies/*`). A folder is kept when it or any folder
  above it matches a pattern, so a pattern also keeps what is inside the matching folder.
- Q: Which kept folders does the preview list? → A: Empty folders the run would otherwise
  have removed, each with its reason: protected, too new, kept by a folder rule (naming
  the rule's path), or matching a keep pattern (naming the pattern). Folders that are not
  empty are never listed.
- Q: Does a rule's "keep: no" override a keep pattern? → A: No. It only clears an
  inherited "keep: yes" from a parent rule. A pattern keeps a folder whatever the rules say.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Keep one folder from the Put.io page (Priority: P1) 🎯 MVP

The user keeps an empty `Movies` folder on Put.io as the place Radarr saves into. Today the
daily cleanup deletes it whenever it is empty. While browsing Put.io in the web UI the user
opens `Movies`, turns on Keep on its folder rule, and saves. From then on the cleanup and
the doctor leave `Movies` and everything under it alone.

**Why this priority**: It is the core of the request and reuses the folder rule card the
user already knows.

**Independent Test**: Against a mocked Put.io account with an empty `Movies` folder and a
rule `keep = true` on `Movies`, run the cleanup as a dry run and check `Movies` is not in
the removed list and is in the kept list with the rule as the reason.

**Acceptance Scenarios**:

1. **Given** an empty, old folder `Movies` with a rule `keep = true`, **When** the cleanup
   runs, **Then** `Movies` is not deleted and is reported as kept by the rule on `Movies`.
2. **Given** `Movies` is kept and holds an empty `Movies/Old`, **When** the cleanup runs,
   **Then** `Movies/Old` is kept too (inherited), with the rule on `Movies` as the reason.
3. **Given** `Movies` is kept and `Movies/Old` has a rule `keep = false`, **When** the
   cleanup runs, **Then** `Movies/Old` is deleted and `Movies` is kept.
4. **Given** the user is on the Put.io page in folder `Movies`, **When** they press Keep
   folder on the rule card, **Then** the rule saves with keep on (other rule settings
   unchanged), the card shows "Keep out of cleanup: yes", and the next preview keeps it.
5. **Given** a kept folder, **When** the doctor's empty-folders check runs, **Then** the
   folder is not listed as removable, and the check's summary says how many empty folders
   were kept by keep settings.

---

### User Story 2 - Keep patterns in Settings (Priority: P2)

The user wants every empty `Season *` folder kept, and everything under `Software/`. In
Settings → Put.io cleanup they add the keep patterns `Season*` and `Software/*`, one per
line, and save. The cleanup keeps any folder whose name or path matches, and what is
inside it.

**Why this priority**: It covers many folders at once without a rule per folder, but the
per-folder toggle already solves the common case.

**Independent Test**: With keep patterns set, run a dry run against a mocked account and
check which folders are removed and which are kept with the pattern as the reason.

**Acceptance Scenarios**:

1. **Given** keep pattern `Season*` and empty folders `TV/Show/Season 1` and `TV/Old`,
   **When** the cleanup runs, **Then** `Season 1` is kept (reason: pattern `Season*`) and
   `TV/Old` is deleted.
2. **Given** keep pattern `Software/*` and an empty `Software/Tools/Old`, **When** the
   cleanup runs, **Then** it is kept.
3. **Given** a keep pattern the pattern checker refuses (e.g. an unclosed quote), **When**
   the user saves Settings, **Then** the save is refused with the checker's message.
4. **Given** keep patterns are saved, **When** the next run happens, **Then** it uses them
   without a restart.

---

### User Story 3 - See what was kept and why (Priority: P2)

Before deleting, the Put.io page's Remove empty folders preview tells the user how many
empty folders are kept and why, so a missing deletion is never a mystery. The CLI's
`clean-empty --dry-run` prints the same list.

**Why this priority**: It makes the keep settings visible and checkable.

**Independent Test**: Dry run against a mocked account with one protected, one too-new,
one rule-kept and one pattern-kept empty folder; check the report lists all four with
their reasons.

**Acceptance Scenarios**:

1. **Given** the preview finds 3 folders to remove and 2 kept, **When** the confirm dialog
   opens, **Then** it names the kept folders with their reasons (up to a few, then "and N
   more").
2. **Given** nothing to remove but 2 kept, **When** the preview runs, **Then** the notice
   says no empty folders to remove and that 2 were kept.
3. **Given** a dry run from the CLI, **When** it finishes, **Then** each kept folder is
   printed with its path and reason.

### Edge Cases

- A folder that is protected and also kept by a rule is reported once, as protected.
- A kept folder that is not empty is never listed; nothing about its contents changes.
- An empty folder inside a kept folder is kept; the cleanup does not delete it.
- Keep patterns are matched against the Put.io names with the same name clean-up rule
  paths use, so `Movies: Old` is matched as `Movies_ Old`, like folder rules.
- Without any keep settings, the cleanup removes exactly what it removed before.
- The root has a rule card but no rule; keep is not offered there (the root is never
  deleted anyway).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: A folder rule MUST accept a `keep` setting (yes, no, or inherit). It is
  inherited by sub-folders like the other rule settings.
- **FR-002**: `[cleanup]` MUST accept `keep_patterns`, a list of feature 020 patterns
  matched against each folder's name and path from the Put.io root. Each is checked on
  load and save with the 020 checker; refused text fails the save.
- **FR-003**: The cleanup MUST NOT delete a folder whose effective rule has keep on, or
  that, or any folder above it, matches a keep pattern.
- **FR-004**: The cleanup report MUST list every empty folder kept, with its id, path and
  reason: protected, too new, kept by a rule (with the rule path), or keep pattern (with
  the pattern). It MUST count rule- and pattern-kept folders as `kept_by_user`.
- **FR-005**: The doctor's empty-folders check and fix MUST follow the same keep settings
  (they reuse the cleanup) and the check's summary MUST mention user-kept folders.
- **FR-006**: The Put.io page's rule card MUST show the effective keep setting with its
  source, offer it in the rule editor, and have a one-click Keep folder / Don't keep button
  that changes only `keep`.
- **FR-007**: Settings → Put.io cleanup MUST offer the keep patterns, one per line.
- **FR-008**: The Remove empty folders preview MUST say how many folders were kept and show
  some of them with their reasons; `clean-empty` MUST print them.
- **FR-009**: Rule saves and cleanup settings saves MUST apply to the next cleanup run
  without a restart.

### Key Entities

- **Folder rule `keep`**: optional yes/no on `[[download.rules]]`, inherited.
- **Keep patterns**: `[cleanup] keep_patterns`, list of pattern strings.
- **Kept folder**: an entry in the cleanup report: id, path, reason text.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A folder marked Keep is never deleted by the daily run, a manual run or the
  doctor's fix, across every test scenario above.
- **SC-002**: The user can mark a folder Keep from the Put.io page in one click.
- **SC-003**: Every empty folder a run does not delete appears in the preview's kept list
  with a reason.
- **SC-004**: With no keep settings, existing cleanup tests pass unchanged.

## Assumptions

- Keep only affects the empty-folder cleanup and the doctor. Downloads, deletes after
  download and the storage free-up are unchanged.
- No new metric: kept folders are logged with structured fields; the run metrics stay.
- The rule editor's existing Inherit / Yes / No control is the "toggle in folder rules";
  the Settings folder-rules list shows it through its rule summary.
