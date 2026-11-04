# Quickstart: Bandwidth Schedules

1. `make ci` passes (`TMPDIR=<scratch> make ci`).
2. Unit level: `cargo test -p putio-sync-core bandwidth` (parsing, windows past midnight,
   next change, warnings) and `cargo test -p putio-sync-download rate_limiter` (total
   across two consumers, live change, pause).
3. API level: `cargo test -p putio-sync-api --test bandwidth` (GET, PUT and DELETE
   override, `/status` section, refusal of a bad entry in `PUT /settings/download`, 503
   in setup mode).
4. By hand against a local daemon with a config containing the example in
   [contracts/config.md](contracts/config.md):
   - Status page shows the Bandwidth card with the limit, source, next change and daemon
     time.
   - Pause for 1 hour: the card says Paused until …; `curl /metrics | grep bandwidth`
     shows the limit gauge at 0 and the override gauge at 1.
   - Resume schedule: back to the schedule.
   - Settings › Download: add an entry with start 25:00, Save is refused naming entry and
     field.
   - Doctor: the Bandwidth check shows the limit and the time zone.
