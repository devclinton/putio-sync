# Implementation Plan: Notifications

**Feature**: `specs/030-notifications` | **Date**: 2026-11-11 | **Spec**: [spec.md](spec.md)

## Summary

Add two events to the bus, `transfer.stuck` (from the stall detector) and `doctor.problem`
(from each doctor run, new problems only), so hooks and Lua and WebAssembly plugins get
them, and teach the bundled notifiers to send them by default. Add an email channel in a
new crate, `putio-sync-notify`, that reads the bus and sends chosen events over SMTP with
lettre and rustls, grouping repeats the way the notifier plugins do. Add a Settings ›
Notifications section, loaded only when opened, with the email form and the bundled
notifier plugins as rows.

## Technical Context

**Language/Version**: Rust 1.96 (workspace); plain ES modules for the UI
**Primary Dependencies**: lettre 0.11 (new; `smtp-transport`, `builder`, `tokio1-rustls`,
`ring`, `rustls-native-certs`), tokio, axum, serde
**Storage**: `[notifications.smtp]` in `config.toml`; the password masked like other secrets
**Testing**: cargo test (`putio-sync-notify/tests/smtp.rs` with a fake SMTP server,
`putio-sync-api/tests/{settings,secrets_sweep,settings_assistant,ui_assets}.rs`, core and
doctor unit tests, `putio-sync-plugins/tests/bundled.rs`), node tests for the UI
**Target Platform**: every release target, including armv7 and musl
**Constraints**: binary growth under 1.5 MB, no C toolchain, UI under 488 KiB
**Scale/Scope**: a few emails an hour at most; one mail server

## Constitution Check

- Simple, self-contained daemon: one small crate; email runs in the daemon, no new
  process. ✓
- Safe by default: email is off; STARTTLS never falls back to plain text; no login without
  TLS; the password is masked in the API, the export and logs. ✓
- Tests with the change: events, grouping, TLS refusal, masking and the page logic are
  covered. ✓
- Docs and CHANGELOG with every change. ✓

## Project Structure

```text
putio-sync-core/src/
  events.rs, events/schema.json  # TransferStuck, DoctorProblem
  email.rs                       # new: SmtpConfig, SmtpSecurity, validate, mask
  config.rs, notification.rs     # [notifications.smtp]
  stall.rs                       # emits transfer.stuck once per stall
putio-sync-doctor/src/lib.rs     # emits doctor.problem for new problems
putio-sync-notify/               # new crate
  src/lib.rs                     # EmailService: bus, grouping, queue, status, test
  src/message.rs                 # event text and the grouper
  src/smtp.rs                    # one send with lettre
  tests/smtp.rs                  # fake SMTP server
putio-sync-plugins/bundled/      # notifier.lua text, plugin.toml defaults
putio-sync-api/src/routes/system/settings.rs   # smtp, smtp_status, POST /notifications/test
putio-sync-api/ui/notify.js, settings/notifications.js, settings/sections.js, pages/settings.js
putio-sync-cli/src/commands/daemon.rs           # starts EmailService
```

## Phases

- Phase 0: [research.md](research.md): SMTP crate, size, TLS handling, event sources.
- Phase 1: [data-model.md](data-model.md), [contracts/](contracts/),
  [quickstart.md](quickstart.md).
- Phase 2: [tasks.md](tasks.md).

## Complexity Tracking

None. The Notifications page is loaded with a dynamic import so it does not count against
the UI budget, which had only a few bytes left.
