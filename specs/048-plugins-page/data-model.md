# Data Model: Plugins Settings Page

No stored data changes. The UI works with three shapes it already has.

## Plugin status (`GET /plugins` → `plugins[]`)

Unchanged. Used fields: `name`, `version`, `description`, `source`, `state`
(`disabled | enabled | broken | needs_approval | missing_config`), `error`, `events`,
`permissions.http.{hosts, host_from_config}`, `config_fields` (absent or empty when the
manifest did not load), `actions`, `runs`.

## Plugin settings (`GET|PUT /settings/plugins` → `plugins[]`)

Unchanged `[[plugins]]` entry: `name`, `enabled`, `approved_hosts`, `timeout_secs`,
`memory_mb`, `config`, `secrets` (masked as `********` when read).

## Editor row (`ui/plugins.js` `fromStatus`)

Unchanged, except that `enabled` is no longer edited on the tab; it follows the saved entry
and changes only through Enable and Disable.

## Derived (new helpers in `ui/plugins.js`)

- `split(found, stored)` → `{ tabs, available }`: `tabs` are found plugins whose saved entry
  has `enabled: true`; `available` the rest; both in name order.
- `summary(description)` → the first sentence or line.
- `approvalText(status, row)` → the confirmation text: the hosts from `hostsFor`, or "no
  network calls", plus the labels of URL fields that have no value yet.
- `withEnabled(stored, status, on)` → the `[[plugins]]` list with that plugin's entry set to
  `enabled: on`, all others as saved; a plugin with no entry gets one from its defaults.

## State transitions

```text
Available ──Enable + approve──▶ Tab (enabled; state enabled | missing_config | broken)
Tab ──Disable──▶ Available (entry kept, enabled = false)
```
