# Implementation Plan: Sign-in options

**Feature**: `specs/019-sign-in-options` | **Date**: 2026-11-11 | **Spec**: [spec.md](spec.md)

## Summary

Add password sign-in (form and HTTP Basic) and OAuth sign-in (Google, GitHub, Discord) to
the API and web UI, next to the API token and Tailscale identity.

- Sessions are HMAC-signed cookies, checked against the config on every request. CSRF for
  cookie writes comes from a per-session token.
- The OAuth flow keeps its pending state in memory.
- Config lives in a new `[sign_in]` section in core, with masking and validation there.
- The doctor gets a config-only check.
- The UI's sign-in page body and the new Settings › Sign-in section both load with
  `import()`.

## Technical Context

- **Language**: Rust (workspace, edition 2021). The UI is vanilla ES modules.
- **New crates**: `argon2` 0.5 (RustCrypto, pure Rust), `hmac` 0.12 and `sha2` 0.10
  (already in the lock). Random values come from `argon2::password_hash::rand_core::OsRng`.
- **HTTP to providers**: the workspace's `reqwest` (rustls).
- **Testing**: `cargo test`, with OAuth providers faked by mockito via an AppState endpoint
  override. UI helpers are tested with node.
- **Constraints**: the static UI budget is 499,712 B with 124 B left, and none of the
  existing auth paths may change.

## Constitution Check

The plan stays inside the constitution's rules:

- **No new services.** Everything runs in the existing daemon, with no new database tables.
- **Secrets are masked and redacted**, like 030.
- **Every path has tests.** Each sign-in path and refusal is tested (SC-001, SC-003).
- **Docs ship with the change**: CHANGELOG, README, config.example.toml and ROADMAP.

The design was re-checked after Phase 1 and nothing changed.

## Project Structure

```text
putio-sync-core/src/sign_in.rs          [sign_in] config, validation, masking, allow-list match
putio-sync-core/src/config.rs           field + validate + secret_values
putio-sync-api/src/signin/mod.rs        SignIn state: session key, pending flows, Basic cache
putio-sync-api/src/signin/session.rs    cookie signing and checking, CSRF token
putio-sync-api/src/signin/oauth.rs      provider endpoints, authorize URL, code exchange, user info
putio-sync-api/src/middleware/auth.rs   Basic and session paths, CSRF check
putio-sync-api/src/routes/system/auth.rs  /auth/status additions, /auth/password, /auth/logout
putio-sync-api/src/routes/system/signin.rs  /auth/oauth/*, /settings/sign-in*
putio-sync-doctor/src/checks/sign_in.rs  the `sign_in` check
putio-sync-api/ui/pages/login.js        loader only; body in pages/signin.js (import())
putio-sync-api/ui/settings/signin.js    Settings › Sign-in (lazy)
putio-sync-api/ui/signin.js             pure helpers + node tests
putio-sync-api/tests/sign_in.rs         every path against a fake provider
```

## Complexity Tracking

| Choice | Why | Simpler option rejected |
|---|---|---|
| Signed cookies instead of a session table | Works without the database; survives restarts | In-memory sessions lose everyone at the weekly restart |
| A small cache of verified Basic credentials | argon2 per request would slow scripts | Dropping Basic auth goes against the brief |
