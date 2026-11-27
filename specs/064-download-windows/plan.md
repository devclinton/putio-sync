# Plan: Download windows and a storage budget (064)

## Where things go

- `putio-sync-core/src/download_window.rs`: `WindowEntry`, `Windows` (`open_at`,
  `next_open`, `state`), `space_paused` (hysteresis) and `validate`. Reuses the
  bandwidth schedule's day and time parsing.
- `putio-sync-core/src/forecast.rs`: `fill(samples, total, now)`, least squares over the
  last week.
- `putio-sync-core/src/config.rs`: `download.windows`, `pause_free_space_mb`,
  `resume_free_space_mb`; `storage.budget_enabled`, `budget_used_percent`.
- `putio-sync-download/src/gates.rs`: `Gates` holds the rules and the last answer
  (`Hold::Window` or `Hold::Space`). `rate_limiter.rs` gets a gate: while it holds, the
  limit is Paused, which is how running downloads and new claims already stop for a
  bandwidth pause. `DownloadManager::update_config` swaps the rules (hot reload).
- `putio-sync-download/src/waiting.rs`: codes `download_window` and `disk_pause`;
  `QueueState.rule`.
- `putio-sync-db`: table `usage_samples`, `StorageRepository::add_sample` / `samples`.
- `putio-sync-storage`: `plan::over_budget`; `budget_preview` (dry run, not kept),
  `budget_run` (plan, keep, `free_locked` with trigger label `budget`); a sample on each
  `refresh`; `forecast`.
- `putio-sync-daemon/src/schedule.rs`: `budget_run` after each 15-minute refresh.
- API: fields on `/settings/download` and `/settings/storage`;
  `POST /putio/storage/budget/preview`; `GET /stats/forecast`; summary fields.
- UI: `download_limits.js` (wording and the preview lock), a "Download hours and disk
  pause" card, the budget in the storage card, a Forecast card on Stats, two cases in
  `waiting.js`.

## Decisions

- The gate sits in the rate limiter, not in the claim loop, so a window closing pauses
  running downloads instead of letting a 50 GB file run into the morning.
- The budget reuses the free-up path whole. The only new selection code is how many
  bytes are needed; which files may go is untouched.
- A budget preview is not stored as a confirmable plan, so a preview can never be turned
  into a delete by `/free`.
- After a budget run that could delete nothing, no walk of the account for an hour.
- Forecast states are explicit, so the page never shows a date it can't stand behind.
