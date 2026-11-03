# Feature Specification: Select All, Sort and Filter on the File and Transfer Lists

**Feature Branch**: `045-select-sort-filter`

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "The local files view, putio view, and transfers view should all
have select all, sorting, and filtering. Filtering should support glob with and or (ie
-*photoshop* *exe*)."

## Context

- Three pages list things the operator acts on in bulk: **Put.io** (one Put.io folder at a
  time), **Transfers** (every Put.io transfer, refreshed every 5 s) and **Local files** (one
  folder of the download or a watch folder, refreshed every 10 s; feature 044).
- Put.io and Transfers already have row checkboxes, a "Select all" box and bulk actions
  (feature 008): Download and Delete on Put.io, Retry and Remove on Transfers. Local files has
  per-row Delete, Rename and Re-check but no selection.
- Only Put.io has a filter today: a plain "contains" match on the name. No page can be sorted;
  rows come in the order the server sends them.
- A Put.io folder full of a release's extras, or a long transfer list, is hard to clean up one
  row at a time. The operator wants to say "everything with `exe` in the name that isn't
  Photoshop" and act on just those rows.
- Roadmap item 020 ("Glob filters with AND and OR") asks for the same pattern language in
  folder rules and settings. This feature builds the language for the three lists only; 020
  can reuse it later.

## Clarifications

### Session 2026-11-03

- Q: How do AND, OR and NOT combine in a filter? → A: Terms separated by spaces must all
  match (AND). `OR` or `|` between two terms makes them alternatives, and binds tighter than
  the space, so `*.mkv OR *.mp4 -sample` means "(mkv or mp4) and not sample". A leading `-`
  (or the word `NOT`) negates a term. The word `AND` is allowed and changes nothing. No
  parentheses.
- Q: Which characters are glob characters? → A: Only `*` (any run of characters, including
  none) and `?` (exactly one character). Square brackets are literal, because release names
  such as `[YTS]` use them. A term with a glob character must match the whole name; a term
  without one matches anywhere in the name, as today's Put.io filter does.
- Q: What does "Select all" cover? → A: Every row the filter shows that can be selected, and
  nothing the filter hides. Changing the filter unselects rows it hides.
- Q: Where do filtering and sorting run? → A: In the browser, over the rows already loaded.
  Each page already loads its whole list (one Put.io folder, all transfers, or one local
  folder up to its listing cap), so no server change is needed.
- Q: Do the filter and sort survive a reload? → A: Yes. Both go in the page's address
  (`?q=…&sort=…`) without reloading the page, so a reload, Back, or a bookmark keeps them.
  Opening another folder keeps the sort and clears the filter.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Filter a list with patterns (Priority: P1)

On any of the three pages, the operator types a filter such as `-*photoshop* *exe*` and the
list shows only names containing "exe" that don't contain "photoshop". The list heading says
how many rows match out of how many there are.

**Why this priority**: Filtering is what makes bulk actions safe: the operator narrows the
list, checks what is left, then acts.

**Independent Test**: Load a list with `Photoshop.exe`, `setup.exe`, `movie.mkv`; type
`-*photoshop* *exe*`; only `setup.exe` shows and the heading reads "1 of 3".

**Acceptance Scenarios**:

1. **Given** the Put.io page lists `Movie.2024.mkv`, `Movie.2024.sample.mkv` and `info.nfo`,
   **When** the operator types `*.mkv -sample`, **Then** only `Movie.2024.mkv` is shown.
2. **Given** the same list, **When** the operator types `*.mkv | *.nfo -sample`, **Then**
   `Movie.2024.mkv` and `info.nfo` are shown and the sample is hidden.
3. **Given** any list, **When** the operator types `movie` (no glob characters), **Then** every
   name containing "movie" in any case is shown.
4. **Given** the Transfers page refreshes every 5 s, **When** a filter is typed, **Then** it
   still applies after each refresh, and new transfers that match appear.
5. **Given** a filter that nothing matches, **When** the list renders, **Then** it says
   nothing matches the filter (not that the folder is empty).

---

### User Story 2 - Select all of what the filter shows (Priority: P1)

The operator filters a list, ticks "Select all", and runs a bulk action. Only the rows the
filter shows are selected and acted on. On Local files this brings selection and a bulk
Delete for the first time.

**Why this priority**: Selecting by filter is the point of the feature; acting on hidden rows
would delete things the operator never saw.

**Independent Test**: On Local files, filter to two of five files, Select all, Delete; the
confirm names the two files, and after it only the other three remain.

**Acceptance Scenarios**:

1. **Given** a filtered Put.io list, **When** the operator ticks Select all, **Then** every
   shown row that is not queued is selected and "N selected" counts only those.
2. **Given** rows are selected, **When** the operator changes the filter so some selected rows
   are hidden, **Then** those rows are unselected.
3. **Given** Local files with some rows selected, **When** the operator clicks Delete,
   **Then** one confirmation names how many files and folders will go (and that folders go
   with everything in them), and on confirm each is deleted and one toast sums the result.
4. **Given** a selected local entry is being written by a download or has a name the server
   can't act on, **When** the list renders, **Then** that row can't be ticked, as its own
   Delete button is already hidden.
5. **Given** the existing bulk actions (Put.io Download and Delete, Transfers Retry and
   Remove), **When** used on a filtered, sorted list, **Then** they behave as before on the
   selected rows.

---

### User Story 3 - Sort by a column (Priority: P2)

The operator clicks a column heading to sort by it, and clicks again to reverse it. An arrow
shows the column and direction. At phone width, where the table header is hidden, a "Sort"
menu next to the filter does the same.

**Why this priority**: Sorting by size or date finds the biggest or oldest items to clean up,
but filtering alone already covers the main need.

