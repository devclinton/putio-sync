# Feature Specification: Crash reports to GitHub (part 1 of 058)

**Feature Branch**: `058-crash-reports`

**Created**: 2026-11-22

**Status**: Implemented

**Input**: ROADMAP.md, "058 Crash reports, then anonymous usage stats", part 1. Part 2 (anonymous
usage stats) is a later spec.

## Context

- Nothing in putio-sync calls home. The daemon can't open GitHub issues itself: a token in the
  binary would let anyone file issues as the project. So the person files the report, from their
  own GitHub account and their own browser.
- Logs are already masked by `log_redact` and kept in an in-memory ring (047). The update
  checker (055) detects how putio-sync was installed (`Channel::kind()`).
- The web UI polls nothing at start and shows notices after sign-in (What's new, 017; updates,
  055).

## User Scenarios

### US1 - Report a crash (P1)

After a panic, the next time the person opens the web UI it says "putio-sync stopped
unexpectedly" and shows the whole report. They check it for names and paths, then choose
**Report on GitHub** (a prefilled new issue opens in a new tab), **Copy report**, or
**Dismiss**.

1. **Given** a panic wrote a crash file, **When** the UI loads, **Then** the notice shows the full
   report as text, with **Search existing issues** first.
2. **Given** the report is too long for a link, **When** the person opens it, **Then** the issue is
   prefilled with the start of the report and asks them to paste the copied one.
3. **Given** a report was dismissed, **When** the UI loads again, **Then** no notice shows.

### US2 - Notice a run that stopped without a report (P2)

An out-of-memory kill, `SIGKILL` or container restart runs no panic hook. At the next start the
leftover "running" marker, with no newer crash file, becomes a "stopped without a report" entry.

### US3 - CLI and doctor (P2)

`putio-sync crash-report list|show ID|open ID` works without the daemon. The doctor warns about an
undismissed crash from the last 7 days.

## Requirements

- **FR-001** A panic hook writes `crash-reports/crash-<id>.json` next to the config file: version,
  install kind, OS, arch, uptime, thread name, message, location, a forced backtrace and the last
  50 log lines. Every text goes through `log_redact`, and the home folder becomes `~`.
- **FR-002** At most 10 crash files are kept; the oldest are deleted.
- **FR-003** The hook never panics and never logs. It chains the previous hook. Reading the log
  ring is done on a helper thread with a 1 s limit, in case the panic happened inside the ring's lock.
- **FR-004** `daemon` and `server` write a `running` marker at start and remove it on a clean stop
  (including the restart after "Update now"). A marker left with no crash file newer than its start
  time becomes a `kind: "unclean"` entry.
- **FR-004a** Each report records `stopped`: true for a panic on the main thread, for a stop
  without a report, and for a report newer than a marker that was left behind at the next start;
  false for a panic the process survived. The UI says "putio-sync stopped unexpectedly" for the
  first and "putio-sync hit an internal error" for the second, and the issue title starts with
  "Crash:" or "Panic:". A rollback after a failed update trial clears the marker.
- **FR-005** `GET /crash-reports` (list and `pending`), `GET /crash-reports/{id}` (full report,
  `text`, `url`, `search_url`, `truncated`), `POST /crash-reports/{id}/dismiss`. All need the same
  authentication as the other routes. Ids are checked, so no path can escape the folder.
- **FR-006** The issue URL is `https://github.com/devclinton/putio-sync/issues/new` with
  `template=crash.yml`, `labels=crash`, `title=Crash: <signature>` and `report=<text>`. The
  signature is the first three non-plumbing frames, shortened. The whole URL is at most 6000
  characters: the report is cut from the end, a note is added, and `truncated` is true.
- **FR-007** `.github/ISSUE_TEMPLATE/crash.yml` is an issue form with the `crash` label and a
  `report` textarea.
- **FR-008** The UI shows the report with `textContent` in a `<pre>`, never `innerHTML`. Only
  `https://github.com/` links are used as `href`, with `target="_blank" rel="noopener"`.
- **FR-009** Strings are in `ui/i18n/en.json`; the UI stays under its size budget.
- **FR-010** Release builds keep symbol names (`strip = "debuginfo"`; the snap strips debug info only).

## Out of scope

- Sending anything from the daemon. Anonymous usage stats (part 2).
- Crash files for panics in plugins' Lua or WASM sandboxes beyond what the panic hook sees.
