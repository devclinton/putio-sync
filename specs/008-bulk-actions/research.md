# Research: Multi-select and Bulk Actions

Sources: Put.io's own TypeScript SDK (`putdotio/putio-sdk-typescript`,
`src/domains/transfers.ts`), the existing client (`putio-sync-client`). No live Put.io calls
were made.

## R1. Which Put.io calls take several ids

- **Decision**: files → `POST /files/delete {file_ids: "1,2,3"}` (already in the client);
  transfers remove → `POST /transfers/cancel {transfer_ids}` (already in the client);
  clear by status → `POST /transfers/clean {transfer_ids}` (new client method); retry →
  `POST /transfers/retry {id}` once per id (Put.io has no multi-id retry).
- **Rationale**: `/transfers/clean` with `transfer_ids` cleans exactly those finished transfers
  and answers `{"deleted_ids": [...]}`, so the server can tell which ones Put.io removed.
  Without ids it cleans everything finished; we always pass ids so the server decides which
  states are cleared, not Put.io.
- **Alternatives considered**: `POST /transfers/remove` takes `transfer_ids` or
  `remove_filter` (`all` | `completed`). It returns only `OK`, so we can't report what went,
  and there is no `failed` filter. Not used.

## R2. Bulk remove uses cancel

- **Decision**: bulk remove sends one `/transfers/cancel` with every id.
- **Rationale**: the single Remove already uses cancel (feature 004); cancel works on a
  transfer in any state, so mixed selections behave the same as clicking each row.

## R3. Clear falls back to cancel

- **Decision**: clear = list → filter by status group → clean(ids) → cancel(ids clean left).
- **Rationale**: Put.io doesn't document which states clean accepts. If it leaves, say, error
  transfers in place, one cancel removes them, so the button does what it says. At most two
  calls after the listing. If `deleted_ids` is missing from the answer, every id sent counts as
  cleaned.
- **Alternatives considered**: cancel only (works but ignores the purpose-built call); clean
  without ids (Put.io would decide what "finished" means, and could clear seeding or error
  transfers the operator meant to keep).

## R4. Status groups

- **Decision**: `completed` = COMPLETED + SEEDING; `failed` = ERROR + FAILED; `finished` =
  both. Matching is case-insensitive.
- **Rationale**: a seeding transfer's files are already in the account; the UI already badges
  COMPLETED and SEEDING alike, and ERROR and FAILED alike (`transfers.js`).

## R5. Dry run for clear

- **Decision**: `POST /putio/transfers/clear` requires `dry_run`, like the empty-folder cleanup
  (feature 005). The UI previews, confirms with the count, then runs.
- **Rationale**: Constitution IV (destructive operations support a dry run), and the server, not
  the browser, chooses the ids.
- **Bulk delete and bulk remove take explicit ids** chosen by the operator and confirmed with
  the count, like their single-row versions, so they have no dry run.

## R6. Delete guard for several files

- **Decision**: check every id against the queue first; if any is pending, downloading or
  paused, answer 409 for the whole request (nothing deleted) with the protected names in
  `details`.
- **Rationale**: same rule and status as the single delete (feature 004, R11); deleting "the
  rest" silently would make the outcome depend on a race the operator can't see. The UI
  disables those checkboxes, so a 409 only happens when the queue changed after the page loaded.

## R7. Bulk Download

- **Decision**: the browser calls `POST /local/queue {file_id}` per selected item, sequentially,
  and sums the counts.
- **Rationale**: queuing walks each folder on Put.io anyway; a batch endpoint would save
  browser round trips, not Put.io calls (Constitution VII).

## R8. Request shape

- **Decision**: reuse the existing request models: `{"file_ids": [...]}` and
  `{"transfer_ids": [...]}`; ids are deduplicated; an empty list is 400.
- **Routes**: `POST /putio/files/delete`, `POST /putio/transfers/remove`,
  `POST /putio/transfers/retry`, `POST /putio/transfers/clear`. Static segments coexist with
  `/:id` in axum 0.7.
