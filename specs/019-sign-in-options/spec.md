# Feature Specification: Sign-in options

**Feature Directory**: `specs/019-sign-in-options`
**Created**: 2026-11-11
**Status**: Draft
**Input**: Roadmap 019. The web UI and API are protected by one API token today. Add HTTP
basic auth with a username and password, and sign-in through OAuth providers: Google,
GitHub, and Discord if its OAuth stays free. Limit who may sign in by account or email, and
keep the API token for scripts and Radarr/Sonarr.

## Current state

- Every protected route needs `Authorization: Bearer <api token>` (or the raw token). The
  web UI asks for the token once, checks it with `POST /auth/login` and keeps it in the
  browser's local storage.
- Wrong tokens are counted per client IP: 5 in a minute gives 429 until the minute ends.
- With `[tailscale] identity_sign_in` on (036), a request with no valid token is let in when
  its Tailscale login is on `allowed_users`.
- `/transmission/rpc` (Radarr/Sonarr's download client) uses its own Basic auth from
  `[arr] username`/`password`.
- First run with no config serves the setup page, protected by a setup code printed in the
  log (013).

## User Scenarios & Testing

### User Story 1 - Sign in with a username and password (Priority: P1)

An operator sets a username and password in Settings › Sign-in. From then on the sign-in
page offers a username and password form next to the API token. A script can send the
same username and password as HTTP Basic auth instead of the token.

**Why this priority**: a password is easier to remember and type than a token, and it needs
no outside account.

**Independent Test**: set a password through the API; sign in with the form and use the
session cookie for reads and writes; call a protected route with Basic auth; check wrong
passwords are refused and rate limited.

**Acceptance Scenarios**:

1. **Given** a username and password are set, **When** the form gets them, **Then** the
   browser gets a session cookie (HttpOnly, SameSite=Strict, Secure behind HTTPS) and the
   UI works without the token.
2. **Given** a session cookie, **When** the UI sends a write (POST, PUT, DELETE) without
   the CSRF token it got from `/auth/status`, **Then** it is refused with 403. Reads don't
   need it, and token or Basic requests never do.
3. **Given** a protected route, **When** a request has `Authorization: Basic` with the
   right username and password, **Then** it is answered; with a wrong one it gets 401 and
   counts toward the rate limit.
4. **Given** 5 failed sign-ins from one IP in a minute, **When** a sixth is tried, **Then**
   it gets 429 with Retry-After, before the password is checked.
5. **Given** the password is changed, **When** an old session is used, **Then** it is
   refused, so changing the password signs everyone out.
6. **Given** the stored password, **When** settings are read or exported, **Then** only
   "set" or "not set" is shown. The file keeps a slow hash (argon2id), never the password.

### User Story 2 - Sign in with Google, GitHub or Discord (Priority: P1)

An operator registers an OAuth app with a provider, enters its client ID and secret, and
lists who may sign in (emails, `@domain`, or account names). The sign-in page shows
"Sign in with Google" (GitHub, Discord). A listed person signs in with one click. Anyone
else is refused.

**Why this priority**: it's the main ask of the roadmap item, and it means no shared secret
to hand out.

**Independent Test**: point the providers at a fake OAuth server; follow the authorize
redirect, call back with a code, and check the session; check a user who isn't listed, a
wrong state and a missing PKCE verifier are refused.

**Acceptance Scenarios**:

1. **Given** Google is set up, **When** the user presses Sign in with Google, **Then** the
   browser goes to Google with `state`, a PKCE S256 `code_challenge`, and the redirect URL
   `<public URL>/auth/oauth/google/callback`.
2. **Given** the provider calls back with a code and the matching state, **When** the code
   is exchanged (with the PKCE verifier) and the account is on the allow-list, **Then** a
   session starts and the browser lands on the page it came from.
3. **Given** an account that isn't listed, or an email the provider hasn't verified,
   **Then** no session starts, the sign-in page says the account isn't allowed, the
   refusal is logged with the account name, and it counts toward the rate limit.
4. **Given** a callback with an unknown, reused or expired state, or one opened in another
   browser, **Then** it is refused.
5. **Given** putio-sync behind a reverse proxy, **When** `public_url` is set, **Then**
   redirect URLs use it. Without it, they are built from the request's host and the
   proxy's `X-Forwarded-Proto` and `X-Forwarded-Host`. Settings shows the exact URL to
   register.
6. **Given** a client secret is saved, **When** settings are read, **Then** it shows as
   `********`. Saving the mask keeps it, and it never reaches a log.

### User Story 3 - Keep the API token for scripts and Radarr/Sonarr (Priority: P1)

Nothing that uses the token or the `[arr]` password changes.

**Acceptance Scenarios**:

1. **Given** any sign-in options, **When** a request has the API token, **Then** it works
   exactly as before.
2. **Given** `/transmission/rpc` and `/arr/*`, **Then** they authenticate exactly as today.
3. **Given** no API token configured, **Then** password and OAuth sign-in still work, and
   the old "no token" notice shows only when no sign-in option is set at all.

### User Story 4 - Know when a provider is set up wrong (Priority: P2)

The doctor's new **Sign-in options** check reports:

- a problem when a provider is on without a client ID or secret, or with nobody allowed;
- a problem when `public_url` isn't an http(s) URL;
- a warning when a provider is on without `public_url` (redirects then depend on the
  proxy's headers);
- a warning when the password is set without a username.

### Edge Cases

- **Tailscale.** Tailscale identity sign-in keeps working. A session or Basic auth is
  checked before Tailscale, after the token.
- **Allow-list changes.** A session whose account is taken off the allow-list, or whose
  provider is turned off, stops working at the next request.
- **Discord.** Discord's OAuth is free. It's offered like the others.
- **Signing out.** Sign out clears the session cookie. **Sign out everywhere** in Settings
  rotates the session key, which ends every session.
- **Restarts.** Sessions survive a restart: the key is in a file next to the config, 0600.
  Without a config file, the key lives in memory only.
- **Missing database.** None is needed: sessions are signed cookies, so sign-in works with
  or without the database.
- **Setup mode.** Unchanged.

## Requirements

### Functional Requirements

- **FR-001**: A `[sign_in]` config section holds:
  - `public_url`;
  - `session_hours` (default 168);
  - `[sign_in.password]` with `username` and `password_hash`;
  - `[sign_in.google]`, `[sign_in.github]` and `[sign_in.discord]`, each with `enabled`,
    `client_id`, `client_secret` and `allowed_users`.
- **FR-002**: Passwords are stored as argon2id PHC strings. The API takes a new password in
  plain text and stores only the hash.
- **FR-003**: The auth middleware lets a request in when any of these holds, in order:
  1. the API token;
  2. HTTP Basic with the password;
  3. a valid session cookie (with CSRF for writes);
  4. Tailscale identity.

  Wrong credentials of any kind count toward the per-IP limiter.
- **FR-004**: The OAuth flow:
  - uses the authorization code flow with PKCE (S256) and a random `state`;
  - binds the flow to the browser with a short-lived SameSite=Lax cookie;
  - keeps the pending state for 10 minutes, and each state works once.
- **FR-005**: The allow-list:
  - Entries match the account's verified email, or an `@domain` that ends it, or the
    account name (GitHub login, Discord username), ignoring case.
  - Unverified emails never match.
- **FR-006**: The session cookie:
  - It is signed, HttpOnly and SameSite=Strict, and Secure when the request came over
    HTTPS.
  - It expires after `session_hours`.
  - It carries who and how. It's checked against the current config on every request.
- **FR-007**: CSRF: a cookie-authenticated request that isn't GET, HEAD or OPTIONS needs
  `X-CSRF-Token` equal to the token `/auth/status` gives that session.
- **FR-008**: API:
  - `GET /auth/status` adds `sign_in` (password on or off, the providers that are on) and
    `user` (login, name, how, CSRF token) for a session.
  - `POST /auth/password` signs in with the password.
  - `GET /auth/oauth/:provider` starts the provider flow.
  - `GET /auth/oauth/:provider/callback` finishes it.
  - `POST /auth/logout` signs out.
  - `GET` and `PUT /settings/sign-in` read and save the settings.
  - `POST /settings/sign-in/sign-out-everywhere` ends every session.
- **FR-009**: Client secrets and the password hash are masked like other secrets and added
  to the log redaction list.
- **FR-010**: The doctor check `sign_in` follows User Story 4.
- **FR-011**: The UI:
  - The sign-in page offers the password form and provider buttons when they're set up,
    and the API token form otherwise or as well.
  - The top bar shows who is signed in and how.
  - Settings › Sign-in edits it all. It's lazy-loaded, and the sign-in page code is
    loaded with `import()`, so the always-loaded UI stays under its cap.

### Key Entities

- **Sign-in settings**: the `[sign_in]` section.
- **Session**: who (login, display name), how (password, google, github or discord),
  expiry, and a password stamp. It is signed with the session key.
- **Pending OAuth flow**: state, PKCE verifier, provider, browser binding, where to return,
  and when it was made.

## Success Criteria

- **SC-001**: Every sign-in path has a test: token, Basic, the password form and its
  session, OAuth for each provider against a fake server, and Tailscale (existing).
- **SC-002**: The existing token, `/arr` and `/transmission/rpc` tests pass unchanged.
- **SC-003**: Refused paths are tested: a wrong password, rate limiting, a missing CSRF
  token, a bad state, an account that isn't listed, an expired session, and a changed
  password.
- **SC-004**: Password checks use argon2id. Basic requests don't run argon2 for every
  request: a verified credential is remembered for a few minutes.
- **SC-005**: The always-loaded UI stays under 499,712 B.

## Assumptions

- Discord OAuth is still free. It is.
- putio-sync has no TLS listener of its own, so HTTPS means a proxy that sets
  `X-Forwarded-Proto: https`, or a `public_url` starting with `https://`.
- A provider's endpoints are fixed. Tests point them at a fake server.
