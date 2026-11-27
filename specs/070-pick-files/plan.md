# Plan: Pick files inside a transfer (070)

## Approach

A choice is data the sync reads; nothing is sent to Put.io.

1. `putio-sync-db`: tables `transfer_choices` (transfer id, root file id, `others`,
   `pending`) and `transfer_choice_files` (transfer id, path, wanted);
   `ChoicesRepository` and `Choice::wants(path)`.
2. `putio-sync-download/src/choice.rs`: pure matching. `locate` walks a file's parents to
   the root item of a transfer and gives its path under it; `Choices::wanted` answers
   yes, no, or "no choice here"; `torrent_paths` gives a `.torrent`'s files the same paths.
3. `DownloadManager`: the sync pass loads the choices, resolves each one's root file from
   Put.io's transfer list and asks `wanted` before the rules. `transfer_files`,
   `set_choice` and `clear_choice` serve the API. A file left out is recorded as a skipped
   decision ("Left out by your choice") so the Active page counts it.
4. `pipeline.rs`: `PipelineItem.chosen`, and `choose_files` for a pending choice, which
   also keeps a finished transfer with no queued files on the page.
5. API: `GET /putio/transfers/{id}/files`, `PUT` and `DELETE …/choice`,
   `POST /putio/transfers/preview`; `choice` and `choose_first` on add; `choice` on
   listed transfers; `transfer_id` on queue rows.
6. UI: `ui/pick.js` (tree, ticks, choice; no DOM) and `ui/pick_dialog.js` (the dialog).
   A "Choose files" button on Transfers, Active and Queue rows; "Choose files first" on
   the add form.

## Files

- `putio-sync-db/src/choices.rs` (new), `schema.rs`, `lib.rs`, `sources.rs`, `decisions.rs`
- `putio-sync-download/src/choice.rs` (new), `manager.rs`, `pipeline.rs`, `waiting.rs`
- `putio-sync-api/src/routes/putio/transfers.rs`, `models/transfers.rs`,
  `routes/local/queue.rs`, `src/ui.rs`, `examples/mock_ui.rs`
- `putio-sync-api/ui/pick.js`, `ui/pick_dialog.js` (new), `ui/waiting.js`, `ui/app.css`,
  `ui/pages/transfers.js`, `ui/pages/active.js`, `ui/pages/queue.js`, `ui/i18n/*.json`
- Tests: `choice.rs`, `choices.rs` and `pipeline.rs` unit tests,
  `putio-sync-download/tests/file_choice.rs`, `putio-sync-api/tests/transfer_choice.rs`,
  `putio-sync-api/tests/ui/pick.test.mjs`, `waiting.test.mjs`

## Risks

- A `.torrent` choice is matched by path, made before Put.io has the files. It assumes
  Put.io names the transfer's folder after the torrent and keeps the paths inside it. A
  path that does not match falls under `others`.
- The root file of a choice is learned from Put.io's transfer list. A held transfer that
  is cleared from that list before a sync sees its files loses its choice, and its files
  then follow the rules.
- Saving a choice starts a sync, so the queue changes a moment after the toast.
