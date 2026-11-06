# Data Model: Processing Pipeline

## pipeline_items

| Column | Type | Notes |
|--------|------|-------|
| `key` | TEXT PK | the 039 item key |
| `name` | TEXT | |
| `category` | TEXT NULL | |
| `source` | TEXT | `arr`, `watch` or `sync` |
| `dir` | TEXT | the item folder, or the file path for single-file items (the staging path while staging) |
| `stage` | TEXT | the current stage (see the order below) |
| `state` | TEXT | `running`, `held`, `waiting_for_operator`, `done`, `done_with_warnings`, `failed`, `cancelled` or `skipped` |
| `created_at`, `updated_at`, `finished_at` | TIMESTAMP | |

Stage order: `pre_download`, `downloading`, `pre_processing`, `subtitles`, `translating`,
`post_processing`, `notifications`.

## pipeline_steps

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `item_key` | TEXT FK | |
| `stage` | TEXT | |
| `branch` | TEXT NULL | for parallel branches, such as the language code (026) |
| `position` | INTEGER | the order inside the stage |
| `kind` | TEXT | `hook`, `download`, `notify` (later `subtitles` and `translate`) |
| `name` | TEXT | the hook name, or `download` / `notify` |
| `required` | BOOL | |
| `changes_files` | BOOL | |
| `state` | TEXT | `waiting`, `running`, `done`, `failed`, `skipped` or `interrupted` |
| `skip_reason` | TEXT NULL | `not_configured`, `category`, `operator` or `hook` |
| `next_attempt_at` | TIMESTAMP NULL | |
| `progress` | REAL NULL | 0..1 when the step reports it |

There is one `downloading` step per item. Its state mirrors 039's NAS stage.

## pipeline_attempts

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `step_id` | INTEGER FK | |
| `n` | INTEGER | 1-based |
| `started_at`, `ended_at` | TIMESTAMP | |
| `result` | TEXT | `done`, `failed`, `timeout`, `skip` or `interrupted` |
| `exit_code` | INTEGER NULL | for command hooks; the HTTP status for URL hooks |
| `output` | TEXT | the last 200 lines or 64 KB, redacted |

## Config additions

```toml
[pipeline]
concurrency = 2                 # items whose steps run at once, 1..=16
optional_retries = [60, 300, 600]  # seconds before each retry of an optional step, ≤ 10 entries
staging = "auto"                # "auto" (on when a file-changing step exists), "on", "off"

[[hooks]]
name = "tidy"
stage = "post_processing"       # new; absent = event hook (027)
required = false                # new; default true
categories = ["sonarr"]         # new; default all
command = ["/config/tidy.sh"]
timeout_secs = 300
```

Validation:
- `stage` set means `events` is empty.
- `stage` is one of `pre_download`, `pre_processing` or `post_processing`.
- `categories` entries are not empty.

## Item state transitions

```
(seen) → pre_download[running] → downloading → pre_processing → … → notifications → done
pre_download hook skip → skipped
required step failed → waiting_for_operator --retry ok / skip--> next stage
optional step failed N times → (continue) … → done_with_warnings
hold (039) → held (between steps) --release--> previous state
deleted / cancelled → cancelled (running step killed)
download failed (039) → failed (later stages: not reached)
```
