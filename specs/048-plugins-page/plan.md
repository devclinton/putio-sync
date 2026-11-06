# Implementation Plan: Plugins Settings Page

**Branch**: `048-plugins-page` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/048-plugins-page/spec.md`

## Summary

UI only. The plugins card moves from the Integrations section to a new Settings section,
`plugins`, and its code moves from `ui/settings/integrations.js` to `ui/settings/plugins.js`.
Inside the card:

1. enabled plugins (by their saved `enabled`) get a tab bar and one tab panel each, holding
   the old card body without the Enabled box and with a Disable button;
2. the rest are one "Available plugins" list: name, version, one line of description, and
   Enable (or the load error).

Enable opens the app's confirm dialog with the hosts the plugin will contact, then PUTs
`/settings/plugins` with only that entry changed. The save already approves the hosts of an
enabled plugin (`check_at`), so the daemon and API stay as they are. Disable does the same
with `enabled = false`. After either, the card re-reads `/settings/plugins` and `/plugins`
and draws again, keeping unsaved edits on other tabs; `form.js` gives the card a way to keep
its "unsaved" state right after such a save. Pure helpers (split, one-line summary, approval
text, toggled entry list) go in `ui/plugins.js` with node tests. Story 5 (added the same day): an action
answer's code gets Copy and, with a link, "Copy and open"; a code or link stays across `next`
steps (`carryFlow`); the Plex plugin links to plex.tv/link. Decisions are in
[research.md](research.md).

## Technical Context

**Language/Version**: vanilla ES modules (UI); Rust 1.96 for the doctor text and tests.

**Primary Dependencies**: none new.

**Storage**: none new; `[[plugins]]` in the config file as today.

**Testing**:
- `node --test` on `tests/ui/plugins.test.mjs` and `tests/ui/settings_sections.test.mjs`
  (run by `cargo test` through the existing UI test harness).
- `ui_assets` keeps the 416 KiB cap.
- Doctor tests that match the "Settings › …" text.

**Target Platform**: browser UI served by the daemon (NAS container).

**Project Type**: Cargo workspace with an embedded web UI.

**Performance Goals**: none beyond today's; the page makes the same two GETs plus one PUT and
two GETs per Enable or Disable.

**Constraints**: UI bundle about 401 KiB of 416 KiB; this feature adds at most 11 KiB (SC-005; measured 10,902 bytes, leaving about 4 KiB under the cap).
Reuse the confirm dialog, the 046 pill style and the existing card code.

**Scale/Scope**: up to a few dozen plugins.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | UI files plus one doctor string; no crate changes. ✅ |
| II. Local-first | Covered by `make ci` (node UI tests run inside `cargo test`). ✅ |
| III. Tests without Put.io | No Put.io calls. ✅ |
| IV. Download integrity | Not touched. ✅ |
| V. Observable by default | Not touched. ✅ |
| VI. Secrets stay secret | Enable and Disable send the saved entry as read, with secrets as the mask, so the stored secrets are kept and never shown. ✅ |
| VII. Simplicity | No new endpoint or setting; the existing save does the approval. One small hook in `form.js`. ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/048-plugins-page/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/plugins-page.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-api/ui/
├── settings/sections.js     # + plugins section, KEY_SECTION, old-name link
├── settings/integrations.js # - PLUGINS group and pluginsExtra
├── settings/plugins.js      # new: the Plugins card (moved code, tabs, list, enable/disable)
├── settings/form.js         # extra(data, {clean, rebase, onSaved})
├── pages/settings.js        # BUILD.plugins; a card address picks its section; open a tab on jump
├── pages/logs.js            # plugin link → #/settings/plugins
├── plugins.js               # + split, summary, approvalText, withEnabled
└── app.css                  # tab bar, list rows
putio-sync-api/tests/ui/     # plugins.test.mjs, settings_sections.test.mjs
putio-sync-doctor/src/checks/plugins.rs  # "Settings › Plugins"
putio-sync-plugins/bundled/plex/  # sign-in links to plex.tv/link (story 5); tests/mediaserver.rs
PLUGINS.md, README.md, EVENTS.md, config.example.toml, example-config.toml, CHANGELOG.md
```

**Structure Decision**: one new UI module for the moved card; everything else is edits.

## Complexity Tracking

No violations.
