# Research: Hold Magnets and Ask the Cache Again (043)

This research started as "get magnet metadata from peers". Its findings led to the rescope
(spec Clarifications, 2026-10-28): build option B, hold and re-ask the cache. R3–R7 are kept for
the deferred peer lookup. R9 covers the design questions for B.

All probes ran on 2026-10-28 from the maintainer's Mac (home broadband, IPv4, no port
forwarding), not from the NAS. They used only legal torrents: Debian and Ubuntu-family ISOs
(info hashes computed from the .torrent files on releases.ubuntu.com and cdimage.debian.org) and
the Blender open films (Sintel, Big Buck Bunny, Cosmos Laundromat, Tears of Steel). The probe
code is throwaway and lives outside the repo. The probe sources and raw output were kept in the session notes, not in the repo.

## R1. How much time a peer lookup actually gets

**Finding**: a cache miss usually uses the whole cache budget, so peer lookup would get about
4 s.

- The add deadline is 14 s (`ADD_BUDGET`, `putio-sync-arr/src/service.rs:40`). The cache lookup
  gets what is left, capped at `lookup_timeout_secs` (default 10, range 1..=30).
- For a hash it already has, itorrents.net answers in 40–60 ms.
- For a hash it doesn't have, it **does not answer within 10 s**: 16 of 17 fresh hashes timed
  out at exactly 10 s. A made-up hash got a 302 to an HTML page after 12.2 s.
- So after a typical miss, 14 − 10 = 4 s are left. A magnet that came from a redirected URL has
  less (the URL fetch came first).

**Consequence**: peer lookup is only useful if the cache gets a smaller share of the budget.
The spec's FR-007 gives the first ask its own shorter limit when holding is on. A cache that misses by timing out tells us nothing, so
shortening it to around 3–4 s loses almost no hits (hits arrive in under 2 s).

## R2. itorrents.net fetches what it doesn't have

**Finding**: most first-time misses are "not yet", not "never".

| Ask | Hits (of 17 fresh hashes) |
|---|---|
| First ask, 10 s limit | 1 |
| ~3 min after the first ask | 15 (+1 a 302 "not found", +1 still timing out) |
| 90 s later | 16 (the 302 stayed a 302) |

The Debian and Blender hashes asked earlier in the day behaved the same: a 10 s timeout on the
first ask, then 200 in about 40 ms a minute later.

**Consequence: a cheaper fallback (option B, chosen).** 042 already
accepts an add and tracks an item without a Put.io transfer (the reject path, `transfer_id =
NULL`). A magnet that misses could be held the same way: accepted, not sent to Put.io, and the
cache asked again every 30–60 s for a few minutes. On a hit it is checked as a .torrent. After
the last try it goes to Put.io as 042 does today. This needs:

- no new dependency, no DHT, no UDP, no change to what the NAS reveals (itorrents.net already
  sees the hash);
- a "held" state on the item and a small retry loop in the arr service (and the watch folder);
- a delay of a few minutes for magnet-only releases. The *arr sees the item as queued.

Caveats: one probe day, from one IP, on popular legal releases. itorrents.net is one volunteer
service. How it fetches (probably DHT plus its own peers) and how long it keeps entries is not
documented. Fakes are often new, so the first ask almost always misses, which is the case
re-asking helps most.

## R3. Can peers supply the file list in time?

Two probes, each from a cold start (fresh DHT node per hash, no saved state), DHT only (magnet
trackers not used), 14 s limit, 23 hashes:

| Probe | Found | Median | ≤ 4 s | ≤ 7 s | ≤ 14 s |
|---|---|---|---|---|---|
| librqbit 9.0.1 `list_only`, no listener, no trackers | 22/23 | 3.9 s | 13/23 | 17/23 | 22/23 |
| Hand-written: `mainline` 8.0.1 DHT + plain TCP BEP 9/10 | 7/23 | ~4 s | 3/23 | 6/23 | 7/23 |

Observations:

- DHT `get_peers` gives the first peers in 0.3–0.9 s from cold, and 60–800 peers for these
  torrents. About 1 cold start in 6 gave no peers at all within 14 s (bootstrap trouble).
- Most peers can't be reached (connect timeout or refused: NAT, firewalls), and 15–20% close a
  plaintext handshake (they require encryption).
