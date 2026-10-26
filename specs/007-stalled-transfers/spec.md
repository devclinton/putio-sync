# Feature Specification: Stalled Put.io Transfer Detection and Re-add

**Feature Branch**: `007-stalled-transfers`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "Wonder if we can do anything for jobs on putio that get to 97+% and
freeze? I think if putio lets us, we could download files and possibly recover/repair some."
Option B of the feasibility study: stall detection (metric + UI badge) plus manual Re-add and
Copy magnet.

## Context

- Some Put.io transfers reach 97% or more and then stop progressing for days. The usual cause
  is that no peer in the swarm has the missing pieces (Put.io reports this as availability
  below 100%).
- A feasibility study (2026-10-26) checked the Put.io API: it gives no access to the data of an
  unfinished transfer. A transfer's files appear in the account only once it completes, so
  "download what's there and repair it" is not possible. What the API does give is enough to
  *recognise* a stall (bytes downloaded, speed, peers, availability, status and tracker
  messages) and the transfer's magnet link, which allows cancelling and adding it again.
- Retry, added in feature 004, applies to transfers in the error state; it does nothing for a
  transfer that is still "downloading" but frozen.
- Today the operator only notices a frozen transfer by happening to look at the Put.io page.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See which transfers are stalled (Priority: P1)

The operator opens the Put.io transfers page and sees, on any transfer that has stopped making
progress, a "Stalled" badge saying how long it has been stuck and the most likely reason
(for example "Stalled 9h · availability 92%"). Their monitoring stack can also alert on the
number of stalled transfers without anyone opening the page.

**Why this priority**: Nothing can be done about a stall nobody knows about; detection is useful
on its own, even before any action exists.

**Independent Test**: Against a mocked Put.io that reports a downloading transfer whose
downloaded bytes never change, advance time past the threshold and check that the transfer is
marked stalled in the API, the UI shows the badge, and the stalled-transfers metric reads 1.

**Acceptance Scenarios**:

1. **Given** a transfer in the downloading state whose downloaded bytes have not changed for
   longer than the stall threshold, **When** the operator opens the transfers page, **Then**
   that transfer shows a Stalled badge with the time since its last progress and a reason.
2. **Given** a transfer that was stalled, **When** its downloaded bytes increase, **Then** the
   badge disappears at the next check and the stalled count drops.
3. **Given** two stalled transfers, **When** the metrics endpoint is scraped, **Then** the
   stalled-transfers gauge reads 2 and carries no per-transfer labels.
4. **Given** a transfer that is queued, completed, seeding or in error, **When** it has not
   changed for longer than the threshold, **Then** it is not marked stalled.

---

### User Story 2 - Copy a transfer's magnet link (Priority: P2)

From a stalled (or any torrent) transfer, the operator clicks "Copy magnet" and gets the
transfer's magnet link on their clipboard, to add elsewhere or keep for later.

**Why this priority**: Cheap, never destructive, and the only way to take the torrent somewhere
else (another service or a local client) by hand.

**Independent Test**: Against a mocked Put.io returning a transfer with a magnet link, the
transfers listing includes the link and the UI's Copy magnet button copies it.

**Acceptance Scenarios**:

1. **Given** a torrent transfer with a magnet link, **When** the operator clicks Copy magnet,
   **Then** the link is copied and the UI confirms it.
2. **Given** a transfer with no magnet link (for example a plain URL download), **When** the
   page renders, **Then** no Copy magnet button is shown for it.

---

### User Story 3 - Re-add a stalled transfer (Priority: P2)

The operator clicks "Re-add" on a stalled transfer and confirms. putio-sync cancels the
transfer on Put.io and adds the same magnet again to the same destination folder, then shows
the new transfer. This forces Put.io to look for peers afresh; it will usually start again
from 0%.

**Why this priority**: The one recovery action the API allows. It is destructive (progress on
Put.io is lost), so it is manual and confirmed.

**Independent Test**: Against a mocked Put.io, re-add a transfer and check that exactly one
cancel and one add were sent, the add used the original magnet and destination folder, and the
response carries the new transfer.

**Acceptance Scenarios**:

1. **Given** a transfer with a magnet link, **When** the operator clicks Re-add and confirms,
   **Then** the old transfer is cancelled, a new one is added with the same magnet into the same
   folder, and the page shows the new transfer.
2. **Given** the operator clicks Re-add, **When** they dismiss the confirmation, **Then**
   nothing is sent to Put.io.
3. **Given** the cancel succeeded but adding the transfer again fails, **When** the result is
   shown, **Then** the operator sees that the old transfer is gone, why the add failed, and the
   magnet link so they can add it by hand.
