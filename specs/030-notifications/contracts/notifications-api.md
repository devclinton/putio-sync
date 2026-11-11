# Contract: notifications settings API

All routes need the API token.

## `GET /settings/notifications`

```json
{
  "enabled": false,
  "backend": "None",
  "webhook_url": null,
  "email": null,
  "smtp": {
    "enabled": true,
    "host": "smtp.example.com",
    "security": "starttls",
    "username": "nas",
    "password": "********",
    "from": "putio-sync <nas@example.com>",
    "to": ["me@example.com"],
    "events": ["download.finished", "transfer.stuck", "doctor.problem"],
    "group_minutes": 5
  },
  "smtp_status": {
    "enabled": true,
    "sent": 3,
    "failed": 1,
    "last": { "at": "2026-11-11T10:00:00Z", "subject": "Downloaded", "error": null }
  }
}
```

- `smtp.port` is left out when not set (the usual port for `security`).
- `smtp.password` is `"********"` when set and `""` when not; the real value is never in
  a response, the export or the logs.
- `smtp_status` is read-only and ignored on PUT.

## `PUT /settings/notifications`

Body: the GET body (`smtp_status` may be left out). `smtp.password`:

- `"********"`: keep the saved password;
- any other string: replace it; `""` clears it.

Answers like every settings PUT. Email changes apply now:
`"Notification settings saved. Applies now: smtp."`. 422 names the field, such as
`notifications.smtp.host: a mail server name such as smtp.example.com is needed` or
`notifications.smtp.username: a login needs security "starttls" or "tls"; it is never
sent unencrypted`.

## `POST /settings/notifications/test`

Body: an `smtp` object as above (a masked password means the saved one). Sends one test
email now with those settings, whether email is on or not, and counts it in
`smtp_status`.

```json
{ "ok": true }
{ "ok": false, "error": "smtp.example.com:587: TLS failed (...)" }
```

422 when the settings do not validate. A failed send is a 200 with `ok: false`, as the
request itself worked.

## Unchanged routes the page uses

- `GET /plugins`: the bundled notifiers' state and `config_fields`.
- `GET`/`PUT /settings/plugins`: the `[[plugins]]` list; the page changes `enabled` and
  `config.notify` for one notifier and sends the rest as it read it.
- `POST /plugins/test`: the notifier's **Send test**.
