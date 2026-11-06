# Research: Processing Pipeline

## R1. When an item enters the pipeline

- **Decision**: `pipeline_tick` (every 10 s, 039) gives the pipeline service the current item
  list (`Pipeline` from `putio-sync-download`).
  - An item key the store has not seen gets a `pipeline_items` row and its plan of steps.
  - If it has pre-download hooks, and its files have not started, a hold is placed through
    039's hold mechanism (`pipeline_holds`, reason `pre-download`). The hold is released when
    the hooks pass.
  - An item whose NAS stage is done (every file done) is moved to Pre-processing.
- **Rationale**: This reuses the loop and the item unit that already exist, and adds no new
  trigger in the download worker. A 10 s tick is fine for steps that take seconds to minutes.
  SC-001 holds because an item with no file-changing steps is never gated in the *arr view, so
  the arr sees completion at once, as today.
- **Alternative**: a callback from the worker for each file. Rejected: items are multi-file,
  and only the tick knows when the last file of an item is done.

## R2. Item identity

- **Decision**: Use 039's item keys: `transfer:<id>`, `folder:<putio id>` and `file:<putio
  id>`. Plain-sync items without a transfer get a pipeline too (FR-006). They are recorded in
  `pipeline_items` rather than in `pipeline_finished` (039 records transfers only), so their
  timeline survives.

## R3. Pipeline hooks vs event hooks

- **Decision**: Hooks gain an optional `stage` (`pre_download`, `pre_processing` or
  `post_processing`).
  - Without `stage` a hook is an event hook (027): unchanged, and fired by `HooksService`.
  - With `stage`, `events` must be empty, and the hook runs as a pipeline step. It has
    `required` (default `true`) and `categories` (default all).
  - The order is the config order within a stage.
  - The runner reuses `command::run` and `webhook::run` with a new payload type, so the
    timeouts, env, header masking and output capture are shared.
- **Input**:
  - Env: `PUTIO_SYNC_ITEM_KEY`, `_NAME`, `_CATEGORY`, `_SOURCE`, `_DIR`, `_STAGE` and
    `PUTIO_SYNC_ITEM_JSON` (the path of a temp JSON file).
  - URL hooks get the same JSON as the body.
  - The JSON: `{key, name, category, source, dir, files:[{relative_path, size, putio_id}],
    transfer_id}`.
- **Outcome**:
  - exit 0 or 2xx → done;
  - exit 10, or 204 with `{"action":"skip"}` → skip the item (pre-download only; anywhere
    else it counts as done);
  - anything else, or a timeout → failed.

## R4. Holding back the *arr (Transmission mode)

- **Decision**:
  - `core` gets a new trait, `PipelineGate::pending(key) -> Option<String>`. It returns the
    current step label while a file-changing step is still due.
  - `arr::view::torrent` takes the gate. When an item is done locally and the gate returns a
    label, the view reports:
    - `leftUntilDone = max(1, total / 100)`, so 99%;
    - `status` downloading;
    - `errorString` empty.
  - The item's `ArrState` stays `Local` until the gate clears, then becomes `Done`, as today.
- **Rationale**: Showing under 100% is enough to stop the import.
- **Deviation from the spec**: the spec (FR-014) asks for the step as the *arr's status
  message. Transmission has no message field for anything but errors, and a non-empty
  `errorString` makes the *arr treat the item as failed. So the label shows on the Active page
  only, and the *arr shows "Downloading 99%". The spec should be updated to match.

## R5. Watch-folder staging

- **Decision**:
  - When `[pipeline] staging = true` (the default when any file-changing step is configured)
    and the item came from a watch folder, its files download into
    `<category dir>/.putio-sync-staging/<item name>/`.
  - After the last file-changing step: `rename(staging/<item>, <category dir>/<item>)`. If
    the target exists, `<item> (2)`.
  - At startup and on config save, `staging` checks that the staging dir and the category dir
    have the same `st_dev`. If not, the save is refused and the daemon logs an error and turns
    staging off.
  - The hidden prefix keeps Radarr/Sonarr scans and 044's Local files listing quiet. 044 shows
    it as a folder; that is acceptable.
- **Rationale**: One rename means the item is never half visible (SC-002).

## R6. Retry and give-up

- **Decision**:
  - A required step that fails puts the item in `waiting_for_operator`. Retry runs a new
    attempt. Skip records `skipped(operator)` and moves on.
  - An optional step that fails is retried after 1, 5 and 10 minutes
    (`[pipeline] optional_retries = [60, 300, 600]`). After the last failure it becomes
    `failed` (not retried), the item goes on, and at the end the item is
    `done_with_warnings`. A `pipeline.warning` event is sent through the normal event and
    notification path.
  - Retry from the UI is allowed on any failed step, including an optional one that was given
    up. After that, the remaining steps run.

## R7. Restart

- **Decision**: At startup, steps in `running` become `interrupted`, and a new attempt is
  queued for them. Done steps are never run again. Hooks should be idempotent, and EVENTS.md
  says so.

## R8. Concurrency

- **Decision**: A tokio semaphore with `[pipeline] concurrency` permits (default 2), taken
  per item stage run. Branches inside one stage share the item's permit pool, up to the
  same limit.

## R9. History

- **Decision**: "History" in the spec is 039's Recently finished. Finished items keep their
  `pipeline_items`, steps and attempts until 039's retention prunes the item (the same
  `finished_retention_hours`). The timeline route reads them, and the Recently finished rows
  link to it. The download history (per file) is unchanged.
