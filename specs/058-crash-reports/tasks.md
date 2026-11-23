# Tasks: Crash reports to GitHub (058 part 1)

Paths are relative to the repo root.

- [x] T001 Write `specs/058-crash-reports/spec.md` and this file
- [x] T002 `putio-sync-core/src/crash.rs`: report type, `write_panic`, retention of 10, list, get,
  dismiss, redaction, the signature, the issue URL with a 6000-character cap, the running marker
  and the unclean-stop entry. Tests in the same file
- [x] T003 Panic hook (`crash::start`) chained to the previous hook, with the log ring read on a
  helper thread with a time limit
- [x] T004 `daemon` and `server` call `crash::start` with the install kind; the daemon ends the
  session before the restart after an update
- [x] T005 `GET /crash-reports`, `GET /crash-reports/{id}`, `POST /crash-reports/{id}/dismiss`
  in `putio-sync-api/src/routes/system/crash.rs`; tests in `putio-sync-api/tests/crash_reports.rs`
- [x] T006 `.github/ISSUE_TEMPLATE/crash.yml`
- [x] T007 Web UI notice in `putio-sync-api/ui/crash.js`, a slot in `index.html`, styles, the
  call after sign-in in `app.js`, strings in `i18n/en.json`; UI budget raised 512 to 520 KiB
- [x] T008 `putio-sync crash-report list|show|open` in
  `putio-sync-cli/src/commands/crash_report.rs`
- [x] T009 Doctor check `crashes` (undismissed crash in the last 7 days)
- [x] T010 Release profile `strip = "debuginfo"`; snap strips debug info only
- [x] T011 CHANGELOG, README "What leaves your machine", ROADMAP 058 part 1 marked done
