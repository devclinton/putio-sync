# Research: Stream Mode with DLNA

## R1. Where the mode lives

- **Decision**: A rule setting, `mode`, on `[download]` and on each folder rule, resolved by
  `RuleSet` like `skip` or `delete_after_download`.
- **Rationale**: The spec wants account-wide and per-folder choice with inheritance and a
  reported deciding rule. Folder rules (009) already do exactly that, including the preview and
  the editor UI.
- **Alternatives**: A separate `[stream]` section with a folder list: a second inheritance
  model to explain and keep in sync with rules. Rejected.

## R2. Order against the other rule outcomes

- **Decision**: `skip` → exclude/include patterns → type filter → mode. `Decision::Stream`
  replaces `Decision::Download` only.
- **Rationale**: Skip must win (spec US2-4). Blocked files in a stream folder are junk the
  operator wants deleted from Put.io (`delete_blocked`), so blocking still applies first.

## R3. Who honours the mode

- **Decision**: Only the folder sync pass. The *arr download client and an explicit "Download
  this" from the Put.io page queue regardless (`honour_stream = false`), and the *arr case is
  logged once per file.
- **Rationale**: Radarr/Sonarr import local files only (FR-006). A click on "Download" is an
  explicit request that should not be silently ignored.

## R4. Tracking arrived files

- **Decision**: A `stream_files` table keyed by Put.io file id with `pending` (space-separated
  step names still to run), `attempts`, `last_error`. Inserted the first time a pass decides
  `Stream`. Rows whose file was not decided `Stream` in a full pass are pruned (deleted on
  Put.io, moved out, or switched to download).
- **Rationale**: `rule_decisions` has a `CHECK` on its decision values and is replaced on every
  decision; a table rebuild for one value is more risk than a new table. An in-memory set would
  re-run steps after every restart (SC-005).
- **Alternatives**: Asking Put.io each pass whether the MP4 exists: one call per file per pass.
  Rejected.

## R5. The seam for 023/024

- **Decision**: `trait ArrivedStep { fn name(); fn run(&ArrivedFile) -> BoxFuture<StepResult> }` (dyn-safe without async-trait), a
  `Vec<Arc<dyn ArrivedStep>>` on the download manager, built by the daemon. Each row's
  `pending` holds the steps it still needs. `StepResult::Done` removes the step,
  `StepResult::Retry(reason)` keeps it with `attempts + 1` and `last_error`.
- **Rationale**: 023 (TV) and 024 (movies) each need "do something once with a newly arrived
  file, retry if Put.io fails". They add a step; the pass does not change. Steps added later
  apply to files that arrive after they are added; organising an existing library is their own
  preview flow (roadmap 023).
- **Order**: steps run in list order per file. An organiser that moves a file keeps its Put.io
  id, so the MP4 version and the media server are unaffected.

## R6. MP4 requests

- **Decision**: `POST /files/{id}/mp4` (Put.io API v2) when `GET /files/{id}/mp4` says
  `NOT_AVAILABLE`. Statuses `COMPLETED`, `IN_QUEUE`, `CONVERTING`, `PREPARING` mean nothing to
  do. A 404 means the file is gone: done, logged. Other errors: retry next pass.
- **Which files**: video extensions from the media server's media table, minus `.mp4` / `.m4v`.
- **Bound**: 20 step runs per pass (FR-012), oldest first.

## R7. Default Put.io root of the media server

- **Decision**: `DlnaConfig::putio_root()` = `putio_folder.unwrap_or(0)`. Validation no longer
  requires `putio_folder` when `roots` has `putio`.
- **Rationale**: The sync mirrors the account root (`enqueue_tree(0)`), so the synced folder is
  0. Existing configs with a folder set are unchanged.

## R8. Settings warning

- **Decision**: Settings › Downloads reads the media server settings it already can
  (`GET /settings/media-server`). If stream is used (account mode or any rule) and the server
  is off or lacks the `putio` root, it shows a warning with one button that `PUT`s the media
  server settings with `enabled = true` and `putio` added to `roots`.
- **Rationale**: Reuses the existing route; no new endpoint.
