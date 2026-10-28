# Feature Specification: Magnet Metadata from Peers

**Feature Branch**: `043-magnet-peer-metadata`

**Created**: 2026-10-28

**Status**: Draft (spec and research only; no plan until the user approves)

**Input**: User description: "Peer metadata fallback for magnet pre-check (follow-up to 042):
when the [precheck] caches lookup misses for a magnet link, fetch the info dictionary (file list)
directly from BitTorrent peers using DHT (BEP 5) and the ut_metadata extension (BEP 9/10), within
the time left of the add's 14 s budget (ADD_BUDGET, capped by lookup_timeout_secs). No seeding,
no piece data. Opt-in, default off, because it joins the DHT and reveals the info hash to peers;
docs must say so. Tests must not touch the network or use real torrents."

## Context

- 042 checks a release's file list before it reaches Put.io. A .torrent carries its file list.
  A magnet carries only the info hash, so 042 looks the .torrent up by hash in public caches
  (`[precheck] caches`, tried in order). On a miss the magnet goes to Put.io and is checked there
  when the transfer finishes (042 US3). Until then Put.io downloads all of it, which is the
  bandwidth 042 exists to save.
- Only one cache still works: itorrents.net (042 research R5, re-probed 2026-10-28).
- 042's clarifications said no to fetching metadata from peers: it needs new dependencies and
  puts the NAS's IP in the swarm of every release, which Put.io exists to avoid. This feature
  reopens that question as an **opt-in** setting that is **off by default**, and asks whether it
  is worth building at all.
- Research for this spec ([research.md](research.md)) measured three things on 2026-10-28 with
  legal torrents (Linux ISOs, Blender open films):
  1. **A cache miss uses up the add's time budget.** For a hash itorrents.net has not seen,
     the request does not answer within 10 s (`lookup_timeout_secs`, default 10). After a 10 s
     miss, 4 s of the 14 s budget are left.
  2. **itorrents.net fetches unknown hashes itself.** Of 17 hashes it did not have, it had 15
     about three minutes later and 16 after another 90 s. Most first-time misses are "not yet"
     rather than "never".
  3. **Peers can supply the file list fast from a cold start.** A mature client library, in
     list-only mode using only the DHT, got the file list for 22 of 23 torrents within 14 s
     (median 3.9 s, slowest 13.9 s), but only 13 of 23 within the 4 s a cache miss leaves. A minimal hand-written client got only 7 of 23 because
     popular seeders closed the connection.

## Clarifications

- Q: Which fallback should this feature build: peers over the DHT, asking the cache again
  later, or both? → A: [NEEDS CLARIFICATION: Research finding 2 suggests a cheaper fallback:
  hold the magnet locally for a few minutes and ask the cache again, with no DHT, no new
  dependencies and no change to privacy. Options: (A) peers only, as briefed; (B) cache re-ask
  only, and drop peer fetch for now; (C) both, cache re-ask first and peers as the opt-in last
  resort. The recommendation is B, with C later only if 042/043 telemetry shows enough misses
  are left.]

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A magnet the cache doesn't have is still checked before Put.io (Priority: P1)

The operator has turned on peer lookup. Sonarr sends a magnet link and the cache does not have its
.torrent. putio-sync asks BitTorrent peers for the release's file list. It gets the list within the
add's time limit and checks it exactly as it checks a .torrent. A fake (for example a single
1.4 GB `.exe`) is rejected and never reaches Put.io.

**Why this priority**: this is the feature. Every magnet that misses the cache today costs Put.io
the whole download before it can be checked.

**Independent Test**: with peer lookup on and a fake DHT and a fake peer on the local machine,
both serving a synthetic file list with only an `.exe`, add the magnet. It is rejected as in 042
User Story 1, with "peers" recorded as where the file list came from, and no Put.io transfer is
made.

**Acceptance Scenarios**:

1. **Given** peer lookup is on and the cache misses, **When** a peer sends the file list in time,
   **Then** the release is checked as in 042 (rejected or let through) before anything reaches
   Put.io.
2. **Given** peer lookup is on, **When** a peer sends a file list whose hash is not the
   magnet's, **Then** that list is thrown away and other peers are tried.
