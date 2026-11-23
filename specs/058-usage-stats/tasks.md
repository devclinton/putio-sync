# Tasks: Anonymous usage stats (058 part 2)

Paths are relative to the repo root.

- [x] T001 Write `specs/058-usage-stats/spec.md` and this file
- [x] T002 `putio-sync-core/src/usage_stats.rs`: config, payload, buckets, install id state,
  schedule, guarded POST, `send_now`; tests in `putio-sync-core/tests/usage_stats.rs`
- [x] T003 `[usage_stats]` config section with validation in `putio-sync-core/src/config.rs`
- [x] T004 `StatsRepository::library_bytes` in `putio-sync-db/src/stats.rs`
- [x] T005 `GET/PUT /usage-stats`, `POST /usage-stats/reset-id` and the scheduler in
  `putio-sync-api/src/routes/system/usage_stats.rs`; tests in
  `putio-sync-api/tests/usage_stats.rs`
- [x] T006 Start the scheduler from `daemon` and `server`
- [x] T007 `putio-sync usage-stats show|enable|disable`
- [x] T008 Settings card in `putio-sync-api/ui/usage_stats.js` (lazy loaded), strings in
  `i18n/en.json`
- [x] T009 Cloudflare Worker in `packaging/usage-stats-worker/` and its node test
- [x] T010 README, `config.example.toml`, CHANGELOG, ROADMAP
