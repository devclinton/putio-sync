# Implementation Plan: What's New

**Branch**: `017-whats-new` (worktree branch `claude/nostalgic-dhawan-736be1`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/017-whats-new/spec.md`

## Summary

Build `CHANGELOG.md` into the binary with `include_str!`, parse it once into release
sections, kinds and entries, and give each entry a stable id: an FNV-1a 64-bit hash of its
whitespace-normalised text. A new SQLite table `whats_new_seen` holds the ids this install
has seen. `GET /whats-new` returns the parsed changelog with a `new` flag per entry (and
records a baseline when the table is empty); `POST /whats-new/seen` records ids. The web UI
checks once per page load after sign-in and shows a modal "What's new" panel with up to 20
new entries; closing it posts their ids. A `#/news` page shows the whole changelog, and the
Status page's version links to it. The Docker build copies `CHANGELOG.md` into the build
stage.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser JavaScript (ES modules, no build)

**Primary Dependencies**: existing only (`axum`, `serde`, `sqlx`). The hash is a 10-line
FNV-1a, not a crate: `DefaultHasher` is not stable across Rust releases, and ids must stay
the same across builds.

**Storage**: SQLite, one new table `whats_new_seen` (see [data-model.md](data-model.md)).

**Testing**: `cargo test`: parser unit tests on fixture text and on the real built-in
changelog; a db repository test and the migration test; API integration tests in
`putio-sync-api/tests/whats_new.rs` (baseline, new after a seen row is removed, mark seen,
auth required, no database); the UI asset tests (module embedded, `node --check`, size cap).

**Target Platform**: the daemon on Linux (Docker on a NAS), macOS and Windows desktops.

**Project Type**: Cargo workspace with an embedded web UI.

**Performance Goals**: the changelog is parsed once per process (`OnceLock`); a request is
one `SELECT id FROM whats_new_seen` and a JSON body of about 30 KB.

**Constraints**: UI total under 300 KB (307,200 bytes); it is about 294 KB before this
feature, so the new page module, panel and CSS must stay within about 5 KB, leaving room
for 044, which is being built at the same time.

**Scale/Scope**: about 35 entries today, growing by a few per feature; the seen table grows
by one row per entry ever shown.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Parsing and the routes live in `putio-sync-api` (the only consumer, which also embeds the UI); the seen table and its repository live in `putio-sync-db`. No upward dependency. Pass. |
| II. Local-first verification | Covered by `make ci`; nothing CI-only. Pass. |
| III. Tests without Put.io | No Put.io calls; tests use a temp database and the built-in changelog. Pass. |
| IV. Download integrity | Not touched. Pass. |
| V. Observable by default | A failed seen read/write logs a `warn` with the error; no metric is needed for a UI notice. Pass. |
| VI. Secrets stay secret | The routes sit behind the API auth middleware; the changelog holds no secrets. Not served in setup mode. Pass. |
| VII. Simplicity | No new dependency, no config knob. One table, two routes, one UI module. Pass. |

Post-design re-check: unchanged, all pass.

## Project Structure

### Documentation (this feature)

```text
specs/017-whats-new/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/whats-new-api.md
└── tasks.md
```

### Source Code (repository root)

```text
CHANGELOG.md                         # the notes, embedded at build time
Dockerfile                           # COPY CHANGELOG.md into the build stage
putio-sync-db/src/schema.rs          # CREATE_WHATS_NEW_SEEN_TABLE
putio-sync-db/src/lib.rs             # create the table; db.whats_new()
putio-sync-db/src/whats_new.rs       # WhatsNewRepository: seen_ids, mark_seen
putio-sync-api/src/changelog.rs      # parse, entry ids, OnceLock of the built-in notes
putio-sync-api/src/routes/system/whats_new.rs  # GET /whats-new, POST /whats-new/seen
putio-sync-api/src/lib.rs            # nest the router behind auth
putio-sync-api/src/ui.rs             # embed pages/news.js
putio-sync-api/ui/pages/news.js      # the page, the panel, inline markdown
putio-sync-api/ui/app.js             # route "news", check once after sign-in
putio-sync-api/ui/pages/status.js    # version links to #/news
putio-sync-api/ui/app.css            # a few rules for the panel and "new" marks
putio-sync-api/tests/whats_new.rs    # API tests
```

**Structure Decision**: follow the existing split: storage in `putio-sync-db`, HTTP and the
embedded UI in `putio-sync-api`. The parser sits next to the route in the API crate because
the API is its only user and owns what is embedded in the binary for the UI.

## Complexity Tracking

None.
