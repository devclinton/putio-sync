# Data Model: What's New

## Release (parsed, in memory)

| Field | Type | Notes |
|-------|------|-------|
| `name` | string | `Unreleased` or the version (`0.2.0`), from `## [name]` |
| `date` | string or null | the text after ` - ` on the heading, if any |
| `kinds` | list of Kind | in file order; may be empty |

## Kind

| Field | Type | Notes |
|-------|------|-------|
| `name` | string | the `###` heading: `Added`, `Fixed`, `Changed`, … |
| `entries` | list of Entry | in file order |

## Entry

| Field | Type | Notes |
|-------|------|-------|
| `id` | string, 16 hex digits | FNV-1a 64 of `text` (research R2) |
| `text` | string | the bullet without `- `, continuation lines joined by one space |
| `new` | bool | in API responses only: id not in `whats_new_seen` |

Identity: two entries with the same normalised text have the same id, wherever they are.
The parser keeps both; the UI shows both; marking one seen marks both.

## whats_new_seen (SQLite)

```sql
CREATE TABLE IF NOT EXISTS whats_new_seen (
    id TEXT PRIMARY KEY,
    seen_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
```

- Rows are only added (`INSERT OR IGNORE`); nothing removes them. An id no longer in the
  changelog is harmless.
- Empty table = first run (research R4): the next `GET /whats-new` inserts every built-in id.

### State of an entry

```text
(build adds entry) ──► new ──(panel closed: POST /whats-new/seen)──► seen
first run: every entry goes straight to seen
```
