# Data Model: Multi-select and Bulk Actions

No storage changes. Request and response shapes are in [contracts/api.md](contracts/api.md).

## StatusGroup (API)

| Value | Put.io statuses (case-insensitive) |
|---|---|
| `completed` | COMPLETED, SEEDING |
| `failed` | ERROR, FAILED |
| `finished` | all of the above |

## Selection (UI, in memory)

- Put.io page: `Set` of file ids. Cleared on folder change; narrowed to visible rows when the
  filter changes; ids of queued rows are never added. After a bulk action the listing reloads
  and the selection is cleared.
- Transfers page: `Set` of transfer ids. On each 5 s refresh, ids no longer listed are
  dropped. Cleared after a bulk action.

## Clear flow

```text
list ─► filter(group) ─► none? ─► count 0, no further calls
                     └► dry_run? ─► report
                                └► clean(ids) ─► leftover = ids − deleted_ids
                                               └► leftover? ─► cancel(leftover)
```