3. **Given** peer lookup is on, **When** no peer sends the file list before the time limit,
   **Then** the magnet falls back as 042 does today (sent to Put.io, checked there before the
   NAS downloads it), and the add still answers the *arr in time.
4. **Given** peer lookup is off (the default), **When** the cache misses, **Then** nothing is
   sent to the DHT or to any peer, and 042 behaves exactly as it does without this feature.

---

### User Story 2 - The operator knows what turning it on reveals (Priority: P1)

The setting's description, `config.example.toml` and the docs say in plain words what turning
peer lookup on means. The NAS joins the BitTorrent DHT for a few seconds, the DHT nodes it asks
and the peers it contacts see the NAS's public IP and the release's info hash, and copyright
monitors may log that. The setting is off until the operator turns it on.

**Why this priority**: 042 avoids exposing the NAS in swarms. Turning that off without telling
the operator would break the reason many people use Put.io.

**Independent Test**: read the docs page, the config example and the settings screen. Each one
states the default (off) and what is revealed to whom. A fresh install with no config has peer
lookup off.

**Acceptance Scenarios**:

1. **Given** a fresh install, **When** the daemon starts, **Then** peer lookup is off and no DHT
   traffic is sent.
2. **Given** the operator reads the setting in the web UI or the example config, **When** they
   look for what it does, **Then** they find the privacy note next to the switch, not only in
   separate docs.

---

### User Story 3 - The operator can see whether it is paying off (Priority: P2)

On the Stats page and in metrics, the operator sees how many magnets got their file list from
peers, how many timed out, how long it took, and how many of those found by peers were rejected
as fake. With 042's fallback counters, this shows whether peer lookup saves Put.io traffic worth
the privacy cost.

**Why this priority**: the case for this feature rests on numbers nobody has yet: how many real
*arr magnets miss the cache, and how many of those are fakes.

**Independent Test**: run several adds against the local fakes (found, timed out, wrong hash)
and check that the counts and timings appear in metrics and on the Stats page.

**Acceptance Scenarios**:

1. **Given** peer lookups have run, **When** the operator opens Stats, **Then** they see found
   and missed counts by reason, and how many found releases were rejected.
2. **Given** metrics are scraped, **When** peer lookups run, **Then** counters by result and a
   timing histogram are exposed with bounded labels (no hashes or names).

---

### Edge Cases

- **No time left**: the cache lookup used the whole budget (a 10 s miss leaves 4 s). Peer lookup
  gets what is left of the add's time limit. Under a minimum it is skipped with result "no time"
  and the magnet falls back as in 042.
- **DHT can't start** (UDP blocked, no network, all bootstrap nodes down): the lookup fails fast
  with result "unreachable". The add is never delayed past its time limit, and the daemon keeps
  running.
- **Many adds at once** (Sonarr season search): a cap on lookups running at once. Adds past the
  cap skip peer lookup with result "busy" rather than queue past their time limit.
- **Huge file list** (a season pack with thousands of files): the file list size is capped (as
  042 caps .torrent size at 10 MB); a peer that claims more is dropped.
- **Malicious peer**: sends garbage, a wrong-hash list, or stalls. Each peer has its own short
  time limit. Only a list whose hash matches the magnet is used.
- **Private torrents**: magnets from private trackers must not use the DHT. Those are normally
  .torrent files, but a magnet that marks itself private, or names only a private tracker, is
  never looked up on the DHT.
- **Trackers in the magnet**: the magnet's `tr=` trackers are not contacted. A tracker request
  is an announce and would list the NAS as a peer.
- **Watch folder magnets**: no *arr is waiting on them, so peer lookup there may use a longer
  time limit than the *arr add.
- **Dry run** (`[precheck] dry_run`): peer lookup still runs when on, and its result is counted
  and logged like a normal check. Nothing is rejected.
- **Config reload**: turning peer lookup off takes effect for the next add. Lookups in flight
  finish or time out.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Peer lookup MUST be a setting that is **off by default**. With it off, putio-sync
  MUST NOT send any DHT or peer traffic.
- **FR-002**: With it on, putio-sync MUST try peer lookup for a magnet only after every
  configured cache missed (or when magnet cache lookup is off), and only for releases the 042
  check covers (the category or watch folder has the check on).
- **FR-003**: Peer lookup MUST get the release's file list (the info dictionary) only. It MUST
  NOT request, receive or store piece data, and MUST NOT upload anything to peers.
