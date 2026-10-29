# Feature Specification: Hold Magnets and Ask the Cache Again

**Feature Branch**: `043-magnet-cache-retry`

**Created**: 2026-10-28

**Status**: Built (all five stories); NAS quickstart steps left after deploy

**Input**: User description: "Peer metadata fallback for magnet pre-check (follow-up to 042):
when the [precheck] caches lookup misses for a magnet link, fetch the info dictionary (file list)
directly from BitTorrent peers using DHT (BEP 5) and the ut_metadata extension (BEP 9/10), within
the time left of the add's 14 s budget (ADD_BUDGET, capped by lookup_timeout_secs). No seeding,
no piece data. Opt-in, default off, because it joins the DHT and reveals the info hash to peers;
docs must say so. Tests must not touch the network or use real torrents."

Rescoped after research (see Clarifications): "Hold the magnet and re-ask the cache, and shorten
the first cache ask. No DHT for now."

## Context

- 042 checks a release's file list before it reaches Put.io. A .torrent carries its file list.
  A magnet carries only the info hash, so 042 looks the .torrent up by hash in public caches
  (`[precheck] caches`, tried in order). On a miss the magnet goes to Put.io straight away and is
  checked there when the transfer finishes (042 US3). Until then Put.io downloads all of it,
  which is the bandwidth 042 exists to save.
- Research ([research.md](research.md)) measured on 2026-10-28, with legal torrents only:
  1. **A first-time miss uses up the add's time.** For a hash itorrents.net has not seen, the
     request does not answer within 10 s (`lookup_timeout_secs`, default 10). Hits answer in
     under 2 s, so the last 8 s of a miss find nothing.
  2. **itorrents.net fetches unknown hashes itself.** Of 17 hashes it did not have, it had 15
     about three minutes after the first ask and 16 after another 90 s. Most first-time misses
     are "not yet", not "never".
  3. **Peers can supply the file list**, but only about 57% within the time a miss leaves, the
     working client costs about 93 new crates, and it shows the NAS's IP and the info hash to the
     swarm. That option is deferred (see Out of Scope).
- So this feature holds a magnet that misses, instead of sending it to Put.io at once, and asks
  the cache again for a few minutes. Nothing new is revealed: the cache already saw the hash on
  the first ask.

## Clarifications

### Session 2026-10-28

- Q: Which fallback should this feature build: peers over the DHT, asking the cache again
  later, or both? → A: **B, cache re-ask only.** Hold the magnet and re-ask the cache, and
  shorten the first cache ask. No DHT for now. The peer research stays in research.md (R3–R7) for
  a later feature.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A magnet the cache doesn't have yet is still checked before Put.io (Priority: P1)

Sonarr sends a magnet. The cache doesn't have it on the first ask. Instead of handing it to
Put.io unchecked, putio-sync accepts the add, keeps the magnet, and asks the cache again every so
often. When the cache has it, the file list is checked as 042 does for a .torrent: a fake is
rejected and blocklisted without Put.io ever downloading it, and a real release goes to Put.io.

**Why this priority**: it is the whole point. Today every first-time miss reaches Put.io
unchecked, and fakes are usually new, so they are the magnets that miss most.

**Independent test**: a local test cache server that answers "not found" (or stalls) for a hash
on the first ask and serves a synthetic .torrent for it on a later ask. Add the magnet through
the Transmission API: no Put.io transfer is created at add time; after the re-ask the fake is
rejected and blocklisted with no Put.io call, and a real release is added to Put.io.

**Acceptance Scenarios**:

1. **Given** holding is on and the cache misses the first ask, **When** Sonarr adds a magnet,
   **Then** the add succeeds, no Put.io transfer is created, and Sonarr's queue shows the item
   as queued.
2. **Given** a held magnet, **When** a later ask finds a file list that fails the check, **Then**
   the item fails with the check's reason and is blocklisted, exactly as a rejected .torrent is
   in 042, and Put.io is never asked to add it.
3. **Given** a held magnet, **When** a later ask finds a file list that passes, **Then** the
   magnet is added to Put.io and from then on behaves like any checked add.
4. **Given** a held magnet, **When** the hold time ends with every ask missed, **Then** the
   magnet is added to Put.io and checked there when it finishes (042 US3), as today.
5. **Given** holding is off, **When** the cache misses, **Then** behaviour is exactly 042's.

---

### User Story 2 - The first ask doesn't waste the add's time (Priority: P1)

With holding on, the first ask during the add gets a short limit (a few seconds), since a miss
is retried later anyway. The add answers Sonarr sooner, and a magnet that came from a slow
redirect still gets its first ask.

**Why this priority**: a 10 s first ask that almost always times out on a miss delays every
first-time magnet add by 10 s for nothing, and with holding it is no longer the only chance.