- In the hand-written probe, peers on large swarms (mostly qBittorrent 5.x / libtorrent 2.x
  seeders) completed the extended handshake, sent bitfield and unchoke, then **closed the
  connection after the first metadata request**. Sending `interested`, the Fast extension with
  HaveNone, or one request at a time did not help. librqbit got the same torrents in 2–4 s, so
  the gap is in the hand-written peer protocol, not the network. The cause was not found.
- Warm (one DHT node kept running between lookups) made the first peer arrive sooner (0.1 s)
  but did not change which torrents were found.

**Consequence**: with a 4 s budget (R1 unchanged), a good client finds about 57%. With 7 s (cache
cut to 3 s), about 74%. A hand-written client would need real protocol work before it is as
good, and the cause is unknown.

Not measured: real fake releases (not tested on purpose), the NAS's network, and swarms of new
TV releases. Fakes are usually well seeded by the bots that spread them, which helps peer
lookup.

## R4. Crate choice

Requirements: pure Rust, small footprint, UDP from inside the container, metadata only (no
seeding, no pieces), no listening port, nothing persisted, testable on localhost.

New crates are counted against this workspace's current tree (`cargo tree -e normal`, 283
crates), default features unless noted:

| Option | New crates | DHT | Peer metadata (BEP 9/10) | Notes |
|---|---|---|---|---|
| `mainline` 8.0.1 (pubky), no default features | 18 | Yes, BEP 5, client (read-only, BEP 43) by default | No, write it ourselves | Actor on its own thread, async API. Adaptive mode can switch to server mode after running a while if not firewalled; a short-lived node never does. Brings ed25519/sha2 0.11 for BEP 44, which we don't use. Has a localhost `Testnet` for tests. MIT. |
| `librqbit-dht` + `librqbit-peer-protocol` 9.0.1 | ~30 | Yes, full node, no read-only flag | Message types only; connection logic is ours | Pieces of rqbit without its session logic, so the hard part (R3) stays ours. Apache-2.0. |
| `librqbit` 9.0.1, `default-features = false` | 93 | Yes (librqbit-dht) | Yes: `AddTorrentOptions { list_only: true }` | Whole torrent engine: storage, uTP, UPnP, persistence, trackers. All can be turned off (`listen: None`, `persistence: None`, DHT `persistence: None`, `disable_trackers: true`). `list_only` doesn't announce (`session.rs:1236`). Has `initial_peers` and DHT `bootstrap_addrs` for localhost tests. Proven in R3. Apache-2.0. |
| `bip_dht` 0.6.0 | — | Yes | No | Unmaintained since 2018. Rejected. |
| `cratetorrent` | — | No DHT | — | Unmaintained. Rejected. |

**Recommendation if peer lookup is built**: `librqbit` with default features off, one session
per lookup (so no long-lived DHT node, nothing on disk), `list_only`, no listener, no trackers,
no LSD, IPv4 only. It is the only option shown to work (R3). The cost is about 93 new crates,
roughly a third more than today, which Constitution VII says must be justified in the plan.

**Smaller alternative**: `mainline` (18 crates) plus our own BEP 9/10 code (~300 lines). Its
read-only DHT mode is the more private one. But R3 shows the hand-written peer side failing on
the swarms that matter, so this needs the cause found first. Plan it as a spike with a
go/no-go: it must reach librqbit's hit rate on R3's hash list.

## R5. Privacy

Turning peer lookup on reveals, for each magnet that misses the cache:

- **To the DHT nodes asked** (a few dozen, chosen by hash): the NAS's public IP, its DHT port and
  the info hash, in `get_peers` queries. Anyone can run nodes close to a hash (a Sybil node).
  Public DHT indexers already log `get_peers` traffic.
