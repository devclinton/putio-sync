# Contract: sign-in API

## Public (no credentials)

### `GET /auth/status`

The existing fields stay. These are added:

```json
{
  "token_configured": true,
  "setup_required": false,
  "sign_in": { "password": true, "providers": ["google", "github"] },
  "user": { "login": "alice@example.com", "name": "Alice", "via": "google", "csrf": "9f…" }
}
```

`user` is present only for a valid session cookie.

### `POST /auth/password`

The body is `{"username": "…", "password": "…"}`.

| Status | When |
|---|---|
| 204 | Signed in. `Set-Cookie: putio_sync_session=…; HttpOnly; SameSite=Strict; Path=/; Max-Age=…[; Secure]` |
| 401 | Wrong username or password (counted) |
| 404 | Password sign-in isn't set up |
| 429 | Rate limited, before checking |

### `POST /auth/logout`

Answers 204 and clears the cookie (`Max-Age=0`). The session isn't checked, so a stale
cookie can always be cleared.

### `GET /auth/oauth/:provider?return=<hash>`

| Status | When |
|---|---|
| 302 | To the provider's authorize URL with `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state`, `code_challenge`, `code_challenge_method=S256`. Sets `putio_sync_oauth=<binding>; HttpOnly; SameSite=Lax; Path=/auth/oauth; Max-Age=600` |
| 404 | Unknown provider, or one that isn't on |
| 429 | Rate limited |

### `GET /auth/oauth/:provider/callback?code=…&state=…`

| Status | When |
|---|---|
| 302 to `/#<return>` | Signed in. Sets the session cookie and clears the flow cookie |
| 302 to `/#/login?error=<code>` | Refused. The codes are below |

Error codes:

- `denied`: the provider returned `error`;
- `state`: an unknown, expired or reused state, or the wrong browser;
- `exchange`: the token or user call failed;
- `not_allowed`: the account isn't listed or the email isn't verified (counted).

## Protected

- `Authorization: Basic <b64 user:pass>` is accepted wherever the token is.
- A session cookie is accepted wherever the token is. Writes also need `X-CSRF-Token`
  (403 "CSRF token missing or wrong").

### `GET /settings/sign-in`

```json
{
  "public_url": null,
  "session_hours": 168,
  "password": { "username": "admin", "set": true },
  "providers": {
    "google": { "enabled": true, "client_id": "…", "client_secret": "********", "allowed_users": ["alice@example.com"], "redirect_url": "https://sync.example.com/auth/oauth/google/callback" },
    "github": { … },
    "discord": { … }
  }
}
```

`redirect_url` is built from `public_url`, or from this request when it isn't set.

### `PUT /settings/sign-in`

The body is the same shape, with these differences:

- `password.new_password` (optional) sets a new password. `password.clear: true` removes
  it.
- `client_secret: "********"` keeps the stored secret.

It answers `{"message": "Sign-in settings saved…"}`. Changing the password or the username
ends password sessions.

### `POST /settings/sign-in/sign-out-everywhere`

Writes a new session key and answers 204. Every session, this one included, ends.
