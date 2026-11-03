# Contract: Filter Grammar and Sort Keys

What the operator types in the filter box on the Put.io, Transfers and Local files pages, and
what `ui/query.js` exports to the pages.

## Grammar

```text
filter  := group (WS group)*            ; every group must match (AND)
group   := term (WS? OR WS? term)*      ; any term matches (OR)
OR      := "OR" | "or" | "|"            ; any case for the word
term    := ("-" | "NOT" WS)? pattern    ; negated when prefixed
pattern := quoted | bare
quoted  := '"' any-but-quote* '"'?      ; an unclosed quote runs to the end
```

- `AND` (any case) as its own word is skipped.
- `|` is an operator wherever it appears outside quotes, so `a|b` is `a OR b`.
- A lone `-` or `NOT` with nothing after it, a leading or trailing `OR`, and repeated `OR`s are
  ignored.
- Pattern: case-insensitive. With `*` (any run) or `?` (one character) it must match the whole
  name; without, it matches anywhere. All other characters are literal.

## Examples

| Filter | Shows names that… |
|--------|-------------------|
| `movie` | contain "movie" |
| `*.mkv` | end in ".mkv" |
| `-*photoshop* *exe*` | contain "exe" and not "photoshop" |
| `*.mkv OR *.mp4 -sample` | end in .mkv or .mp4, and don't contain "sample" |
| `*.mkv\|*.nfo` | end in .mkv or .nfo |
| `"the office"` | contain "the office" |
| `NOT x264` | don't contain "x264" |
| `[YTS]` | contain "[YTS]" |
| `a?c` | are exactly three characters: a, any, c |

## Module: `ui/query.js`

```text
parseFilter(text) -> Group[]           ; Group = Term[]; Term = { re: RegExp, not: boolean }
filterMatcher(text) -> (name) => bool  ; true for every name when the filter is empty
parseSort(text, keys) -> { key, desc } | null   ; "size-desc" → { key: "size", desc: true }
formatSort({ key, desc }) -> string    ; inverse of parseSort
sortRows(rows, sort, columns, { folder }) -> rows'   ; new array; input unchanged
```

- `columns`: `{ key: (row) => value }`; strings compare with `localeCompare` (numeric,
  base sensitivity), numbers and dates numerically; `null`/`undefined` sort lowest.
- `folder(row)`: when given, folders come before files whatever the direction.
- `sort == null` returns the rows in their given order.
- Ties break on `columns.name`, then the original order (stable).

## Address

`#/<page>[/<path>]?…&q=<filter>&sort=<key>-<asc|desc>`. Empty filter and no sort remove the
parameters.
