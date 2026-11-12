# Tasks: Sign-in options

**Input**: `specs/019-sign-in-options/` (plan.md, spec.md, research.md, data-model.md, contracts/)

**Tests**: requested by the spec (SC-001 to SC-003).

## Phase 1: Setup

- [x] T001 Add `argon2`, `hmac` and `sha2` to putio-sync-api/Cargo.toml

## Phase 2: Foundational

- [x] T002 putio-sync-core/src/sign_in.rs: `SignInConfig` (validate, masked/unmasked, `allows`, `redirect_url`), with unit tests; wire into config.rs (field, validate, secret_values)
- [x] T003 putio-sync-api/src/signin/: session key (file or memory), cookie sign and check, CSRF token, password hash and verify, Basic cache, pending OAuth flows

## Phase 3: US1 Password and Basic (P1)

- [x] T004 [US1] middleware/auth.rs: Basic and session paths after the token and before Tailscale; CSRF on cookie writes; failures counted
- [x] T005 [US1] routes/system/auth.rs: `/auth/status` `sign_in` and `user`; `POST /auth/password`; `POST /auth/logout`
- [x] T006 [US1] routes/system/signin.rs: `GET|PUT /settings/sign-in`, `POST /settings/sign-in/sign-out-everywhere`

## Phase 4: US2 OAuth (P1)

- [x] T007 [US2] signin/oauth.rs: providers, authorize URL, code exchange, user info (GitHub's verified emails), with an endpoint override for tests
- [x] T008 [US2] routes: `GET /auth/oauth/:provider` and `/callback`

## Phase 5: US3 and US4

- [x] T009 [US3] tests/sign_in.rs: every path and refusal; existing auth, arr and transmission tests unchanged
- [x] T010 [US4] putio-sync-doctor/src/checks/sign_in.rs, `CheckId::SignIn`, with tests

## Phase 6: UI

- [x] T011 pages/login.js becomes a loader; pages/signin.js holds the token form, password form and provider buttons; api.js sends `X-CSRF-Token`, signs out with `/auth/logout`, and treats a session as signed in; the top bar shows who and how
- [x] T012 settings/signin.js (lazy section "Sign-in") and ui/signin.js pure helpers with tests/ui/signin.test.mjs; register assets in src/ui.rs; ui_assets passes
- [x] T013 Check the sign-in page and Settings › Sign-in in a browser against a local server

## Phase 7: Docs and polish

- [x] T014 README (API table, sign-in section), config.example.toml `[sign_in]`, CHANGELOG (Unreleased), ROADMAP 019 done
- [x] T015 `make i18n`, then `make i18n-translate`
- [x] T016 Full test run with `--no-fail-fast` and `bash scripts/ci-local.sh`
