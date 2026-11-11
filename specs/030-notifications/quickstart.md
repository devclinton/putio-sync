# Quickstart: Notifications

Run from the repository root.

## Tests

```bash
cargo test -p putio-sync-notify
cargo test -p putio-sync-core --lib
cargo test -p putio-sync-doctor
cargo test -p putio-sync-plugins --test bundled
cargo test -p putio-sync-api --test settings --test secrets_sweep --test settings_assistant --test ui_assets
node --test putio-sync-api/tests/ui/notify.test.mjs putio-sync-api/tests/ui/settings_sections.test.mjs
```

Expected: a fake SMTP server on localhost gets one plain-text email per recipient with no
login over a plain connection; STARTTLS that the server lacks fails before `MAIL` or
`AUTH`; three events of one type send one email and later ones are grouped; a stall sends
`transfer.stuck` once; a new doctor problem sends `doctor.problem` once; the SMTP password
never shows in a response or a log; the static UI stays under 488 KiB.

## By hand

1. Start a local mail catcher on port 1025, for example MailHog or
   `python3 -m aiosmtpd -n -l 127.0.0.1:1025`.
2. Start the daemon, open Settings › **Notifications**. The Email and Push notifications
   cards load when the section opens.
3. Email: tick **Send email**, mail server `127.0.0.1`, security **None**, port `1025`,
   a From and a To address. **Save** says `Applies now: smtp`. **Send test email**: the
   catcher shows `putio-sync: Test email`, and the status line says `1 sent`.
4. Set security to **STARTTLS** with the same catcher: **Send test email** fails with
   `TLS failed` and the catcher gets nothing.
5. Push notifications: **Enable** ntfy (it asks to approve its hosts), set its topic under
   **Edit settings**, come back, tick **Transfer stuck**, and **Send test**: the phone gets
   the test notification.
6. Run the Doctor with a problem (for example a missing download folder): one
   `doctor.problem` reaches email and ntfy; running it again sends nothing more.
