# Contract: Settings assistant API

All routes are under `/settings`, need `Authorization: Bearer <API token>` like the other
settings routes, and return JSON errors `{ "error": "..." }`. Groups: `download`, `watcher`,
`logging`, `notifications`, `cleanup`, `precheck`.

## POST /settings/{group}/preview

Body: exactly the PUT body of that group. Never writes, never changes the running config.

- 200 `{ valid, error?, changes, when, risks }` (see data-model "Preview"). A value the save
  would refuse is `valid: false` with the same `error` text the PUT returns as 400.
- 400 when the body is malformed (same as PUT).

Example: `{"valid":true,"changes":[{"key":"workers","before":4,"after":8,"live":true}],"when":"Applies from the next download","risks":[]}`

## GET /settings/assistant

200 `{ groups: { <group>: { affects, live, when, defaults } }, risks }`.
`defaults` for `precheck` holds the settings only (no `effective`).

## GET /settings/export

200 `text/plain; charset=utf-8` with `Content-Disposition: attachment;
filename="putio-sync-settings.toml"`. Every setting except `[auth]`, `[arr]` and
`api.auth_token`, with a header comment saying so.

## POST /settings/import

Body: `{ "toml": "<text>", "apply": false }`. Over 1 MB → 400.

- `apply: false` → 200 Preview with `changes` (dotted keys), `risks`, `unknown`, `ignored`.
  An import the config would refuse is `valid: false` with `error`.
- `apply: true` → validates again; when valid, writes the file (comment-preserving, file
  credentials and `[arr]` kept), updates the running config and returns 200 Preview with
  `message`, e.g. "Imported 3 changes. Needs a restart: download.target_directory." When not
  valid → 400 with the reason; nothing written.
- Invalid TOML → 400 `"The file is not valid TOML (line N): <message>"`, never quoting the
  line.
