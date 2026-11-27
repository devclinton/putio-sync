# Research: Usage Profile in the Setup Wizard

## R1 Where the plan is computed

- Decision: in `putio-sync-core::profile`, as pure functions over `Answers`, detected folder
  names and a `Config`.
- Rationale: setup mode and Settings both need it; core holds shared config logic (I); pure
  functions are easy to test (III).
- Alternatives: in the UI (untestable in Rust, duplicated for Settings); in `putio-sync-api`
  (crate boundary).

## R2 Typed setting keys instead of JSON paths

- Decision: a `Key` enum (rule preset per folder, watch folder pre-check min/enabled,
  delete after download, empty-folder cleanup, subtitle languages/mode/excluded categories,
  watch enabled/paths) with `get(&Config) -> Value` and `set(&mut Config, Value)`, written
  as strings like `download.rules[Movies].preset`.
- Rationale: rules are an array keyed by path and pre-check watch entries a map keyed by
  path; a typed key makes each read/write exact and testable, and `Null` means "default"
  (the rule's preset removed, the override removed).
- Alternatives: generic JSON paths over the serialised config (fragile for arrays of rules).

## R3 Keeping changes made by hand

- Decision: `[profile] applied` maps each key string to the value the profile last set. On a
  re-run, a key whose current value differs from `applied` is kept and listed; keys no longer
  recommended whose value still equals `applied` go back to the default; others are left.
- Rationale: FR-019/SC-004 without tracking every edit elsewhere.

## R4 Reason text and i18n

- Decision: the daemon returns recommendation ids, reason codes and parameters; the UI holds
  the sentences.
- Rationale: 051 translates UI strings only; daemon messages stay English.

## R5 Setup link

- Decision: `http://host:port/setup#code=XXXX-XXXX-XXXX`; setup mode serves the UI at
  `/setup`; `setup.js` reads `code=` from the fragment, unlocks, and removes it with
  `history.replaceState`. The banner prints the link again when the code is replaced.
- Rationale: a fragment is never sent to the server, so it can't reach access logs or
  proxies.

## R6 Radarr/Sonarr

- Decision: Radarr and Sonarr only (clarified). The usage screen calls `/setup/arr/discover`;
  each ticked app with its key is checked with `/setup/arr/preview` on the review screen and
  set up with `/setup/arr/run` (remember on) at Start syncing, before `/setup/finish`. After
  start, the UI calls the normal `/arr/setup/check` for each saved application and offers
  `/arr/setup/mapping` and `/arr/setup/recommendation`.
- Rationale: reuses 018/049 unchanged; 049's folder check needs the running daemon.

## R7 Folder matching

- Decision: lower-cased top-level names: movies (movies, movie, films, film), TV (tv,
  tv shows, series, shows), music (music), ebooks and audiobooks (books, ebooks, e-books,
  audiobooks, audio books), software (software, apps, applications, programs, games). An
  *arr app ticked adds its category folder (`radarr`, `sonarr`). A kind with no folder gets
  one named Movies, TV, Music, Books or Software.
- A books folder whose name has "audio" gets the audio preset.

## R8 Pre-check sizes

- Decision: video 50 MB (the default, no override), music 5, books 1, software off. Only the
  watch folder is set: the *arr categories are video (Radarr/Sonarr), whose size is the
  default. A watch folder serving several kinds takes the smallest; software turns it off.

## R9 Subtitles

- Decision: with a video kind, `languages` = UI language then `en` when different (via
  `lang::lookup`), `mode = download_only`, `excluded_categories` = the music, books and
  software rule folders. Without a video kind, nothing.
