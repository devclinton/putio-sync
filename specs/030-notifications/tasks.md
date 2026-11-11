# Tasks: Notifications

**Input**: `specs/030-notifications/` (plan.md, spec.md, research.md, data-model.md, contracts/)

**Tests**: requested by the spec (SC-001 to SC-004).

## Phase 1: Setup

- [X] T001 Compare SMTP crates and measure lettre's size in a scratch build; record it in research.md
- [X] T002 New crate putio-sync-notify (workspace member, Dockerfile in all four places) with lettre (`smtp-transport`, `builder`, `tokio1-rustls`, `ring`, `rustls-native-certs`)

## Phase 2: Foundational (events)

- [X] T003 `TransferStuck` and `DoctorProblem` payloads, `EventType::ALL`, samples and schema.json in putio-sync-core/src/events.rs
- [X] T004 Event hooks see the new types (putio-sync-telemetry/src/hooks.rs); EVENTS.md entries

## Phase 3: US2 stuck transfers and doctor problems (P1)

- [X] T005 [US2] putio-sync-core/src/stall.rs: one `transfer.stuck` per stall, with a test that moves and stalls again
- [X] T006 [US2] putio-sync-doctor/src/lib.rs: `doctor.problem` for checks that are a problem now and were not before, with a test
- [X] T007 [US2] Bundled notifiers: text and grouping words in notifier.lua, both events in the four plugin.toml defaults; putio-sync-plugins/tests/bundled.rs

## Phase 4: US1 email (P1)

- [X] T008 [US1] `[notifications.smtp]` (`SmtpConfig`): defaults, validation naming the field, mask and unmask in putio-sync-core/src/email.rs; config and notification wiring
- [X] T009 [US1] putio-sync-notify/src/smtp.rs: one send; STARTTLS required, implicit TLS, none without login; 30 s timeout; errors without the login
- [X] T010 [US1] putio-sync-notify/src/message.rs: event text and grouper with the notifier rules
- [X] T011 [US1] putio-sync-notify/src/lib.rs: `EmailService` (bus, queue, tick, status, configure, test); started by the daemon
- [X] T012 [US1] putio-sync-notify/tests/smtp.rs against a fake SMTP server
- [X] T013 [US1] API: `smtp` and `smtp_status` on `/settings/notifications`, live apply, `POST /settings/notifications/test`, import through the settings assistant
- [X] T014 [US1] Tests: settings (masked password kept, invalid input), secrets sweep (no password in responses or logs), settings assistant affects text

## Phase 5: US3 Notifications page (P2)

- [X] T015 [US3] putio-sync-api/ui/notify.js (form and notifier rows, no DOM) with tests/ui/notify.test.mjs
- [X] T016 [US3] putio-sync-api/ui/settings/notifications.js: Email card and Push notifications card
- [X] T017 [US3] New Notifications section in settings/sections.js, loaded by `import()` in pages/settings.js; old card removed from General; settings_sections test
- [X] T018 [US3] Register the new assets in src/ui.rs; `make i18n`; ui_assets size test passes
- [X] T019 [US3] Check the page in a browser against a local server: both cards load, save and test work, password stays masked

## Phase 6: Polish

- [X] T020 Release build size against the baseline, recorded in research.md
- [X] T021 README (API table), PLUGINS.md, config.example.toml, CHANGELOG, ROADMAP 030 done
- [X] T022 Full test run with `--no-fail-fast` and `bash scripts/ci-local.sh`
