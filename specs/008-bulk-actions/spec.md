# Feature Specification: Multi-select and Bulk Actions on the Put.io and Transfers Pages

**Feature Branch**: `008-bulk-actions`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "Add multi-select and bulk actions to two pages of the putio-sync
web UI. Put.io file browser: a checkbox on each row, a select-all checkbox, and a toolbar that
acts on the selected items: delete from Put.io (one confirm showing the count) and queue for
download. A file that is queued, downloading or paused stays protected the way the single
delete protects it (409). Transfers page: the same checkboxes, select-all and bulk actions (at
least remove and retry), plus one-click actions to clear transfers by status (all completed,
all failed, and similar), using Put.io's multi-id calls instead of one call per item where
Put.io allows it."

## Context

- Today every action on the Put.io and Transfers pages is per row. Tidying a folder of twenty
  finished downloads, or a transfer list full of completed and failed entries, takes twenty
  clicks and twenty confirmations.
- Put.io's API already takes several ids at once for deleting files (`/files/delete`),
  cancelling transfers (`/transfers/cancel`) and cleaning finished transfers
  (`/transfers/clean`, which answers with the ids it removed). Retry takes one id.
- The single delete on the Put.io page is refused (409) for files the queue still needs
  (pending, downloading or paused), and its Delete button is hidden for them.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Delete or queue several Put.io items at once (Priority: P1)

On the Put.io page the operator ticks several files and folders (or the select-all box), then
clicks Delete in the selection toolbar, confirms once in a dialog that states how many items
will be deleted, and they are gone. Or they click Download and all of them are queued.

**Why this priority**: Cleaning up Put.io after downloads is the most repeated chore in the UI.

**Independent Test**: Against a mocked Put.io, select three items, delete them, and check that
Put.io received one delete call carrying the three ids; with one of them queued, check that
nothing is deleted and the answer is 409 naming the queued item.

**Acceptance Scenarios**:

1. **Given** a folder listing, **When** the operator ticks two rows, **Then** the selection bar
   above the table shows "2 selected" and enables Download, Delete and Clear selection.
2. **Given** the select-all box, **When** it is ticked, **Then** every row shown (after the name
   filter) is selected; unticking it clears them. With only some selected it shows as mixed.
3. **Given** three selected items, **When** the operator clicks Delete, **Then** one confirm
   dialog says "Delete 3 items from Put.io?"; confirming sends one delete to Put.io and the
   listing reloads; cancelling sends nothing.
4. **Given** a file the queue still needs, **When** the page renders, **Then** its checkbox is
   disabled with a reason, so it cannot be selected for deletion.
5. **Given** a selection that includes a file that became queued after the page loaded, **When**
   the delete reaches the server, **Then** it is refused with 409, nothing is deleted, and the
   message names the protected item(s).
6. **Given** three selected items, **When** the operator clicks Download, **Then** each is
   queued and one toast sums up found / queued / skipped / blocked / failed across all of them.

---

### User Story 2 - Remove or retry several transfers at once (Priority: P1)

On the Transfers page the operator ticks several transfers (or select-all), then clicks Remove
(one confirm with the count) or Retry.

**Why this priority**: Same chore on the other page; failed transfers often come in batches
(a tracker going down).

**Independent Test**: Against a mocked Put.io, remove three transfers and check one cancel call
carrying the three ids; retry two and check two retry calls and a per-id result.

**Acceptance Scenarios**:

1. **Given** three selected transfers, **When** the operator clicks Remove and confirms,
   **Then** Put.io receives one cancel call with the three ids.
2. **Given** two selected failed transfers, **When** the operator clicks Retry, **Then** each is
   retried, and the toast says how many were retried and how many Put.io refused.
3. **Given** a selection, **When** the 5-second refresh redraws the list, **Then** the
   selection is kept for transfers still listed and dropped for those gone.
4. **Given** the operator clicks Remove, **When** they dismiss the confirmation, **Then**
   nothing is sent to Put.io.

---

### User Story 3 - Clear transfers by status (Priority: P2)

Above the transfer list the operator clicks "Clear completed", "Clear failed" or "Clear all
finished". A confirm states how many transfers that is; confirming removes them from the
Put.io transfer list. The downloaded files stay in the Put.io account.

**Why this priority**: The commonest bulk action needs no selection at all.

**Independent Test**: Against a mocked Put.io listing completed, seeding, failed and
downloading transfers, preview and run "clear completed" and check that only the completed and
seeding ids are sent, in one call.

