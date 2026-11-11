# Data Model: Notifications

## Email channel: `[notifications.smtp]` (`SmtpConfig`, putio-sync-core/src/email.rs)

| Field | Type | Default | Rules |
|---|---|---|---|
| `enabled` | bool | `false` | When on, `host`, `from` and `to` are needed |
| `host` | string | `""` | A host name, no spaces or `/` |
| `port` | u16 or none | none | 1–65535; none means the usual port for `security` (587, 465, 25) |
| `security` | `starttls` \| `tls` \| `none` | `starttls` | See research R2 |
| `username` | string | `""` | Empty for no login; refused with `none` |
| `password` | string, secret | `""` | Shown as `********` when set; the mask on save keeps the saved one |
| `from` | address | `""` | `name@host` or `Name <name@host>` |
| `to` | list of addresses | `[]` | At least one when enabled; each checked |
| `events` | list of event types | the 7 in `DEFAULT_EVENTS` | Each must be a known event type |
| `group_minutes` | u32 | 5 | 0 sends each event |

Errors name the field: `notifications.smtp.<field>: <why>`.
`DEFAULT_EVENTS`: `download.finished`, `download.failed`, `transfer.failed`,
`transfer.stuck`, `sync.completed`, `storage.low`, `doctor.problem`. These are the same
defaults the bundled notifier plugins use.

The legacy `[notifications]` fields (`enabled`, `backend`, `webhook_url`, `email`) stay in
the API and config for old files; the page no longer shows them.

## Email status (`EmailStatus`, putio-sync-notify, read-only)

| Field | Type | Meaning |
|---|---|---|
| `enabled` | bool | Whether the channel sends |
| `sent` | u64 | Emails the server took since the daemon started |
| `failed` | u64 | Emails that failed since the daemon started |
| `last` | `{at, subject, error}` or null | The last email sent or tried, the test included; `error` is null on success |

The counts reset when the daemon restarts; nothing is stored.

## New events (see [contracts/events.md](contracts/events.md))

- `transfer.stuck`: `transfer_id`, `name`, `reason`, `stalled_since`, `size_bytes`,
  `percent_done`.
- `doctor.problem`: `check`, `title`, `summary`, `area`, `item_count`.

## Notifier row (UI only, `ui/notify.js`)

One bundled notifier plugin (`ntfy`, `discord`, `telegram`, `pushover`, in that order) as
the page shows it: the plugin's state from `GET /plugins`, its saved `[[plugins]]` entry,
a summary of its settings that are not secret (secrets only as set or not set), and its
`notify` events. Changing an event or turning it on or off writes the whole `[[plugins]]`
list through `PUT /settings/plugins`, keeping every other plugin's entry and the masked
secrets as they are.

## State: email delivery

```text
event on bus ── not chosen or off ──▶ dropped
      │
      ▼ chosen
grouper: first of its type in the window? ── yes ──▶ queue ──▶ send ──▶ sent | failed
      │ no
      ▼
counted; window closes (15 s tick) ──▶ one summary ──▶ queue
```
