# Research: Notifications

## R1. SMTP crate

**Decision**: lettre 0.11 (0.11.23 in the lock file) with `default-features = false` and
the features `smtp-transport`, `builder`, `tokio1-rustls`, `ring` and
`rustls-native-certs`.

**Rationale**:

- Well kept: lettre is the Rust SMTP crate most projects use, with regular releases and an
  async transport on Tokio, which the daemon already runs.
- rustls, not OpenSSL: `tokio1-rustls` with the `ring` provider. reqwest already brings in
  rustls and ring, so nothing new is compiled for TLS and no release target (armv7,
  aarch64 and x86_64 musl, Windows, macOS) needs a C toolchain or OpenSSL headers.
- Small feature set: no `sendmail` or `file` transports, no `native-tls`, no `pool`
  (a few emails an hour do not need pooled connections), no `hostname` (the EHLO name is
  lettre's default), no `mime03`.

**Size** (release profile, aarch64-apple-darwin, rustc 1.96, stripped):

| Build | Binary | Growth |
|---|---|---|
| Scratch binary with tokio and rustls (ring), no lettre | 1,118,928 B | — |
| The same with lettre and the features above | 2,393,968 B | +1,275,040 B (about 1.2 MiB) |
| putio-sync release binary before 030 | 55,679,808 B | — |
| putio-sync release binary with 030 | 56,546,336 B | +866,528 B (about 0.83 MiB) |

The scratch number is an upper bound: it counts the SMTP code and lettre's message
builder (MIME, encoders, address parsing) and `rustls-native-certs`, while the daemon
already has some of what lettre uses (base64, idna, url parsing).

### Measured in the daemon

`cargo build --release -p putio-sync-cli` on this branch, same machine and toolchain as
the baseline: 55,679,808 B before, 56,546,336 B after, +866,528 B. That growth is the
whole feature (the new events, settings and routes too), so email itself costs a little
less. SC-004 (under 1.5 MB) holds.

**Alternatives considered**:

- `mail-send` (Stalwart): async and rustls, but also brings in its own MIME builder
  (`mail-builder`) and DKIM signing; about as big, less used.
- `async-smtp`: smaller, but kept for deltachat's needs, with fewer releases and no
  message builder.
- Writing SMTP by hand: STARTTLS, AUTH and dot-stuffing are easy to get wrong, and the
  TLS handling below is the part that matters.
- A plugin: plugins get `putio.http` only, not raw TCP and TLS, so email cannot be a
  plugin without a new host API, which is a bigger change than one crate.

## R2. TLS handling

**Decision**: three settings, `security = "starttls" | "tls" | "none"`.

- `starttls` (default, port 587): `starttls_relay`, which sets lettre's `Tls::Required`.
  If the server does not offer STARTTLS, or the upgrade fails, the send fails with
  `TLS failed`; nothing falls back to plain text. The test
  `starttls_that_the_server_lacks_fails_without_sending` checks that the fake server sees
  neither `AUTH` nor `MAIL`.
- `tls` (port 465): `relay`, TLS from the first byte (`Tls::Wrapper`).
- `none` (port 25): `builder_dangerous`, for a relay on the LAN. A username with `none`
  is refused on save (`a login needs security "starttls" or "tls"`), and the sender does
  not attach a login even if one got in, so a password never goes out unencrypted.
- Certificates: rustls with the OS store (`rustls-native-certs`) and the host name
  checked. The Docker image already installs `ca-certificates`. There is no option to
  skip verification; a home server with a self-signed certificate can use `none` on the
  LAN.
- Timeout: 30 s per send, connecting included. Errors name `host:port` and the reason
  (no answer, TLS failed, refused with the server's reply) and never the login.

## R3. Where the new events come from

**Decision**:

- `transfer.stuck`: the stall tracker (`putio-sync-core/src/stall.rs`) already decides
  when a transfer has stalled for the UI. It now emits one event when a transfer goes
  from not stalled to stalled; it emits again only after the transfer moves and stalls
  again. No second timer or threshold.
- `doctor.problem`: the doctor service keeps the previous report; after each run, each
  check that is a problem now and was not in the previous run emits one event. The first
  run after a start has no previous report, so a problem found then is sent once.

**Rationale**: one source of truth per condition, so the page, the Doctor and the events
agree; and no repeats every poll.

## R4. Grouping and delivery

**Decision**: the same rules as `notifier.lua`: the first event of a type is sent at once,
later ones in the window (`group_minutes`, default 5, 0 = no grouping) are counted and
sent as one summary when the window closes (checked every 15 s). Messages go through a
queue of 100 to one sender task, so a slow server never holds the event bus; past 100,
new emails are dropped with a warning. A bus lag is logged and skipped.

## R5. Page size

**Decision**: Settings › Notifications is loaded with `import()` the first time the
section opens. `notify.js` (the pure logic) and `settings/notifications.js` are only
reachable from that import, so the UI size test, which follows static imports, does not
count them. The General section loses its old notifications card, which left the static
UI a few bytes smaller than before.