- **To every peer contacted** (up to the cap, FR-011): the NAS's IP and the info hash in the
  handshake. Copyright monitors run peers in popular swarms and log who connects. Research
  ([Purdue, PAM 2011](https://www.cs.purdue.edu/homes/fahmy/papers/PAM2011.pdf)) describes
  "direct" monitoring that contacts peers. A University of Washington crawler that only
  collected tracker peer lists (no data moved) still received DMCA notices
  ([summary](https://journals.law.unc.edu/ncjolt/?p=980)). A metadata-only connection moves no
  piece data and says it has none, but there is no guarantee a monitor won't send a notice.
- **Not revealed**: anything to trackers (FR-004), any listening port (FR-005), any announce
  that lists the NAS as a peer (FR-004).

This is the exposure 042 chose to avoid, and the main reason people use Put.io. Mitigations:

- Off by default (FR-001), with the note next to the switch (FR-014).
- Only after a cache miss (FR-002), only for checked categories, never for private magnets
  (FR-012), with a cap on peers per lookup (FR-011).
- A SOCKS5 proxy for peer connections (librqbit has `proxy_url`) would hide the IP from peers
  but not from DHT nodes, because the DHT is UDP. Left out for now.
- Option B (R2) reveals nothing new.

## R6. UDP from inside the container

- The DHT is UDP on a random local port. Outbound UDP works under Docker's default bridge
  network: replies come back through the NAT's connection tracking, so no port needs to be
  published.
- Behind a home router without port forwarding the node is "firewalled". `mainline` stays a
  read-only client then, and other nodes can't add it to their routing tables.
- No UPnP/NAT-PMP (FR-005). librqbit's UPnP runs only with a listener, and we configure none.
- The NAS container runs on Docker's `bridge` network (`docker inspect`, 2026-10-28). Host
  networking is not needed.
  If UDP is blocked, lookups fail fast as "unreachable" (edge case).
- Peer connections are plain TCP out. µTP (UDP) is optional for metadata. librqbit's µTP needs
  a listener, so it would be off.

## R7. Tests without the network (Constitution III)

- `mainline::Testnet` starts DHT nodes on localhost. Bootstrap addresses are configurable for
  both crates (`DhtBuilder::bootstrap`, librqbit `DhtSessionConfig::bootstrap_addrs`).
- A fake peer (a small TCP server in the test) serves a synthetic info dictionary over BEP 9/10:
  the right one, one with a wrong hash, one that stalls, one that claims a huge size. librqbit's
  `initial_peers` lets a test skip the DHT and point straight at the fake peer.
- The default-off test checks that no UDP socket is opened at all when the setting is off.
- No real torrent, no real hash, no network: everything listens on 127.0.0.1.

## R8. Is it worth building?

**Not as briefed, not yet.**

1. **Nobody knows how many real *arr magnets miss the cache.** 042's FR-022/023 counters will
   show it. Most indexers behind Prowlarr hand out .torrent files, so magnets may be rare.
2. **A cheaper fallback covers most of the gap.** Asking itorrents.net again after a few minutes
   turned 16 of 17 misses into hits (R2). It costs no dependency, no UDP and no privacy.
3. **Peer lookup within the add's time limit finds only about 57% (4 s) to 74% (7 s)** of what a
   good client finds in 14 s (R3), and the good client costs about 93 crates (R4).
4. **The privacy cost is the one 042 rejected** (R5), even with the setting off by default.

Suggested order:

1. Ship 042 and read its fallback counters for 2–4 weeks.
2. If magnets miss the cache often, build option B (hold and re-ask the cache). Also shorten the
   first cache ask from 10 s to 3–4 s, since misses time out anyway.
3. Build peer lookup (this spec, option C) only if misses after re-asking still cost real Put.io
   traffic. Use librqbit, opt-in, default off.

**Decision (2026-10-28)**: the user chose option B. The spec now covers holding and re-asking;
peer lookup is out of scope.

## R9. Design questions for holding

- **Hold time and interval.** R2's hits came about 1–3 minutes after the first ask. A 30 s
  re-ask over 5 minutes is up to 10 asks per magnet, each a cheap 404/timeout for the cache.
  Both are settings; 0 turns holding off.
- **First-ask limit.** Hits answered in 40–60 ms, the slowest under 2 s. 3 s keeps every hit
  seen and saves about 7 s on each miss.
- **Re-ask limit.** No *arr deadline applies to a re-ask, so it can use the full
  `lookup_timeout_secs` (10 s). A long stall still only costs a background task.
- **Where the item lives.** 042's reject path already stores an arr item with no Put.io transfer
  (`transfer_id NULL`). A held item is the same row in state `putio` with no transfer, so the
  existing state CHECK constraint doesn't change and the refresh loop, which only follows items
  with a transfer, leaves it alone. The magnet text and the schedule go in a new table, because
  `arr_items` and `release_checks` never store a magnet.
- **Sonarr and Radarr.** A Transmission item in status "queued" with no error isn't flagged
  by the *arr's failed-download handling. Its stalled check needs a downloading status with no
  progress, which a held item never reports.
- **Burst control.** One background task re-asks the due magnets one at a time, oldest first,
  so a 20-episode season search makes 20 sequential asks per interval at most.
