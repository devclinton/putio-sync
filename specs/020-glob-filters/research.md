# Research: Glob Filters with AND and OR

## R1 Matcher: hand-written, no `regex`

- Decision: tokenizer + parser mirroring `ui/query.js`, and a backtracking glob matcher
  over `char`s (`*` any run, `?` one character, literal otherwise), case-insensitive by
  lowercasing both sides. Braces are expanded at compile time into alternatives (no
  nesting; capped at 64 per term).
- Rationale: the language is tiny; `regex` would be a new dependency only to mirror the
  JS implementation. Compiling once per folder keeps decisions cheap.
- Alternatives: `regex` (new dep); `glob::Pattern` (case-sensitive, has character classes,
  no substring mode, so it can't match the JS).

## R2 Parity with the browser

- Decision: one JSON table, `putio-sync-core/tests/data/filter-cases.json`, of
  `{ filter, names | paths, shown }` cases, read by a core integration test and by
  `putio-sync-api/tests/ui/query.test.mjs`. 045's existing JS tests stay.
- JS gets the `u` flag so `?` is one code point, like a Rust `char`.
- Rationale: two implementations can only drift where no shared case exists; the table is
  the contract.

## R3 Comma, braces and old values

- Decision: unquoted `,` is an OR operator (like `|`); `{a,b}` inside a pattern is
  alternatives. Old `whitelist_patterns = "*.mkv,*.mp4"` and `*.{mkv,mp4}` then need no
  special reading. Commas inside braces don't split.
- Alternative: a legacy pre-pass only for the two `[download]` keys — rejected: two
  dialects for the same field type.

## R4 Paths

- Decision: a term whose text contains `/` is tested against `folder/…/name` (the rule
  path form, relative to the Put.io root); a glob term matches if it matches the whole
  path or any tail starting after a `/`. Plain (non-glob) `/` terms match anywhere in the
  path. Other terms see the name only.
- Rationale: `Sample/*` should catch a Sample folder at any depth without `**`.

## R5 Strict check vs lenient parse

- Decision: `pattern::check(text) -> Result<Vec<String>, String>`. Errors: unclosed quote,
  unbalanced or nested braces, more than 64 alternatives in a term, only operators, over
  1000 characters. Warnings: ignored operators (leading/trailing/repeated OR, a dangling
  NOT or `-`), bracket look-alikes (`[Ss]`: one letter in both cases; `[0-9]`: a range).
- Config load and every save run the errors (via `Config::validate`); the assistant shows
  the warnings. The filter box keeps 045's lenient parse.

## R6 Where patterns sit in a decision

- Decision: `Effective` gains `include`/`exclude: Sourced<Pattern>` and a private folder
  path set by `resolve`; `decide(name)` order skip → exclude → include → type filter.
  Root values come from `whitelist_patterns` / `blacklist_patterns`; a rule's
  `Some("")` resets to none. Patterns never unblock.
- Rationale: no caller changes; the same per-setting inheritance as 009.

## R7 Preview

- Decision: reuse `POST /rules/preview`. New `global { include, exclude }` replaces the
  account-wide values in the candidate config and is allowed with `folder_id: 0`. The
  response gains `downloads` (first 50 files that would download). Settings calls it with
  the root.
- Alternative: a separate pattern-only preview — rejected: the user wants to see what the
  sync would do, which is the rule preview.

## R8 UI "clear inherited"

- Decision: blank input = inherit (null). When a pattern is inherited, the editor shows a
  "Don't inherit" checkbox that sends `""`.
