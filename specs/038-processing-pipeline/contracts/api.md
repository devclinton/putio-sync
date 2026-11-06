# Contract: API and UI additions (auth required)

## `/active` rows (039), extended

Each item gets `step`:

```json
{"stage": "post_processing", "name": "tidy", "state": "running", "progress": null}
```

It is `null` when the item is in the download stages (039 already shows those) or has no
pipeline row. A new filter value `processing` is added.

## `GET /active/{key}/timeline`

```json
{
  "key": "transfer:123",
  "state": "waiting_for_operator",
  "stages": [
    {"stage": "pre_download", "steps": []},
    {"stage": "downloading", "steps": [{"id": 1, "name": "download", "state": "done", "started_at": "...", "ended_at": "...", "attempts": 1}]},
    {"stage": "post_processing", "steps": [{"id": 7, "name": "tidy", "kind": "hook", "required": true, "state": "failed", "attempts": [{"n": 1, "result": "failed", "exit_code": 3, "started_at": "...", "ended_at": "...", "output": "…"}]}]},
    {"stage": "translating", "branches": {}, "steps": [], "skipped": "not_configured"}
  ]
}
```

Returns `404` for an unknown key. Works for items in Recently finished while they are kept.

## `POST /active/{key}/steps/{id}/retry`

Returns `202`. It is refused with `409` unless the step is `failed` or `interrupted`.

## `POST /active/{key}/steps/{id}/skip`

Returns `202`. It is refused with `409` unless the step is `failed`. It records
`skip_reason=operator`.

## Settings › Hooks (027), extended

The hook editor gets:
- **Runs as**: "Event hook" or a pipeline stage;
- **Required**;
- **Categories**.

The order inside a stage can be changed by dragging, or with up/down buttons. A new Settings ›
Pipeline group holds `concurrency`, `optional_retries` and `staging`, through
`GET`/`PUT /settings/pipeline`.

## UI

- The Active row shows "Post-processing: tidy" (and a spinner) while a step runs, and "Needs
  you: tidy failed" when it is waiting for the operator.
- Expanding a row shows the timeline. Stages run left to right on wide screens and stack on
  narrow ones.
  - Skipped stages are collapsed under "show skipped".
  - Branches are shown side by side.
  - Each step shows its duration and attempts; an attempt expands to its output.
  - Failed steps have Retry and Skip buttons; Skip asks for confirmation.
- Recently finished rows marked "with warnings" open the same timeline.

## Events (EVENTS.md)

| Type | Data |
|------|------|
| `pipeline.step_failed` | `key`, `name`, `stage`, `step`, `attempt`, `required`, `error` |
| `pipeline.completed` | `key`, `name`, `warnings: [step names]` |

Both go through the existing hooks and notifications. `pipeline.completed` with warnings is
the FR-021 notification.

## Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `putio_sync_pipeline_steps_total` | counter | `stage`, `kind`, `result` |
| `putio_sync_pipeline_step_seconds` | histogram | `stage`, `kind` |
| `putio_sync_pipeline_items_waiting` | gauge | `stage` |
