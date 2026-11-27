# Plan: Say why something is waiting (066)

## Approach

The reason is worked out in the daemon and sent as a code; the UI owns the words.

1. `putio-sync-download/src/waiting.rs`: `Waiting` (code plus optional fields),
   `bandwidth_hold` (a paused limit → reason) and `QueueState`, a snapshot of what
   `process_queue` checks. `QueueState::gate` and `for_row` apply the checks in the
   workers' order. Pure, so unit tests cover every rule.
2. `DownloadManager::queue_state` fills the snapshot: bandwidth status, the switch, free
   space and reserved bytes, workers, rows running, and what each failed file's failure
   was. The failure kind is recorded where a download fails (`note_failure`), in memory.
3. `GET /local/queue`: one snapshot per request, `waiting` on each row.
4. `GET /active`: `BuildInput.queue` carries the snapshot into the pure `build`;
   `PipelineItem.waiting` is set per item (Put.io queue, hold, waiting for sync, failed,
   paused, or what holds its pending files).
5. `ui/waiting.js`: code → `{text, tone, link, guide}`; `reasonLine` in `app.js` draws
   it. The Active and Queue rows show it under the name.

## Adding a reason (064 and later)

Add a constant in `waiting::code`, set it where the row is held back (a new check in
`QueueState`, or in `pipeline.rs` for an Active-only state), add a case in
`waitingLine`, and a row in the spec's table. Older UIs ignore the new code.

## Files

- `putio-sync-download/src/waiting.rs` (new), `manager.rs`, `pipeline.rs`, `lib.rs`
- `putio-sync-api/src/routes/local/queue.rs`, `routes/active.rs`, `src/ui.rs`
- `putio-sync-api/ui/waiting.js` (new), `ui/app.js`, `ui/app.css`,
  `ui/pages/active.js`, `ui/pages/queue.js`, `ui/i18n/*.json`
- Tests: `waiting.rs` and `pipeline.rs` unit tests,
  `putio-sync-api/tests/queue_actions.rs`, `putio-sync-api/tests/ui/waiting.test.mjs`

## Risks

- The snapshot is read a moment after the workers decide, so a line can lag by one poll.
- `free_bytes` changes often, so a held queue redraws on most polls; the page keeps focus.