**Independent Test**: On Put.io, click "Size" twice; the largest item is first.

**Acceptance Scenarios**:

1. **Given** the Put.io page, **When** the operator sorts by Name, Size or Date, **Then** rows
   are ordered by it, with folders kept before files.
2. **Given** the Transfers page, **When** the operator sorts by Name, Status, Progress, Size or
   Added, **Then** rows are ordered by it.
3. **Given** the Local files page, **When** the operator sorts by Name, Size or Modified,
   **Then** rows are ordered by it, with folders kept before files.
4. **Given** a sort is chosen, **When** the page reloads or refreshes, **Then** the sort is
   kept.
5. **Given** two rows with the same sort value, **When** sorted, **Then** they are ordered by
   name, so refreshes don't shuffle them.

---

### Edge Cases

- An empty filter, or one made only of spaces, `-`, `OR` or `|`, shows every row.
- A dangling `OR` at either end, or two in a row, is ignored rather than an error.
- `-` alone is not a term; `-foo` negates "foo"; a `-` inside a name (`x-men`) is literal.
- A term in double quotes (`"two words"`) is one term and may hold spaces; a quoted `OR` or
  `-` is literal.
- Names with regex or glob-looking characters (`.`, `+`, `(`, `[`, `]`) match literally.
- Folders are matched on their name without the trailing `/` the page shows.
- A size shown as "—" (local folders) sorts as the smallest; a missing date sorts as oldest.
- A Local files folder listing cut off at the cap says so; filtering and sorting cover only
  the entries listed.
- A rename in progress on Local files pauses refreshes as today; filtering or sorting during
  it keeps the edit open.
- A filter in the address that came from an old bookmark is applied as typed.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The Put.io, Transfers and Local files pages MUST each have a filter box. The
  Put.io page's existing box MUST use the new filter language.
- **FR-002**: The filter MUST follow this grammar, matched case-insensitively against the
  row's name (file, folder or transfer name):
  - Terms are separated by spaces; a row must match every term (AND). The word `AND` is
    allowed and ignored.
  - `OR` (any case) or `|` between two terms makes them one alternative group; a group matches
    when any of its terms matches. OR binds tighter than AND.
  - A term starting with `-`, or preceded by the word `NOT`, matches when its pattern doesn't.
  - `*` matches any run of characters (including none) and `?` exactly one. A term with either
    must match the whole name; a term with neither matches anywhere in the name.
  - Double quotes group a term that holds spaces, or make `OR`, `AND`, `NOT` or a leading `-`
    literal.
  - Every other character is literal. Malformed input never errors; stray operators are
    ignored.
- **FR-003**: Each page's heading MUST show "N of M items" (or transfers) while a filter hides
  rows, and the empty state MUST say that nothing matches the filter.
- **FR-004**: "Select all" MUST select every shown row that can be selected, and none the
  filter hides. A filter change MUST unselect rows it hides.
- **FR-005**: The Local files page MUST have row checkboxes, Select all and a bulk Delete.
  Rows whose Delete is hidden today (in use, or a name the server can't act on) MUST NOT be
  selectable. Bulk Delete MUST ask once, naming the count and up to five names, and say that
  folders go with their contents; then delete each entry and report the result in one toast,
  naming any that failed.
- **FR-006**: Each page MUST sort by these columns, ascending or descending:
  Put.io: Name, Size, Date; Transfers: Name, Status, Progress, Size, Added; Local files:
  Name, Size, Modified. Clicking a sortable heading MUST sort by it, and clicking it again
  MUST reverse it. Sortable headings MUST be keyboard-operable and announce the current sort
  to screen readers. A Sort control next to the filter MUST offer the same choices for phone
  width.
- **FR-007**: Without a chosen sort, each page MUST keep its current order. With one, ties
  MUST break on name; on Put.io and Local files, folders MUST stay before files.
- **FR-008**: The filter and sort MUST be kept in the page's address as `q` and `sort` and
  restored on load, without reloading the page while typing. Links into another folder MUST
  keep `sort` and drop `q`.
- **FR-009**: Filtering and sorting MUST survive the pages' automatic refreshes, and the
  selection MUST survive them for rows still shown, as it does today.
- **FR-010**: The existing per-row and bulk actions on Put.io and Transfers MUST keep working
  on filtered and sorted lists.
- **FR-011**: The filter and sort logic MUST be shared by the three pages, and the filter
  parser MUST have automated tests covering the grammar and the edge cases above.

### Key Entities

- **Filter**: the text typed, parsed into AND-ed groups of OR-ed terms, each term a pattern
  that may be negated.
- **Sort**: a column key and a direction, e.g. `size-desc`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The operator can reduce any of the three lists to the rows they want and act on
  all of them with three actions: type a filter, tick Select all, click the action.
- **SC-002**: The user's example `-*photoshop* *exe*` shows exactly the rows whose names
  contain "exe" and not "photoshop", on all three pages.
- **SC-003**: No bulk action ever reaches a row the filter hid.
- **SC-004**: Filtering and sorting a list of 1,000 rows updates the list with no noticeable
  delay while typing.
- **SC-005**: After a reload, the list shows the same filter and sort as before.

## Assumptions

- Filtering and sorting run over what each page already loads; Put.io folders are listed in
  one request, which is what the page shows today. Folders past that size are out of scope.
- Matching uses the displayed name only, not paths, sizes or statuses.
- The bulk Delete on Local files uses the existing single-entry delete for each entry; no new
  server route is needed.
- Other lists (Queue, History, Active) are out of scope; they can adopt the shared helper
  later.
- Folder rules and settings (roadmap 020) are out of scope.
