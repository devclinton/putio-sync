# Quickstart: validating the Active Transfers Pipeline View

Everything below runs locally, with no Put.io account (Principle III).

## Prerequisites

- The repository builds: `make ci` passes on the branch.
- A free local port for the mock UI. It picks one if none is given, and prints the URL and the
  token to sign in with.

## 1. Automated checks

```bash
make ci
```

These tests must be in the run:

| Area | What it proves | Spec |
|---|---|---|
| `download::pipeline` unit tests | Every row of research R5 and R6 maps to the right stage and state. Counts add up. Old finished transfers are not active. | FR-002–FR-011, SC-002, SC-004 |
| Stop/resume test (slow local HTTP body) | Pause stops a running file within 5 s, keeps the partial, and Resume continues from it with a range request. It is written first and fails on today's code. | FR-014, FR-015, SC-007 |
| Hold test (mockito Put.io) | Holding sends nothing to Put.io. When the transfer finishes, its files are queued `paused`. | FR-016, FR-017, SC-008 |
| Switch test | When the switch is on, nothing is claimed and new files from a sync stay queued. Turning it off resumes all except individually paused files. It survives a restart, which reopens the database. | FR-020, FR-020a |
| `api/tests/active.rs` | `GET /active` shape, per-key pause/resume results, 400 on empty keys, auth required, and the `/status` `active` section. | contracts/api.md |
| arr view test | A paused or held item reports status 0, not finished. | FR-021 |
| Telemetry contract test | The three new families exist, with no name or id labels. | FR-031 |

## 2. Look at it in the mock UI

```bash
cargo run -p putio-sync-api --example mock_ui 15432
```

Sign in with the printed token and open `http://127.0.0.1:15432/#/active`, then check:

1. **Counts**: both rows of counts show every state, including zeros, and "1 on hold".
2. **Rows**: one row per fixture transfer. Check the season-pack fixture: one row with "4 of 12
   files", and a "seeding" note.
3. **Stalled**: the stalled fixture shows the Stalled badge and is counted under Stalled.
4. **Waiting for sync**: the fixture shows the next sync time.
5. **Pause in the NAS stage**: Pause on a NAS-stage row makes it Paused, with speed "—" and time
   left "Paused". Resume reverses it.
6. **Pause in the Put.io stage**: Pause on a Put.io-stage row shows "On hold", and its Put.io
   progress keeps moving.
7. **Bulk**: tick three rows, then click Pause. A toast lists each result.
8. **Switch**: Pause NAS downloads shows the banner here, on Status and on Queue. On Queue, the
   old Pause all/Resume all are now that one switch.
9. **Status page**: the Active transfers card shows the counts sentence, up to 5 lines with
   stage badges, and a "See all" link.
10. **Links**: a Put.io-stage row opens Transfers. A NAS-stage row opens Queue filtered to its
    files.
11. **Phone width (375 px)**: no sideways scrolling, and the table turns into cards like the
    other pages.
12. **Failed rows**: the Put.io Error fixture stays in the list and in the Error count. Recently
    finished shows the failed fixtures that can no longer be retried, with their reason.
13. **Empty**: with every active fixture removed, the page says nothing is downloading and links
    to Transfers.
14. **Many rows**: restart the mock with `MOCK_ACTIVE_COUNT=200`. The page opens, and a name or
    count filter applies, in under 2 seconds (SC-005).
15. **Retry and skip on Queue**: Retry on a failed file puts it back in the queue. Skipping the
    last failed file of a transfer that has other files done moves it to Recently finished as
    done, with "1 file skipped".

## 3. Put.io unreachable

Start the mock with `MOCK_PUTIO_DOWN=1`. This switch, like `MOCK_ACTIVE_COUNT`, is new in this
feature: the mocked Put.io then answers the transfer list with a 503. The page then marks the
Put.io figures as out of date, with their age, and NAS figures keep updating.

## 4. Live NAS check after deploy (manual, optional)

Add a small magnet in the web UI. Then watch the row:
1. Put.io Queued, then Downloading;
2. NAS Waiting for sync, then Downloading;
3. it leaves the list and shows in Recently finished with stage times.

Pause it in the middle of the NAS stage, check that the speed drops to "—" within 5 seconds,
then Resume. Finally, check `/metrics` for `putio_sync_pipeline_items`.
