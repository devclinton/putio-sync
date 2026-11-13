# 061 Command palette: design

## Intent

A quick-search box over the web UI, like the command palette in VS Code and other apps. One
shortcut from any page finds a place, runs an action, or finds a file or queue item, without
clicking through the navigation. The user asked for it because it is a useful, familiar
pattern; there is no single pain point behind it, so it should feel like the palettes people
already know rather than something new to learn.

## Goals

- Open from any signed-in page with one shortcut; close with Esc.
- One search box across six kinds of result: actions, pages, settings, queue items, put.io
  files and local files.
- Keyboard first: type, arrow keys, Enter. The mouse works too.
- Built-in results appear as you type; file results follow shortly after.

## Non-goals (this version)

- Running queue actions (pause, retry, skip) on an item from inside the palette. Picking an
  item takes you to it; acting on it happens on its page. A later step may add this.
- Searching file contents, history, logs or transfers.
- User-defined commands or shortcuts, or a recent-items list.

## Anti-goals

- The palette must never make the UI slow or stall the daemon: a big NAS folder tree or a
  slow put.io must not freeze typing or tie up the server.
- An action that changes things must not run on one stray Enter without the same
  confirmation its button asks for today.
- It must not break the browser's own shortcuts beyond the one it takes.

## Decided with the user

- Scope: places, settings, actions, queue items, put.io files and local files.
- Both put.io files and local synced files are searched.
- Out of scope: queue item actions inside the palette (see Non-goals).

## Claude's calls (accepted in the playback, adjustable)

- **Shortcut:** Cmd+K on macOS, Ctrl+K elsewhere. Cmd+P is left to the browser (print).
  It opens from inside text fields too, as other apps' palettes do.
- **Group order:** Actions, Pages, Settings, Queue, Put.io files, Local files. Empty groups are
  hidden.
- **Matching:** loose (fuzzy) match on names for the built-in groups, so "subt" finds
  Subtitles. The queue is filtered on the client by name.
- **Timing:** actions, pages and settings are in the page already. The queue is fetched once
  when the palette opens. File searches start after 2 or more characters and a short pause
  (about 250 ms), and a newer query cancels or ignores an older one.
- **Confirmation:** actions that need it go through the existing `confirm()` dialog in
  `app.js`, with the same text their buttons use.
- **Local search limits:** stop at about 50 hits or 2 seconds, whichever comes first, and say
  in the results when it stopped early.
- **Local search source:** walk the sync roots on disk (what the Files page shows), not the
  download history in the database, so files the user put there themselves are found.

## Settled after the builder check (Claude's calls)

- **Settings catalog:** today's Settings search scans the DOM of loaded panels, so it only
  works on that page. Build a static catalog (section, card, field key, label) and move
  Settings search onto it, so the palette finds settings from any page. Jumps go by the
  existing `?field=` key.
- **Downloads pause:** "Pause downloads" and "Resume downloads" are two separate entries.
  They map to the local downloads pause (`setNasPaused`, `POST /local/queue/pause` and
  `/resume`), not the bandwidth pause. The palette needs no state when it opens.
- **Confirmations:** only "Clear completed" confirms, with a generic "Remove completed
  items?" text. Sync now, pause, resume, toggle theme, check for updates and sign out run
  without asking, as their buttons do now. "Check for updates" goes to the Update page and
  starts a check there. "Sign out" is hidden when sign-in is Tailscale-only.
- **Queue:** the source is `GET /local/queue`. Matching is on `filename`. Picking an item
  opens the Queue page's existing `?file=<file_id>` filtered view. A highlighted row in the
  full list is not needed.
- **Local search across roots:** one budget (50 hits or 2 s) shared across all roots. Each
  result shows its root's name. Symlinks are not followed, and hidden files and dotfiles are
  skipped.
- **Put.io search:** fetch only put.io's first results page, capped at 50, skipping trash.
- **When the shortcut works:** it does nothing on the login, setup and sign-in pages, and
  while the confirm dialog is open. Otherwise it calls `preventDefault` on Cmd/Ctrl+K.

## How results behave

| Group | Source | Picking one |
|---|---|---|
| Actions | built-in list | runs it (confirm first if it changes things) |
| Pages | the router's pages | `navigate("#/<page>")` |
| Settings | the index Settings search already builds (`pages/settings.js`) | opens that settings card |
| Queue | `GET /local/queue`, filtered by `filename` | Queue page filtered to that item (`?file=<file_id>`) |
| Put.io files | new `GET /putio/files/search?q=` | Put.io page in the file's folder, file highlighted |
| Local files | new `GET /local/files/search?q=` | Files page in the file's folder (`?path=` exists), file highlighted |

Starting action list (the builder may add obvious ones): sync now, pause downloads, resume downloads,
clear completed, check for updates, toggle theme, sign out. Each must map to something the UI
can already do.

## Server work

- `GET /putio/files/search?q=`: forwards to put.io's own file search through
  `putio-sync-client`, which does not wrap that endpoint yet. Returns enough to name the file
  and open its parent folder (id, name, parent id, type, size).
- `GET /local/files/search?q=`: case-insensitive name match under each configured sync root,
  bounded as above, run off the async runtime so it cannot block other requests. Returns root
  key, relative path, name, type, size, and whether the walk was cut short. Must respect the
  same root resolution and path-escape checks as the existing `/local/files` routes.
- Both need the same auth as the other API routes, and API tests in `putio-sync-api/tests`.

## Constraints

- Plain JS modules in `putio-sync-api/ui`, matching the existing style (`h()`, `navigate()`,
  `confirm()`, `toast()` from `app.js`). No framework or new dependency.
- All UI text goes through the 051 translation catalogs keyed by English text.
- Works at phone width and in both themes; the dialog is accessible (focus trap, roles,
  `aria-activedescendant` or equivalent, Esc restores focus).
- Settings search logic is shared with the palette rather than copied.
- Feature number 061; add a ROADMAP.md entry and a CHANGELOG.md line under [Unreleased].
- Highlighting a target row on the Queue, Put.io and Files pages may need small additions to
  those pages (a query parameter they read).

## Left to the builder

- The exact fuzzy-match scoring and how many results each group shows.
- Dialog layout and styling within the existing `app.css`.
- How the put.io page is told which folder and file to show, if it has no parameter today.
- Whether the local walk uses an existing helper in `putio-sync-file-manager` or a new one.
- Test split between Rust API tests and the UI tests in `putio-sync-api/tests/ui`.