**Independent test**: a test cache that stalls. With holding on, the add returns within the
short first-ask limit plus a small margin; with holding off, the first ask still gets the full
`lookup_timeout_secs`.

**Acceptance Scenarios**:

1. **Given** holding is on and the cache stalls, **When** a magnet is added, **Then** the first
   ask gives up after the first-ask limit and the add answers well within the *arr's timeout.
2. **Given** holding is on, **When** the cache has the hash, **Then** the first ask finds it and
   the add is checked as in 042 (hits answer in under 2 s, so the shorter limit loses none).
3. **Given** holding is off, **When** a magnet is added, **Then** the first ask keeps 042's
   limit.

---

### User Story 3 - Held items are handled like any other item (Priority: P2)

A held item shows in the *arr's queue and in the putio-sync UI, can be removed from Sonarr, and
survives a daemon restart.

**Why this priority**: without it a held item is invisible or lost, and the user can't tell why
a download hasn't started.

**Independent test**: hold a magnet, restart the service on the same database, and see the
re-asks continue. Remove a held item through the Transmission API and see the hold end with no
Put.io call.

**Acceptance Scenarios**:

1. **Given** a held item, **When** Sonarr lists its queue, **Then** the item is queued at 0%
   and does not count as stalled or failed.
2. **Given** a held item, **When** the user opens the Queue or Status page, **Then** it shows as
   "waiting for the file list" with how long it has left before it goes to Put.io unchecked.
3. **Given** a held item, **When** Sonarr removes it, **Then** the hold ends, nothing is sent to
   Put.io, and its pending check is closed as removed.
4. **Given** a held item, **When** the daemon restarts, **Then** the item is still held and the
   re-asks pick up where they left off; a hold whose time ran out during the restart goes to
   Put.io on the first pass.
5. **Given** a held item, **When** Sonarr adds the same magnet again, **Then** it is the
   duplicate 042 already reports, and no second hold starts.

---

### User Story 4 - Watch-folder magnets are held too (Priority: P3)

A `.magnet` file dropped in a watch folder that the cache misses is held the same way, then
checked or sent to Put.io.

**Why this priority**: the watch folder has no request deadline, so it gains the most, but its
pre-check is 042 US4.

**Independent test**: as 042 US4's watch test, with a test cache that misses first and hits
later.

**Acceptance Scenarios**:

1. **Given** 042 US4 is built and holding is on, **When** a `.magnet` file's first ask misses,
   **Then** the file is held, re-asked on the same schedule, and processed as a checked add on a
   hit or as 042 US4's fallback when the hold ends.
2. **Given** a held watch magnet, **When** its file is deleted from the folder, **Then** the
   hold ends with nothing sent to Put.io.

---

### User Story 5 - The operator can see whether holding pays off (Priority: P3)

The Stats page and metrics show how many held magnets were found on a later ask (checked before
Put.io), how many ran out of time (sent unchecked), and how long holds took.

**Why this priority**: it shows whether the hold time is right, and whether the deferred peer
lookup is ever worth building.

**Independent test**: drive held items to each outcome against a test cache and read the
counters and the Stats endpoint.

**Acceptance Scenarios**:

1. **Given** held magnets that ended each way, **When** the operator opens Stats, **Then** they
   see counts for found later, ran out of time, and removed, plus a typical time to found.
2. **Given** the metrics endpoint, **When** it is scraped, **Then** it has those counts and the
   number held now, with bounded labels.

### Edge Cases

- **The cache errors instead of missing** (5xx, DNS failure): treated as a miss; the hold goes
  on. Every ask erroring until the end gives the same fallback.
- **The cache serves a .torrent with the wrong hash**: not used (as 042); counts as a miss.
- **The cache is down for the whole hold**: the magnet goes to Put.io at the end, as 042 does
  after one miss today. The delay is the only cost.
- **Many magnets held at once** (a season pack search): asks are spread out and limited, so the
  cache isn't hit with a burst (FR-008).
- **The magnet came through a redirected URL**: held the same way; what is kept is the magnet.
- **A private magnet**: as 042, never sent to a public cache, so never held.
- **dry_run**: a held magnet whose later file list fails the check is logged as "would reject"
  and sent to Put.io, as a dry-run reject is in 042.
- **Precheck turned off or the category exempted while items are held**: the next pass sends
  them to Put.io unchecked.
- **Put.io refuses the add at the end of the hold** (out of space, bad magnet): the item fails
  with Put.io's message, as an add error does today, but asynchronously rather than in the add
  reply.
- **Hold time set to 0**: holding off.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: When a magnet's first cache ask misses and holding is on, putio-sync MUST accept
  the add without creating a Put.io transfer and keep the magnet for re-asking.
- **FR-002**: Holding MUST apply only where 042 would look a magnet up: precheck on, the
  category or watch folder checked, `magnet_lookup` on, and the magnet not private.