4. **Given** a transfer without a magnet link, **When** Re-add is requested through the API,
   **Then** it is refused before anything is sent to Put.io.

### Edge Cases

- The daemon restarts: progress history is lost, so a transfer's stall clock starts again from
  the first check after restart (a stall is reported at most one threshold late).
- A transfer disappears from Put.io (completed and cleaned, or removed): its history is
  dropped and it no longer counts.
- Put.io is unreachable during a check: previous state is kept, nothing is marked stalled or
  un-stalled from missing data, and the error is logged.
- A transfer's reported bytes go down (Put.io restarted it): that counts as a change.
- Put.io returns the share ratio as text rather than a number: the transfer list still loads.
- Two Re-add requests for the same transfer arrive close together: the second fails cleanly
  (the transfer no longer exists) instead of adding the torrent twice.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The daemon MUST check the Put.io transfer list on a regular interval
  (default every 10 minutes) whether or not anyone has the UI open.
- **FR-002**: For each transfer in the downloading state, the system MUST remember when its
  downloaded byte count last changed.
- **FR-003**: A transfer MUST be considered stalled when it is in the downloading state and its
  downloaded byte count has not changed for at least the stall threshold (default 6 hours).
  Transfers in any other state MUST NOT be considered stalled.
- **FR-004**: Each stalled transfer MUST carry a reason, chosen in this order: availability
  below 100% ("availability N%"), no peers sending data ("no peers"), otherwise "no progress".
- **FR-005**: The stall threshold and the check interval MUST be configurable, with the
  defaults above documented in the example configuration.
- **FR-006**: The system MUST expose the number of currently stalled transfers as a metric,
  without per-transfer labels.
- **FR-007**: The transfers listing returned by the API MUST include, for each transfer,
  whether it is stalled, since when, the reason, and its magnet link when one exists.
- **FR-008**: The Put.io transfers page MUST show a Stalled badge with the elapsed time and
  reason on each stalled transfer.
- **FR-009**: The Put.io transfers page MUST offer Copy magnet on each transfer that has a
  magnet link.
- **FR-010**: The system MUST provide a Re-add action for a transfer that has a magnet link: it
  cancels the transfer on Put.io, then adds the same magnet to the same destination folder, and
  returns the new transfer.
- **FR-011**: The UI MUST ask for confirmation before Re-add, stating that progress on Put.io
  will be lost.
- **FR-012**: If adding again fails after the cancel succeeded, the response MUST say so and
  include the magnet link.
- **FR-013**: Re-add MUST be refused, with nothing sent to Put.io, when the transfer has no
  magnet link.
- **FR-014**: The Re-add action MUST require the same authentication as the other transfer
  actions, and MUST be logged with the transfer id and name.
- **FR-015**: Reading the transfer list MUST tolerate fields Put.io sends in more than one
  form (the share ratio as a number or as text).
- **FR-016**: Magnet links MUST NOT be written to logs (they identify the content); logs use
  the transfer id and name.

### Key Entities

- **Transfer progress record**: per transfer id, the last downloaded byte count seen and when
  it last changed. Held in memory by the daemon; dropped when the transfer leaves the list.
- **Stall status**: derived for a transfer: stalled or not, stalled-since time, and reason.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A transfer frozen in the downloading state is flagged within one check interval
  after the stall threshold passes (by default within 6 h 10 min of its last progress).
- **SC-002**: The operator can tell from the transfers page alone which transfers are stuck,
  for how long, and why, without opening Put.io.
- **SC-003**: Re-adding a stalled transfer takes one click plus a confirmation, and needs no
  visit to Put.io's site.
- **SC-004**: No transfer that is progressing, queued, completed, seeding or in error is ever
  flagged as stalled in the automated tests.
- **SC-005**: The stalled count is available to the monitoring stack so an alert can fire
  without anyone opening the UI.

## Assumptions

- Recovering data from an unfinished transfer is out of scope: the Put.io API does not expose
  it. par2 repair, handing the torrent to a local torrent client, and automatic re-add are also
  out of scope.
- Re-add cancels first and adds second, because Put.io is expected to refuse a second transfer
  for a torrent it is already downloading.
- The magnet link is taken from the transfer's magnet field, or from its source when that is a
  magnet link. Transfers added from an uploaded .torrent may have neither; they get no Copy
  magnet or Re-add.
- Progress history lives in memory only; losing it on restart delays detection by at most one
  threshold, which is acceptable for something measured in hours.
- Alert rules on the new metric are configured in the home-network monitoring stack, not in
  this repository.
