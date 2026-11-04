# Contract: Setup API (setup mode only)

Served on the normal API address while the daemon is in setup mode. JSON bodies; errors are
`{"error": "<message>"}` with 4xx/5xx.

**Auth**: every `/setup/*` route except `GET /setup/status` needs
`Authorization: Bearer <setup code | API token>`. Wrong/missing → 401 and counted by the auth
limiter (5 failures / 60 s / IP → 429).

## Public in setup mode

| Route | Response |
|-------|----------|
| `GET /health` | `{"status":"setup","version":"…"}` |
| `GET /auth/status` | `{"token_configured": bool, "setup_required": true}` (normal mode returns `setup_required: false`) |
| `GET /`, `GET /assets/*` | the web UI |
| `GET /setup/status` | `{"setup_required": true, "reasons": [SetupReason…], "api_token_configured": bool}` |
| anything else | 503 `{"error":"Setup needed","setup_required":true}` |

## Gated

| Route | Body | Response |
|-------|------|----------|
| `POST /setup/unlock` | – | 204 (validates the bearer only) |
| `GET /setup/session` | – | `{reasons, account?, download_dir?, preset?, folder_rules, watch_dir?, metrics?, arr_enabled, app_id_available, in_container, suggestions: {download_dir, watch_dir?}, presets: [{name,label}]}` (never tokens) |
| `POST /setup/putio/link` | – | `{"code","qr_code_url"?,"link_url":"https://put.io/link"}`; 409 when no app ID |
| `GET /setup/putio/link` | – | `{"state":"pending"}` or `{"state":"linked","account":{username,used,size}}` (token stored in session) or `{"state":"expired"}` |
| `POST /setup/putio/token` | `{"token"}` | `{"account":{…}}`; 400 when Put.io rejects |
| `GET /setup/putio/folders` | – | `[{"name","path":"/Name","size"}]` top-level folders (needs a linked account) |
| `POST /setup/folder/check` | `{"path","create"?:bool}` | `FolderCheck` |
| `PUT /setup/folder` | `{"path"}` | `FolderCheck`; 400 if not usable (stored only when usable) |
| `PUT /setup/download` | `{"preset", "folder_rules":[{"path","preset"}]}` | 204; 400 unknown preset / bad path |
| `POST /setup/watch/check` | `{"path","create"?}` | `FolderCheck` |
| `PUT /setup/extras` | `{"watch_dir"?:string\|null, "metrics"?:bool, "metrics_bind"?:string}` | 204 |
| `POST /setup/arr/discover` | – | same shape as `POST /api/arr/discover` (018) |
| `POST /setup/arr/preview` | as 018 setup preview | as 018 |
| `POST /setup/arr/run` | as 018 setup run | as 018; changes apply in Radarr/Sonarr at once and mark arr enabled in the session |
| `POST /setup/dry-run` | – | `DryRunReport` (needs account + folder) |
| `POST /setup/finish` | – | `{"api_token"?: "…"}` — present only when this call created it; config written once; setup mode then ends. 400 lists what is missing (`account`, `download_dir`). |

Secrets: the Put.io token is accepted but never returned; the API token is returned once by
`finish`; the setup code is never returned.
