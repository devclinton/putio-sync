# Feature Specification: Notifications

**Feature Directory**: `specs/030-notifications`
**Created**: 2026-11-11
**Status**: Draft
**Input**: Roadmap 030. Notifications for finished, failed and stuck downloads, failed
transfers, low Put.io storage and doctor problems. ntfy, Discord, Telegram and Pushover are
bundled plugins (028) that pick their events, have a test button and group repeats. Still to
do: email over SMTP (which cannot be a plugin), stuck-download and doctor-problem events, and
a simpler Notifications page over the bundled channels.

## User Scenarios & Testing

### User Story 1 - Get an email when something happens (Priority: P1)

A user without a phone push service enters their mail server, a sender and one or more
recipients, picks the events, sends a test email, and from then on gets an email for each
of those events, with repeats of one kind within a few minutes grouped into one summary
email, like the push channels.

**Why this priority**: email is the one channel everybody has, and the roadmap names it.

**Independent Test**: point the email channel at a local test mail server, send a test,
emit events, and check the messages received (subject, body, recipients, grouping).

**Acceptance Scenarios**:

1. **Given** email set up with STARTTLS, implicit TLS or no encryption (for a LAN relay),
   **When** the user presses **Send test email**, **Then** a test message arrives, or the
   page shows why it did not (cannot connect, TLS failed, login refused, recipient
   refused), without the password.
2. **Given** email on for `download.failed`, **When** three downloads fail within the
   grouping window, **Then** one email is sent for the first and one summary for the other
   two once the window closes.
3. **Given** a saved password, **When** settings are read, exported or shown, **Then** the
   password reads `********`, saving the mask keeps it, and it never appears in logs.
4. **Given** the mail server is down, **When** an event is sent, **Then** the failure is
   logged and shown on the page as the channel's last error, and syncing goes on.

### User Story 2 - Be told about stuck transfers and doctor problems (Priority: P1)

A user is told when a Put.io transfer has stopped moving for the stall threshold, and when
the doctor finds a new problem, on any channel: email, the bundled plugins, their own Lua or
WebAssembly plugins and event hooks.

**Why this priority**: these are the two events the roadmap lists that do not exist yet.

**Independent Test**: feed the stall tracker a transfer that stops moving and run the
doctor with a failing check; both events reach a hook-style subscriber and a plugin.

**Acceptance Scenarios**:

1. **Given** a downloading transfer whose bytes stop changing, **When** the stall threshold
   passes, **Then** one `transfer.stuck` event is sent with its name, reason and since
   when; it is not sent again until the transfer moves and stalls again.
2. **Given** a doctor run where a check becomes a problem, **When** the run ends, **Then**
   one `doctor.problem` event is sent for that check; a check that was already a problem in
   the previous run sends nothing.
3. **Given** a plugin with `events = ["transfer.stuck"]` (Lua or WebAssembly), **When**
   the event is sent, **Then** the plugin receives it.

### User Story 3 - Set up notifications in one place (Priority: P2)

A user opens Settings › Notifications and sees every channel (Email, ntfy, Discord,
Telegram, Pushover) with whether it is on, the one or two values it needs, the events it
sends as checkboxes, and a test button. Turning one on and saving needs no trip to the
Plugins section; the full plugin settings stay there.

**Independent Test**: in the UI tests, the channel rows built from `GET /plugins`,
`GET /settings/plugins` and `GET /settings/notifications` show the right state and events,
and a save sends the plugin's events and values back unchanged otherwise.

**Acceptance Scenarios**:

1. **Given** ntfy is off, **When** the user enters a topic, ticks events, turns it on and
   saves, **Then** ntfy is enabled with those values, its host is approved, and the
   Plugins section shows the same.
2. **Given** the page is never opened, **When** the UI loads, **Then** none of its code is
   downloaded.

### Edge Cases

- No recipients, a bad address or a missing host: refused on save with the field named.
- The SMTP server needs a login over a plain connection: refused unless security is
  `none` and the user set that on purpose; the login is never sent without TLS.
- Many events at once: emails are grouped per event type; no more than one email per type
  per window after the first.
- A doctor check that flips between problem and fine sends again each time it becomes a
  problem, but at most once per run.

## Requirements

### Functional Requirements

- **FR-001**: The system MUST send `transfer.stuck` once per stall of a Put.io transfer,
  with `transfer_id`, `name`, `reason`, `stalled_since`, `size_bytes`, `percent_done`.
- **FR-002**: The system MUST send `doctor.problem` for each check that is a problem in a
  doctor run and was not in the previous run, with `check`, `title`, `summary`, `area`,
  `item_count`.
- **FR-003**: New events MUST be in EVENTS.md and the event schema, reach event hooks and
  Lua and WebAssembly plugins, and have text in the bundled notifiers, which send them by
  default.
- **FR-004**: An email channel MUST send over SMTP with STARTTLS, implicit TLS or none,
  optional login, a sender, one or more recipients, chosen events and grouping of repeats
  (same rules as the bundled notifiers), and a test send.
- **FR-005**: The SMTP password MUST be masked everywhere secrets are (API, export, logs).
- **FR-006**: A Notifications page MUST list email and the bundled notifier plugins with
  state, key values, events and a test button, saving through the existing settings, and
  load only when opened.
- **FR-007**: New UI strings MUST be in the i18n catalogs; the UI MUST stay under 488 KiB.

### Key Entities

- **Email channel**: `[notifications.smtp]`: on/off, host, port, security, username,
  password (secret), from, to, events, group_minutes.
- **Channel row**: one channel on the page: kind (email or plugin), on, values, events.

## Success Criteria

- **SC-001**: A test email reaches a local test server in under 5 s.
- **SC-002**: A stuck transfer and a new doctor problem each produce exactly one event.
- **SC-003**: A user turns on a push channel with events in one page and one save.
- **SC-004**: Release binary growth from email stays under 1.5 MB, with no C toolchain
  needed on any release target.

## Assumptions

- "Stuck downloads" are Put.io transfers that stop moving (feature 007's stall verdict);
  local downloads that stop already fail and retry, and end in `download.failed`.
- The old `[notifications]` backend/webhook/email fields never sent anything; they are kept
  for compatibility but the page no longer shows them.
- No HTML email; plain text is enough.
