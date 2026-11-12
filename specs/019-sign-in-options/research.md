# Research: Sign-in options

## Password hashing

- **Decision**: argon2id with the `argon2` crate's defaults (19 MiB, 2 passes, 1 lane),
  stored as a PHC string.
- **Rationale**: pure Rust, and the brief asks for it.
- **Alternatives**: bcrypt and scrypt. Both are fine, but neither was asked for.

## Sessions

- **Decision**: a cookie `putio_sync_session` = `v1.<payload b64url>.<HMAC-SHA256 b64url>`.
  - The payload holds how, login, name, expiry, and a stamp: the first 8 bytes of SHA-256
    of the password hash for password sessions.
  - The 32-byte key is in `<config dir>/session.key` (0600), made on first use.
  - "Sign out everywhere" writes a new key.
- **Rationale**: survives the weekly restart, and needs no database.
- **Alternatives**: a session table, which needs the database (it's optional); in memory,
  which signs everyone out on restart.

## CSRF

- **Decision**: the token is HMAC(key, "csrf" + cookie value), truncated to 16 bytes and
  hex-encoded.
  - `/auth/status` returns it, and the UI sends it as `X-CSRF-Token`.
  - The session cookie is also SameSite=Strict.
- **Rationale**: there's nothing to store, and it changes with each session.

## OAuth

- **Decision**: authorization code flow with PKCE S256 and a 32-byte random `state`.
  - The pending flow (verifier, provider, browser binding, return hash, created) is kept in
    memory for 10 minutes, at most 100 at once.
  - The browser binding is a random value in a SameSite=Lax, HttpOnly cookie scoped to
    `/auth/oauth`, so it is sent on the provider's top-level redirect back.
- **Endpoints**:

  | Provider | Authorize | Token | User |
  |---|---|---|---|
  | Google | accounts.google.com/o/oauth2/v2/auth | oauth2.googleapis.com/token | openidconnect.googleapis.com/v1/userinfo (`email`, `email_verified`, `name`) |
  | GitHub | github.com/login/oauth/authorize | github.com/login/oauth/access_token (Accept JSON) | api.github.com/user, plus api.github.com/user/emails (verified only) |
  | Discord | discord.com/oauth2/authorize | discord.com/api/oauth2/token | discord.com/api/users/@me (`username`, `email`, `verified`) |

- **Scopes**: Google `openid email profile`; GitHub `read:user user:email`; Discord
  `identify email`.
- **Discord**: OAuth2 is free for any application.
- **Redirect URL**: `<public_url>/auth/oauth/<provider>/callback`.
  - Without `public_url`, it's `X-Forwarded-Proto` (else http), then `X-Forwarded-Host`
    (else `Host`).
- **Alternatives**: the `oauth2` crate. It adds a dependency tree for three fixed
  providers, and hand-rolled code is small.

## Basic auth cost

- **Decision**: remember the SHA-256 of a verified `user:password` for 5 minutes (at most
  16 entries). The cache is cleared when the password changes.

## Rate limiting

- **Decision**: reuse `AuthLimiter`. Failed password sign-ins, failed Basic requests and
  refused OAuth accounts all count.
