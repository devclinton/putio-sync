# Research: Stalled Put.io Transfer Detection and Re-add

Sources: Put.io OpenAPI 2.8.14 (`api.swaggerhub.com/apis/putio/putio/2.8.14`), Put.io's own Go
SDK (`putdotio/go-putio`, `types.go`), the existing client (`putio-sync-client`). No live
Put.io calls were made.

## R1. What Put.io exposes about an unfinished transfer

- **Decision**: Detect stalls and act through cancel + add. Do not attempt data recovery.
- **Rationale**: The transfer endpoints are list, get, add, retry, cancel, clean and remove.
  None returns files of an unfinished transfer. `file_id` is set only on completion;
  `save_parent_id` is the destination folder. The Go SDK's `Transfer` struct has
  `magneturi`, `torrent_link`, `source`, `type`, `status_message`, `tracker_message`, which our
  model lacks.
- **Alternatives considered**: downloading partial or completed files (no endpoint); par2 (needs
  the data locally); handing the magnet to a local torrent client (starts from 0% against the
  same swarm; left to the operator through Copy magnet).

## R2. Stall definition

- **Decision**: Stalled = status `DOWNLOADING` and `downloaded` bytes unchanged for at least the
  threshold (default 6 h), measured from the first observation of the current value.
- **Rationale**: `downloaded` is exact bytes; `percent_done` is a rounded integer and can stay
  flat for hours on a large torrent that is still moving. Status filters out queued, waiting,
  completing, seeding, completed and error transfers (error already has Retry).
- **Alternatives considered**: `down_speed == 0` alone (momentary, flaps); `estimated_time`
  (Put.io reports 0 or huge values unpredictably).

## R3. Reason shown

- **Decision**: first match of `availability < 100` → "availability N%", `peers_sending_to_us
  == 0` → "no peers", else "no progress".
- **Rationale**: availability below 100% means no one in the swarm has the missing pieces, the
  typical 97% case and the one re-adding is least likely to fix; it is the most useful thing to
  show first.

## R4. Where the tracker lives and who feeds it

- **Decision**: `StallTracker` in `putio-sync-core` (`stall.rs`), a `Mutex`-guarded map keyed
  by transfer id, fed by a daemon task polling `list_transfers` every check interval (default
  600 s). The API reads it through `AppState`.
- **Rationale**: Constitution I: shared types used by two crates (daemon writes, api reads)
  belong in `core`; scheduling belongs in `daemon`. `core` already depends on
  `putio-sync-client`, so the tracker takes `&[Transfer]`. Polling in the daemon makes
  detection independent of the UI (FR-001).
- **Alternatives considered**: updating the tracker from `GET /putio/transfers` only (no
  detection while the UI is closed); a database table (Constitution VII: history lost on
  restart costs at most one threshold, see spec Assumptions).
- **Clock**: `observe(transfers, now)` takes `now: SystemTime` so unit tests are
  deterministic without `tokio::time::pause`.

## R5. Server engine

- **Decision**: Only the daemon engine runs the check task. `AppState::new` starts with an
  empty tracker; `with_stall_tracker` swaps in the daemon's. Under the `server` command nothing
  is ever marked stalled.
- **Rationale**: The NAS runs the daemon (feature 004). The `server` engine is the smaller
  legacy path; adding a loop there is not needed now (VII).

## R6. Metric

- **Decision**: `putio_sync_transfers_stalled` (IntGauge, no labels), set after each check.
  Added to the basic families of the 003 contract.
- **Rationale**: Constitution V: bounded cardinality, no ids or names as labels.

## R7. Magnet link

- **Decision**: `magnet` = `magneturi` if non-empty, else `source` if it starts with
  `magnet:`, else none. Exposed in the API's transfer response; never logged.
- **Rationale**: Transfers added from an uploaded .torrent may have no magnet; `torrent_link` is
  a Put.io-relative path whose download semantics are undocumented, so it is not used.

## R8. Re-add order and failure handling

- **Decision**: `GET /transfers/{id}` (fresh magnet and folder) → refuse with 400 if no magnet →
  `POST /transfers/cancel` → `POST /transfers/add {url: magnet, save_parent_id}`. If the add
  fails, answer 500 (`ApiError::WithDetails`) with a message saying the old transfer was cancelled and the magnet in the
  error details.
- **Rationale**: Put.io is expected to refuse adding a torrent it already has, so cancel must
  come first. A concurrent second Re-add fails at the GET (404) before cancelling anything.
- **Alternatives considered**: add first then cancel (likely rejected as duplicate).

## R9. `current_ratio` as string

- **Decision**: a `deserialize_with` helper accepting number, numeric string, or null.
- **Rationale**: The Go SDK documents that Put.io returns either. A string today would fail the
  whole list.

## R10. New dependencies

None.
