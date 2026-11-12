# Data model: Sign-in options

## `[sign_in]` (config file)

| Key | Type | Default | Notes |
|---|---|---|---|
| `public_url` | string? | none | `http(s)://host[:port][/path]`, no query; a trailing `/` is ignored |
| `session_hours` | int | 168 | 1–8760 |
| `password.username` | string | "" | needed when a hash is set |
| `password.password_hash` | string | "" | argon2id PHC; masked |
| `<provider>.enabled` | bool | false | provider is `google`, `github` or `discord` |
| `<provider>.client_id` | string | "" | |
| `<provider>.client_secret` | string | "" | masked |
| `<provider>.allowed_users` | [string] | [] | email, `@domain`, or account name |

Validation (on save and load):

- `public_url` must parse as http(s), with no query or fragment.
- `session_hours` must be in range.
- A hash must start with `$argon2`.
- An enabled provider needs a client ID, a secret and at least one allowed user.
- Entries can't be empty or contain spaces.

## Session (cookie payload, JSON)

`{"v":1,"how":"password|google|github|discord","login":"…","name":"…","exp":unix,"stamp":"hex"}`

The session is valid when:

- the signature matches;
- `exp` is in the future;
- the method is still on;
- for the password, the login equals `username` and the stamp matches the current hash;
- for a provider, the login is still allowed by the current `allowed_users`.

## Pending OAuth flow (memory)

| Field | Notes |
|---|---|
| `state` | key, 32 random bytes b64url |
| `verifier` | PKCE verifier, 32 random bytes b64url |
| `provider` | |
| `binding` | SHA-256 of the browser cookie's value |
| `redirect_uri` | as sent, reused in the exchange |
| `created` | 10-minute lifetime; removed on use |
