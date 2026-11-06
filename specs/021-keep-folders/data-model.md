# Data Model: Keep folders out of empty-folder cleanup

## Config

```toml
[[download.rules]]
path = "Movies"
keep = true          # new, optional: keep this folder and everything below out of cleanup

[cleanup]
keep_patterns = ["Season*", "Software/*"]   # new, default []
```

- `keep` is inherited by sub-folders; a deeper rule's `keep = false` clears it. A rule with
  only `keep` set is not empty.
- Each `keep_patterns` entry must pass `pattern::check` (feature 020) on load and save.

## Cleanup report (`EmptyFolderReport`)

| Field | Type | Notes |
|---|---|---|
| `kept` | list of `KeptFolder` | new: every empty folder kept, root excluded, in walk order |
| `kept_by_user` | integer | new: kept by a rule or a keep pattern |
| `kept_protected`, `kept_too_new` | integer | unchanged |

`KeptFolder`: `id` (Put.io file id), `path` (`/Movies`), `reason` (text).

Precedence when several apply: protected → kept by user (rule, then pattern) → too new.
