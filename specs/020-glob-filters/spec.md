# Feature Specification: Glob Filters with AND and OR

**Feature Branch**: `020-glob-filters` (built on `claude/elated-shtern-cb1522`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "020 Glob filters with AND and OR. Simple glob patterns
(`*.mkv`, `Sample/*`, `*[Ss]ample*`) wherever files are chosen: folder rules, skip and
delete settings, and the Put.io page. Patterns combine with AND, OR and NOT (for example
`*.mkv AND NOT *sample*`), with a preview of which files match before saving. 045 built
this language for the Put.io, Transfers and Local files lists; what is left is using it in
folder rules and settings, with the same grammar on the server. Existing plain patterns
must keep working unchanged."

## Context

- 045 added a filter box to the Put.io, Transfers and Local files lists. Its language
  (`specs/045-select-sort-filter/contracts/filter-grammar.md`): spaces or `AND` join terms
  (all must match), `OR` / `|` give alternatives (tighter than AND), `-` or `NOT` negates a
  term, `*` and `?` are globs that must match the whole name, a term without them matches
  anywhere in the name, quotes keep spaces, case never matters, brackets are literal
  (`[YTS]`). It runs only in the browser.
- 009 folder rules choose files by extension only: a preset (allow or block list) plus
  "also allow" and "also block" extensions, a "skip folder" switch, and "delete blocked
  files", which removes files the type filter blocks from Put.io at each sync. The Put.io
  page shows the folder's rule card and "blocked" / "skipped" badges, and a rule edit can
  be previewed (counts plus the blocked and skipped files) before saving.
- `[download] whitelist_patterns` ("Only download") and `blacklist_patterns` ("Never
  download") are in the config, the Settings page and the examples, written as comma lists
  (`*.mkv,*.mp4`) or brace lists (`*.{mkv,mp4,avi}`), but no download path has ever
  applied them.
- Every place that decides whether a file is downloaded (sync, manual queue, dry run, the
  Radarr/Sonarr release check) asks the folder rules, so a change there applies everywhere.

## Decisions

- Q: Which language do saved patterns use? → A: 045's, extended in both places at once so
  the filter box and saved patterns stay one language: an unquoted comma separates
  alternatives like `OR` (so `*.mkv,*.mp4` keeps its meaning), `{a,b}` inside a pattern
  means "a or b" (so `*.{mkv,mp4}` keeps its meaning), and a pattern with `/` is matched
  against the file's path (below). One shared table of examples is checked against both
  the browser and the server matcher.
- Q: What does `Sample/*` match? → A: a pattern containing `/` is matched against the
  file's folder path plus name (relative to the Put.io root), and matches when it matches
  the whole path or any tail of it that starts at a folder boundary. So `Sample/*` matches
  `Movies/Film/Sample/a.mkv`; `Movies/*.nfo` matches `Movies/a.nfo` and
  `TV/Movies/a.nfo`. Patterns without `/` look at the name only, as in 045. Put.io names
  cannot contain `/`, so 045's lists behave as before.
- Q: Brackets: `*[Ss]ample*`? → A: brackets stay literal, as in 045, because release names
  often contain `[YTS]`-style tags and case never matters anyway (`*sample*` already
  matches `Sample`). A saved pattern that looks like a character class (`[Ss]`, `[0-9]`)
  gets a warning suggesting the plain form.
- Q: How do folder rules use patterns? → A: a rule gains two optional patterns: "Only
  download" (files must match) and "Never download" (matching files are refused). Like
  the rule's other settings, the deepest rule on the path that sets one wins; an empty
  value set on a rule clears the inherited one. The account-wide "Only download" /
  "Never download" settings are the root values. Patterns only narrow: they never let in
  a file the type filter refuses.
- Q: Order of checks? → A: skipped folder, then "Never download", then "Only download",
  then the type filter. The first that refuses names the reason ("matches never-download
  pattern `*sample*` · from Movies", "doesn't match only-download pattern `*.mkv` ·
  global").
- Q: Delete? → A: "delete blocked files" removes every file a rule refuses, by pattern or
  by type, as the preview shows. Because an "Only download" pattern refuses everything
  that doesn't match, a rule that deletes blocked files under an "Only download" pattern
  is flagged as a risk (Settings banner and doctor).
- Q: When is a pattern refused? → A: on save and at config load when the text cannot mean
  what was typed: an unclosed quote, unbalanced `{` `}`, text that holds only operators
  (`OR`, `NOT`), or longer than 1000 characters. Leftover operators (a trailing `OR`, an
  empty comma entry) are ignored as in 045 and only warned about. The list filter box
  stays as forgiving as 045 made it.
- Q: The existing Only / Never download settings were never applied. → A: they are applied
  from this release, with the meaning their help text always gave. The changelog and
  README say so, and the Settings preview shows what they do before anyone saves.
- Q: Preview? → A: the rule preview (Put.io page) and a new preview on the Settings
  Download group both show, before saving, the file counts, the refused files with the
  reason and whether they would be deleted, and a sample of the files that would be
  downloaded. Settings previews the whole account (the root).

## Clarifications

### Session 2026-11-03

This session was started by another session with no one to answer questions live, so each
question below was settled with the recommended option; the hand-over note lists them for
review.

- Q: Should `[Ss]`-style brackets become character classes, as the roadmap's `*[Ss]ample*`
  example suggests? → A: No. Brackets stay literal everywhere (045's behaviour, needed for
  `[YTS]` tags); case never matters, so `*sample*` covers it, and a bracket look-alike gets
  a warning.
- Q: Should the long-inert Only / Never download settings start applying, or be replaced
  by new keys? → A: Apply the existing keys; they are the account-wide root values. A new
  key would leave two settings with the same promise.
- Q: Are pattern refusals counted separately in the metrics? → A: No. They are "blocked"
  decisions in the existing rule-decision counter; the reason text (in Recently blocked
  and the logs) names the pattern. No new label, so cardinality is unchanged.
- Q: Which path does a `/` pattern see in a folder rule: relative to the rule's folder or
  to the Put.io root? → A: the Put.io root, with the any-tail match, so the same pattern
  means the same thing in a rule and in Settings.
- Q: Does a file's log line say which pattern refused it? → A: yes: the existing "blocked
  by rule" log line carries the reason text, which names the pattern.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Never download samples in a folder (Priority: P1)

On the Put.io page the user opens the Movies folder, edits its rule, types `*sample*` in
"Never download", previews (sees the sample files listed as refused) and saves. The next
sync skips them; the badges on the Put.io page mark them "blocked" with the reason.

**Why this priority**: the commonest reason to want patterns: junk that the extension
filter can't tell apart (a sample is still an `.mkv`).

**Independent Test**: a rule `{ path = "Movies", exclude = "*sample*" }` and files
`Film.mkv`, `Film.sample.mkv` under Movies: the preview and the sync download only
`Film.mkv`.

**Acceptance Scenarios**:

1. **Given** the rule, **When** a sync runs, **Then** `Film.sample.mkv` is not downloaded and
   the Recently blocked list shows it with "matches never-download pattern `*sample*`".
2. **Given** `Movies/Kids` has its own rule with "Never download" set to empty, **Then**
   samples under Movies/Kids download again.
3. **Given** the rule also deletes blocked files, **When** previewed, **Then** the sample
   files show "would be deleted" and the save asks before deleting.
4. **Given** the Put.io page shows Movies, **Then** each sample file has a "blocked" badge
   whose reason names the pattern and where it comes from.

---

### User Story 2 - Combine patterns with AND, OR and NOT (Priority: P1)

The user writes `*.mkv OR *.mp4 AND NOT *sample*` as a folder's "Only download", or
`Sample/* OR *.nfo` as its "Never download". The pattern means the same as it does in the
list filter box.

**Why this priority**: it is the feature's name; one pattern per setting is too weak.

**Independent Test**: the shared example table (pattern, names or paths, expected matches)
passes on both the browser and the server matcher.

**Acceptance Scenarios**:

1. **Given** "Only download" `*.mkv OR *.mp4 -sample`, **Then** `a.mkv` and `b.MP4`
   download and `a.sample.mkv` and `info.nfo` do not.
2. **Given** "Never download" `Sample/*`, **Then** `Movies/Film/Sample/clip.mkv` is refused
   and `Movies/Film/clip.mkv` is not.
3. **Given** `"the office" OR x264`, **Then** quoted spaces and OR behave as in 045.
4. **Given** the list filter box on the Put.io page, **Then** the same text shows the same
   files (comma, braces and paths included).

---

### User Story 3 - Account-wide Only and Never download (Priority: P1)

The user sets Settings › Download › "Only download" to `*.mkv,*.mp4,*.srt` and "Never
download" to `*sample* OR *.exe`, previews the effect over the whole account, and saves.
Every folder without its own pattern follows them.

**Why this priority**: the settings exist and promise this today but do nothing; that is
a bug users meet first.

**Independent Test**: with `whitelist_patterns = "*.{mkv,mp4}"` and files `a.mkv`,
`b.avi` at the root, a sync downloads only `a.mkv`.

**Acceptance Scenarios**:

1. **Given** an old config with `whitelist_patterns = "*.mkv,*.mp4"`, **Then** it loads
   unchanged and means `*.mkv OR *.mp4`.
2. **Given** `*.{mkv,mp4,avi,mov}` from the old example config, **Then** it means any of the
   four extensions.
3. **Given** the Download group, **When** the user presses Preview, **Then** it shows counts,
   refused files with reasons, and a sample of what downloads, without saving.
4. **Given** neither setting (the default), **Then** nothing changes for anyone.

---

### User Story 4 - Bad and risky patterns are caught (Priority: P2)

Typing `"the office` (no closing quote) in a rule or setting shows the error and the save
is refused. Risky setups are flagged in the Settings banner and the doctor's Config check:
a folder that deletes blocked files under an "Only download" pattern, a bracket class like
`*[Ss]ample*`, and leftover operators that are ignored.

**Why this priority**: patterns now decide what gets deleted from Put.io, so mistakes must
show before they cost anything.

**Independent Test**: save a rule with `"abc` → 400 naming the problem; a config with
`[[download.rules]] path = "Movies", include = "*.mkv", delete_blocked = true` → the doctor
Config check warns.

**Acceptance Scenarios**:

1. **Given** an unclosed quote, unbalanced brace, operator-only text or over 1000 characters,
   **Then** the save (rule, Settings, settings import) is refused with a message naming the
   setting and the problem, and a config file with it fails to load with the same message.
2. **Given** `*.mkv OR` or `*.mkv,,*.mp4`, **Then** it saves, works as `*.mkv` /
   `*.mkv OR *.mp4`, and a warning says the leftover is ignored.
3. **Given** `*[Ss]ample*`, **Then** a warning explains brackets are literal and suggests
   `*sample*`.
4. **Given** a rule that deletes blocked files with an "Only download" pattern in force,
   **Then** a risk warning says every file that doesn't match is deleted from Put.io.

### Edge Cases

- A file at the root: its path is just its name, so `Sample/*` can't match it.
- Names are matched as Put.io gives them, before characters are replaced for the local
  disk; folder paths are the ones rules already use.
- An empty or all-whitespace pattern is "not set".
- A rule that sets only patterns is a real rule (not dropped as empty).
- Patterns never unblock: `*.exe` in "Only download" still leaves `.exe` blocked by the
  default blocked extensions.
- A huge account: the Settings preview walks the whole tree; it says so and shows at most
  200 refused and 50 downloaded files.
- A pattern with `|` or `,` inside quotes stays literal.
- Setup mode: the preview and rule routes answer 503 like every other route.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The server MUST have a matcher for the filter language that gives the same
  answer as the browser's for every example in one shared table, run in both test suites.
- **FR-002**: The language MUST gain, in both places, unquoted comma as OR, `{a,b}`
  alternatives inside a pattern, and path matching for patterns containing `/` (whole path
  or any tail starting at a folder boundary). Everything else in 045's grammar is unchanged.
- **FR-003**: Folder rules MUST accept an optional "Only download" and "Never download"
  pattern; the deepest rule that sets one wins, an empty value clears the inherited one,
  and the account-wide settings are the root values.
- **FR-004**: The account-wide "Only download" and "Never download" settings MUST be applied
  wherever files are chosen (sync, manual queue, dry run, release check); existing comma
  and brace values MUST keep their meaning.
- **FR-005**: Checks MUST run in the order skipped folder, Never download, Only download,
  type filter, and the refusal MUST name the pattern and the rule (or global) it came from.
- **FR-006**: "Delete blocked files" MUST cover files refused by a pattern, and the preview
  MUST count them in "would be deleted".
- **FR-007**: Saving (rule routes, Settings, settings import) and loading the config MUST
  refuse a pattern with an unclosed quote, unbalanced braces, only operators, or over 1000
  characters, naming the setting and problem.
- **FR-008**: The Settings warnings banner and the doctor Config check MUST warn about
  ignored leftover operators, bracket class look-alikes, and delete-blocked rules under an
  Only download pattern.
- **FR-009**: The rule preview MUST accept candidate patterns and list, before saving, the
  counts, refused files with reasons and delete flag, and up to 50 files that would be
  downloaded.
- **FR-010**: The Settings Download group MUST offer a preview of the typed Only / Never
  download values over the whole account.
- **FR-011**: The Put.io page's rule card MUST show the patterns in force with their source,
  its editor MUST edit them, and the file badges MUST use the same matcher.
- **FR-012**: With no patterns set anywhere, every decision MUST be what it is today.
- **FR-013**: A file refused by a pattern MUST be counted as "blocked" in the existing
  rule-decision metric (no new label) and its log line and recorded decision MUST carry
  the reason naming the pattern.

### Key Entities

- **Pattern**: the text of one setting in the filter language; empty means not set.
- **Folder rule** (009): gains Only download and Never download patterns.
- **Effective rule**: gains the two patterns in force and where each came from.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every example in the shared table (at least 40) gives the same result in the
  browser and on the server.
- **SC-002**: A user can add "never download samples" to a folder, preview it and save it
  in under a minute from the Put.io page.
- **SC-003**: With no patterns configured, the existing rule tests pass unchanged.
- **SC-004**: Each refused pattern form in FR-007 is rejected on save with a message that
  names the setting.

## Assumptions

- Character classes, `**` and escaping are out of scope; brackets are literal.
- The CLI `list --pattern` option keeps its current glob behaviour.
- The Settings preview walks the whole account the same way the rule preview walks a
  folder; no extra limit on the walk is added.
- No new database data: decisions are already recorded with their reason text.