- **FR-004**: Peer lookup MUST NOT announce the NAS as a peer, to the DHT or to a tracker, and
  MUST NOT contact the magnet's trackers.
- **FR-005**: putio-sync MUST NOT accept incoming peer connections for this feature or open a
  listening port for them, and MUST NOT ask the router to open one (no UPnP or NAT-PMP).
- **FR-006**: A file list MUST be used only if its hash equals the magnet's info hash. Lists
  that don't match MUST be thrown away and counted.
- **FR-007**: A file list from peers MUST be checked exactly as a .torrent's is in 042 (same
  policy, same reject and report path), with its source recorded as "peers".
- **FR-008**: For an *arr add, peer lookup MUST finish within what is left of the add's time
  limit (042's 14 s deadline), capped by a peer lookup timeout setting. The add MUST still
  answer the *arr within 15 seconds in every case (042 SC-003).
- **FR-009**: When the cache lookup would leave peer lookup less than its minimum useful time,
  the cache lookup's share of the budget MUST be shortened so peer lookup gets a fair share
  (research R1: a cache miss today takes the whole 10 s). The split MUST be configurable.
- **FR-010**: When peer lookup does not find the file list, the magnet MUST fall back exactly
  as 042 FR-012 says (sent to Put.io, checked there before the NAS downloads it).
- **FR-011**: The number of peer lookups running at once, the peers contacted per lookup and the
  file list size MUST be capped. Lookups over the concurrency cap are skipped, not queued.
- **FR-012**: Peer lookup MUST NOT run for magnets marked private.
- **FR-013**: DHT state (routing table, node id) MUST NOT be written to disk. When no lookup is
  running, the daemon MUST NOT be a DHT node.
- **FR-014**: The settings screen, `config.example.toml` and a page in `docs/` MUST say that the
  setting is off by default and what turning it on reveals: the NAS's public IP and each looked
  up info hash, to the DHT nodes asked and the peers contacted, which may include copyright
  monitors. The web UI MUST show this next to the switch.
- **FR-015**: The daemon MUST count peer lookups by result (found, timed out, no peers, wrong
  hash, no time, busy, unreachable) and record how long they took. It MUST also count how many
  found releases were rejected, and show these with 042's fallback numbers on the Stats page.
  Labels MUST stay bounded (Constitution V).
- **FR-016**: Each peer lookup MUST be logged with its hash, result, time taken, and peers tried
  and answered. Peer IP addresses MUST NOT be logged above debug level.
- **FR-017**: Automated tests MUST NOT touch the network or use real torrents (Constitution
  III). They MUST run against a DHT and peers on the local machine serving synthetic file lists.

### Key Entities

- **Peer lookup**: one attempt to get a magnet's file list from peers. It has an info hash, a
  result, a duration, peers tried and peers answered. It is recorded on 042's release check as
  the lookup result, with source "peers".
- **Peer lookup settings**: on or off (default off), the timeout, the cap on lookups at once,
  and how the add's time is split between the cache and peers.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With peer lookup on, at least 80% of magnets that miss the cache get their file
  list before the *arr add's time limit, measured over a week of real use.
- **SC-002**: Every *arr add still answers within 15 seconds, with peer lookup on or off,
  including when the DHT is unreachable.
- **SC-003**: With peer lookup off (the default), putio-sync sends zero DHT or peer packets,
  checked by a test and by watching the container's traffic.
- **SC-004**: Bytes Put.io downloads for magnets later rejected on Put.io (042 US3) drop by at
  least half against the week before peer lookup was turned on.
- **SC-005**: An operator can find what the setting reveals, and that it is off by default, from
  the settings screen alone.

## Assumptions

- The NAS reaches the internet over UDP from the container (Docker bridge networking with
  outbound NAT). No inbound port is needed (research R6).
- Magnets that miss the cache are a real share of *arr adds. Nobody has measured this yet. 042
  FR-022 counters will show it once 042 has run for a while, and this feature should not be
  planned before those numbers exist.
- IPv4 only at first. IPv6 DHT is out of scope.
- A SOCKS5 proxy for peer connections is out of scope. It would hide the IP from peers but not
  from DHT nodes (research R5).
- The *arr's add timeout stays above 15 s, as 042 assumes.
