# Contract: Put.io device link (outbound)

Base `https://api.put.io/v2` (overridable in tests).

## Request a link code

`GET /oauth2/oob/code?app_id=<app_id>`

```json
{"status":"OK","code":"ABC123","qr_code_url":"https://api.put.io/v2/oauth2/oob/code/ABC123/qr"}
```

## Check a link code

`GET /oauth2/oob/code/<code>`

```json
{"status":"OK","oauth_token":null}
{"status":"OK","oauth_token":"<token>"}
```

- `null` → pending; a string → linked. 4xx → treat as expired/invalid.
- putio-sync checks at most every 3 s and gives up after 10 minutes.
- The token is stored in the setup session only; it is never logged or returned to the browser.

## Account check

`GET /account/info` with `Authorization: token <token>` → `info.username`, `info.disk.{size,used,avail}`.
401 → "credentials rejected".
