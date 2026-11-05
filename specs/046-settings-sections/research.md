# Research: Settings Split into Sections

## R1 Keeping input when switching sections

- **Decision**: build every section's cards once when Settings opens and hide all but one.
  The router calls `page.update(params)` when the address changes within the same page and
  the page exports `update`; Settings shows the section and returns.
- **Rationale**: the hash router mounts a page on every hash change, which would throw away
  typed values. One small router hook keeps addresses real (Back, bookmarks) without that.
- **Alternatives**: buttons that don't change the address (no links, no Back); storing typed
  values between mounts (every card would need to save and restore its state).

## R2 The Download group shown as four cards

- **Decision**: a card may list only some of a group's fields (`part: true`). Its save body is
  the latest saved values of the group (kept per group key, refreshed by every card's load)
  with its own fields and extra parts on top. Defaults fills only its own fields.
- **Rationale**: the server saves a group as a whole; starting from the latest saved values
  means a save in one card neither undoes another card's save nor sends its unsaved edits.
- **Alternatives**: split the group on the server (API and config change for a page layout);
  keep one big Download card (the problem the user reported).

## R3 Unsaved-change marks

- **Decision**: a card takes a snapshot of its own values on the first pointer, key or focus
  event inside it, and on every input or change compares the current values with it. Defaults
  keeps the snapshot of the card it replaces.
- **Rationale**: some cards finish building after load (plugins, hooks status), so a snapshot
  taken at build time would differ from the first user-visible state; nothing can be edited
  before the operator touches the card.

## R4 Links from text

- **Decision**: `linkParts(text)` in `sections.js` splits text around known "Settings ›
  Section" and "Settings › Section › Card" names and gives each a link. The doctor's
  suggestions and the history page use it; the server's text is simply renamed. What's new
  notes may link `#/settings/...` and open in the same tab.
- **Rationale**: no API change; one table of names, tested in node.
- **Alternatives**: a link field in the doctor's JSON (API and CLI output change).

## R5 Field addresses

- **Decision**: `?field=<group>.<name>` maps to the input id `set-<group>-<name>` (config
  section `watch` maps to the UI group `watcher`); any other value is an element id (e.g.
  `arr-enabled`, `card-plugins`). The section comes from the address; for assistant warnings
  it comes from the key (`sectionOfKey`). A jump waits up to 5 s for the card to load, turns on
  Show advanced if the field is advanced, scrolls it to the middle, focuses it and highlights
  it for 2 s.

## R6 Navigation at phone width

- **Decision**: one list of links; CSS shows it as a sticky side list from 900 px and as a row
  of pills that scrolls sideways within itself below that.
- **Rationale**: pills keep the marks visible; a `<select>` would hide them.

## R7 Search

- **Decision**: at query time, scan the panels' `label.field` elements (label text, help text)
  and card titles; rank label prefix, then label, then help, then card title; show 10.
- **Rationale**: always matches what is on the page, including plugin and hook fields, with no
  index to keep in sync.

## R8 Advanced settings

- **Decision**: each card lists `advanced` field names; their fields get class `adv`. A section
  panel hides `.adv` unless its toggle is on; the toggle is stored per section in
  `localStorage` (`putio-sync.settings.advanced`).