- **FR-003**: putio-sync MUST re-ask the caches for each held magnet on a schedule until one
  finds it or the hold time ends. The interval and the hold time MUST be configurable; holding
  is on by default with a hold time of 5 minutes and a re-ask every 30 seconds.
- **FR-004**: A file list found on a re-ask MUST go through the same check as 042 (policy,
  dry_run, reasons, blocklist, reports). A reject MUST NOT call Put.io.
- **FR-005**: A held magnet whose file list passes MUST be added to Put.io, in the category's
  folder, as 042 does for a passing magnet, and its check recorded as passed before Put.io.
- **FR-006**: When the hold time ends with no file list, the magnet MUST be added to Put.io with
  a pending check, as 042 FR-012 does today, so the check on Put.io (042 US3) still applies.
- **FR-007**: With holding on, the first ask during an add MUST use a separate, shorter limit
  (default 3 s, never more than `lookup_timeout_secs`). Re-asks MUST use `lookup_timeout_secs`.
  With holding off, the first ask keeps 042's limit.
- **FR-008**: Re-asks MUST be serialised or limited to a small number at once, and spaced, so
  many held magnets don't send a burst to a cache.
- **FR-009**: Held magnets MUST survive a daemon restart. A hold whose time ended while the
  daemon was down MUST be released on the first pass after start.
- **FR-010**: A held item MUST appear in the *arr's queue as queued at 0%, with no error, and in
  the UI with its held state and time left.
- **FR-011**: Removing a held item MUST end its hold, send nothing to Put.io, and close its
  pending check as removed.
- **FR-012**: Adding a hash that is already held MUST be reported as a duplicate, as 042 does
  for any hash already tracked.
- **FR-013**: When precheck is turned off, or the item's category stops being checked, held
  magnets MUST be sent to Put.io on the next pass.
- **FR-014**: Putting a held magnet into Put.io after the hold MUST NOT fail silently: a refused
  add MUST fail the item with Put.io's message and be logged.
- **FR-015**: Holding MUST NOT contact anything 042 doesn't already contact: only the configured
  caches, by info hash.
- **FR-016**: The daemon MUST count held magnets by outcome (found, ran out, removed), expose
  the number held now, and record the time each hold took, with bounded labels.
- **FR-017**: Each hold, re-ask result and release MUST be logged with the hash and release
  name, never the full magnet (as 042).
- **FR-018**: Watch-folder magnets MUST be held the same way once 042 US4 exists (User Story 4).
- **FR-019**: Tests MUST NOT use the network or real torrents: caches are local test servers and
  file lists synthetic.

### Key Entities

- **Held magnet**: a magnet accepted but not yet sent to Put.io. It has the info hash, the
  magnet text, where it came from (*arr category or watch folder), when the hold ends, how many
  asks were made, and when the next ask is due.
- **Release check** (042): the pending check row for the held magnet, settled when it is found
  (passed or rejected), or given its Put.io transfer when the hold ends.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In tests, a held magnet whose test cache starts serving it is checked and settled
  (rejected with no Put.io call, or added to Put.io) within one re-ask interval of the cache
  having it.
- **SC-002**: On real use, at least 80% of magnets that miss the first ask are found on a later
  ask within the default hold time (research R2 found 16 of 17), shown by the counters from
  FR-016.
- **SC-003**: With holding on, an add whose first ask misses answers the *arr within 5 s, down
  from about 10 s.
- **SC-004**: No held magnet is lost: after a restart, every held item is either still held or
  sent on, in tests and in the counters.
- **SC-005**: Holding adds no new network destination and no new dependency.

## Assumptions

- Magnets that miss the cache are a real share of *arr adds. 042's fallback counters (FR-022/023)
  will show how many. Building this before those numbers exist is a cheap bet, because holding
  costs no dependency and no privacy.
- A delay of up to 5 minutes before a magnet-only release starts is fine for Radarr and Sonarr.
  They don't treat a queued item as stalled within that time.
- itorrents.net keeps fetching hashes on demand. If it stops, holding simply ends in the 042
  fallback every time, which the counters will show.
- The *arr's add timeout stays above 15 s, as 042 assumes.
- The watch-folder part follows 042 US4 and is not built before it.

## Out of Scope

- **Fetching the file list from BitTorrent peers** (DHT, `ut_metadata`), as first briefed.
  Research R3–R7 cover the measurements, crate choice (librqbit, about 93 crates), privacy
  (reveals the NAS's IP and the info hash to DHT nodes and peers) and test approach. Revisit only
  if FR-016's counters show many magnets still run out of time after holding. It would be
  opt-in and off by default, with the disclosure in the UI and the docs.
- New caches beyond `[precheck] caches`.
- Holding .torrent adds (they carry their file list).