**Acceptance Scenarios**:

1. **Given** two completed, one seeding, one failed and one downloading transfer, **When** the
   operator clicks Clear completed, **Then** the confirm says 3 transfers, and confirming sends
   one clean call carrying those three ids.
2. **Given** the same list, **When** the operator clicks Clear failed, **Then** only the failed
   transfer is cleared.
3. **Given** no transfer in the chosen state, **When** the button is clicked, **Then** a toast
   says there is nothing to clear and no confirm is shown.
4. **Given** a downloading transfer, **When** any clear runs, **Then** it is never touched.
5. **Given** Put.io's clean leaves some of the requested ids in place, **When** the clear runs,
   **Then** those are removed with one cancel call, and the result lists every id cleared.

### Edge Cases

- An empty id list is refused (400) before anything is sent to Put.io.
- Duplicate ids in a request are sent once.
- Navigating to another folder, or changing the filter, clears or narrows the selection to the
  rows shown; actions only ever apply to rows the operator can see.
- A bulk Download where some items fail still queues the rest; failures are counted.
- Without a database (dev mode) there is no queue, so the delete guard is skipped, as for the
  single delete.
- A transfer listed when the clear preview ran but gone by the time the clear runs: the server
  works from a fresh list each time, so it is simply not included.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The Put.io page MUST show a checkbox on each row and a select-all checkbox above
  the table (usable at phone width, where the table header is hidden) that selects every row
  currently shown.
- **FR-002**: The Put.io page MUST show a selection bar with the count and the actions
  Download, Delete and Clear selection, enabled only while at least one row is selected.
- **FR-003**: Bulk delete of Put.io items MUST ask for one confirmation stating the number of
  items, then delete them with a single request to Put.io.
- **FR-004**: Bulk delete MUST be refused (409) as a whole, with nothing deleted, when any
  requested file is pending, downloading or paused in the queue; the error MUST name the
  protected items. The UI MUST NOT let such rows be selected.
- **FR-005**: Bulk Download MUST queue every selected item and report the summed counts.
- **FR-006**: The Transfers page MUST show the same row and select-all checkboxes, and a
  selection bar with Retry, Remove and Clear selection, enabled while anything is selected.
  Retry applies to the selected transfers that have failed.
- **FR-007**: Bulk remove MUST ask for one confirmation stating the count and send one cancel
  request to Put.io with all ids.
- **FR-008**: Bulk retry MUST retry each selected transfer and report, per id, whether Put.io
  accepted it; one refusal MUST NOT stop the others.
- **FR-009**: The Transfers page MUST offer Clear completed (completed and seeding), Clear failed
  (error) and Clear all finished (both).
- **FR-010**: A clear MUST first report how many transfers match (dry run) so the UI can confirm
  with the count, and MUST only touch transfers in the chosen states.
- **FR-011**: A clear MUST use Put.io's multi-id clean call, falling back to one multi-id cancel
  call for any id the clean left in place.
- **FR-012**: The selection on the Transfers page MUST survive the periodic refresh for
  transfers still listed.
- **FR-013**: All new endpoints MUST require the API token like the other `/putio` routes.
- **FR-014**: Bulk actions MUST be logged with the count of ids (not the names).

### Key Entities

- **Selection**: a set of file or transfer ids held by the page; never sent anywhere except as
  the body of a bulk action.
- **Transfer status group**: `completed` (COMPLETED, SEEDING), `failed` (ERROR, FAILED),
  `finished` (both).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Deleting N selected Put.io items takes one click, one confirmation, and one call
  to Put.io regardless of N.
- **SC-002**: Clearing all completed transfers takes one click and one confirmation, with at
  most two calls to Put.io after the listing.
- **SC-003**: No queued, downloading or paused file is deleted by a bulk delete in the
  automated tests.
- **SC-004**: No transfer outside the chosen state group is cleared in the automated tests.

## Assumptions

- Bulk Download queues items one request at a time from the browser: queuing walks each folder
  on Put.io anyway, so a server-side batch would save no Put.io calls.
- "Clear" removes the transfer from Put.io's transfer list only; its files stay in the account.
  Seeding counts as completed (the files are already in the account).
- Put.io has no multi-id retry, so bulk retry makes one Put.io call per transfer.
- A select-all over hundreds of rows is fine: one Put.io call carries every id.
