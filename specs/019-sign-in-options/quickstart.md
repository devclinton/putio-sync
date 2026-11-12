# Quickstart: Sign-in options

## Automated

```bash
CARGO_INCREMENTAL=0 cargo test -p putio-sync-api --test sign_in --test auth --test arr_rpc
```

```bash
CARGO_INCREMENTAL=0 cargo test -p putio-sync-core sign_in
```

```bash
CARGO_INCREMENTAL=0 cargo test -p putio-sync-doctor sign_in
```

```bash
node --test putio-sync-api/tests/ui/signin.test.mjs
```

`tests/sign_in.rs` covers:

- Basic auth;
- the password form, its session, CSRF, and that a password change ends it;
- the rate limit;
- OAuth for Google, GitHub and Discord against mockito: the authorize redirect (PKCE and
  state), the callback, an account that isn't listed, an unverified email, a bad or reused
  state, and the wrong browser;
- the masked secrets;
- sign out everywhere;
- token requests unchanged.

## By hand

1. Run `server` and open Settings › Sign-in. Set a username and password, and save.
2. Sign out, then sign in with the password form. Writes in Settings work.
3. Run `curl -u user:pass http://127.0.0.1:15432/status`. It answers 200.
4. Register an OAuth app with a provider, using the redirect URL that Settings shows. Enter
   its ID and secret, list your email, and save. Sign out, then press **Sign in with …**.

Step 4 needs a real provider account.
