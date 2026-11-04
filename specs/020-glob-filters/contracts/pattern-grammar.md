# Contract: Pattern Grammar (045 + 020)

One language for the list filter box (`ui/query.js`) and saved patterns
(`putio_sync_core::pattern`). 045's grammar
(`specs/045-select-sort-filter/contracts/filter-grammar.md`) with three additions.

```text
filter  := group (WS group)*            ; every group must match (AND)
group   := term (WS? OR WS? term)*      ; any term matches (OR)
OR      := "OR" | "or" | ("|" | ",")+   ; any case for the word        (020: comma)
term    := ("-" | "NOT" WS)? pattern    ; negated when prefixed
pattern := quoted | bare
quoted  := '"' any-but-quote* '"'?
bare    := ( "{" [^}\s"|]* "}"? | [^\s|",{] )+                       (020: braces)
```

- `{a,b,c}` inside a pattern (bare or quoted) means any one of the alternatives; commas
  inside braces do not split terms. No nesting.
- 020: a term containing `/` is matched against the file's path (`Folder/Sub/name`,
  relative to the Put.io root). A glob term matches when it matches the whole path or a
  tail of it starting after a `/`; a plain term matches anywhere in the path. Lists that
  pass only a name use the name as the path.
- Unchanged: case-insensitive; `*` any run, `?` one character (code point); with either it
  must match the whole name (or path tail), without it matches anywhere; brackets and
  every other character are literal; `AND` is skipped; leftover operators are ignored.

## Strict check (saved patterns only)

| Kind | Cases |
|------|-------|
| Error | unclosed `"`; `{` without `}` or `}` without `{`; nested `{`; more than 64 alternatives in a term; non-empty text with no term; over 1000 characters |
| Warning | leading, trailing or repeated OR; `NOT` or `-` with nothing after; `[Ss]`-style or `[0-9]`-style brackets (literal, case never matters) |

## Shared cases

`putio-sync-core/tests/data/filter-cases.json`:

```json
[{ "filter": "*.mkv,*.mp4", "names": ["a.mkv", "b.MP4", "c.avi"], "shown": ["a.mkv", "b.MP4"] },
 { "filter": "Sample/*", "paths": ["Movies/Sample/a.mkv", "Movies/a.mkv"], "shown": ["Movies/Sample/a.mkv"] }]
```

With `paths`, the name is the last segment. Both suites assert `shown` exactly.

## `ui/query.js`

`filterMatcher(text) -> (name, path = name) => bool` (second argument new, optional).
