# Contract: new events

Both use the usual envelope (`schema_version`, `id`, `type`, `occurred_at`, `source`,
`data`) and are in `putio-sync-core/src/events/schema.json`. Full text and examples are in
EVENTS.md.

## `transfer.stuck`

| Field | Type | Notes |
|---|---|---|
| `transfer_id` | integer | Put.io transfer id |
| `name` | string | transfer name |
| `reason` | string | `availability N%`, `no peers` or `no progress` |
| `stalled_since` | string (RFC 3339) | when it last made progress |
| `size_bytes` | integer or null | size, when Put.io knows it |
| `percent_done` | integer | Put.io's percent done |

Once per stall. A transfer that moves and stalls again sends it again. Stall state is in
memory, so a restart starts the clock again.

## `doctor.problem`

| Field | Type | Notes |
|---|---|---|
| `check` | string | check id, such as `disk_space` |
| `title` | string | check title |
| `summary` | string | what it found |
| `area` | string | `Put.io`, `Sync`, `Local` or `Connectivity` |
| `item_count` | integer | items found; 0 when the check does not count |

Once per check that is a problem now and was not in the previous run. Warnings send
nothing.

## Who gets them

- Event hooks (`[[hooks]]` with `events`), by type like every event.
- Lua and WebAssembly plugins that list them in `events`, or `*`.
- The bundled notifiers (`ntfy`, `discord`, `telegram`, `pushover`): in their default
  `notify` list, with text:
  - `Transfer stuck` / `<name>: <reason> at <percent>%`
  - `Doctor: <title>` / `<summary>`
  - grouped summaries say `N transfers stuck` and `N doctor problems`.
- Email, when chosen in `[notifications.smtp] events` (both are in the defaults).
