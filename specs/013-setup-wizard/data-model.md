# Data Model: First-Run Setup Wizard

## Config additions (`putio-sync-core::config`)

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `[putio] app_id` | string, optional | unset | Public Put.io app ID for the device link. Env `PUTIO_SYNC_APP_ID` next, then the built-in (build-time `option_env!`). |
| `[download] preset` | string, optional | unset | Account-wide preset. Unset = `blocked_extensions` at the root (today's behaviour). Must name a built-in or `[download.presets]` entry. |

## SetupReason (core)

Why the daemon is in setup mode. Serialized snake_case.

| Variant | Meaning |
|---------|---------|
| `no_config` | config file does not exist |
| `invalid_config { message }` | file exists but does not parse/validate (message has no secrets) |
| `no_credentials` | no Put.io token in file or env |
| `credentials_rejected` | Put.io answered 401 to the startup account check |
| `folder_missing { path }` | target folder does not exist and cannot be created |
| `folder_not_writable { path }` | exists but the probe write fails |

`SetupCheck { reasons: Vec<SetupReason> }` — empty = no setup needed.
`core::setup::check_local(path, Option<&Config>)` covers everything except `credentials_rejected`
(needs the network; the CLI adds it after an account call).

## FolderCheck (core)

```text
FolderCheck {
  path: String,
  exists: bool, created: bool, is_dir: bool, writable: bool,
  can_create: bool,               // missing, but nearest existing parent is writable
  free_bytes: Option<u64>, total_bytes: Option<u64>,
  warnings: Vec<FolderWarning>,   // less_than_putio_used{used}, low_space{free}, container_path_hint
  error: Option<String>,          // why it is unusable
}
usable() = (is_dir && writable) || (!exists && can_create)   // created on finish
```

## SetupChoices (core)

What the wizard / CLI collected; applied to a `Config` in one step.

```text
SetupChoices {
  putio_token: Option<String>,        // never serialized out
  download_dir: Option<PathBuf>,
  preset: Option<String>,             // account-wide preset
  folder_rules: Vec<FolderRule>,      // per top-level folder (path "/Name", preset)
  watch_dir: Option<PathBuf>,         // Some = enable watch with this path
  metrics: Option<bool>,
  metrics_bind: Option<String>,
  api_token: Option<String>,          // set when creating one
}
apply(&self, &mut Config)  // keeps every other setting; replaces only rules for chosen top-level folders
```

Validation: download dir must pass `FolderCheck::usable()`; preset name must resolve; folder rule
paths are top-level (`/Name`); watch dir must be usable when set.

## SetupSession (api, in memory)

```text
SetupSession {
  code: String,                       // one-time setup code (never logged via tracing)
  base: Config,                       // loaded config, or Config::default()
  config_path: PathBuf,
  reasons: Vec<SetupReason>,
  choices: SetupChoices,
  account: Option<AccountSummary>,    // username, used/size bytes (no email)
  link: Option<LinkState>,            // { code, qr_code_url, created_at } for the device link
  arr: Option<ArrSettings>,           // draft for 018 auto-setup, enabled + generated password
  dry_run: Option<DryRunReport>,
  finished: bool,
}
```

Lifecycle: created when setup mode starts → mutated by `/setup/*` → `finish` validates, applies,
saves (`Config::save`, 0600), sets `finished`, fires shutdown → dropped. Restarting setup mode
creates a new session with a new code.

## LinkState / device link (client)

```text
OobCode { code: String, qr_code_url: Option<String> }
OobCheck::Pending | OobCheck::Linked(token)
```
Expiry: the UI treats a code older than 10 minutes as expired.

## DryRunReport (download)

```text
DryRunReport {
  files_seen: u64,
  to_download: { count, bytes },
  already_present: { count, bytes },
  skipped: { count, bytes },          // by rules/presets
  samples: Vec<DryRunItem>,           // 20 largest to download: path, size
  partial: bool,                      // hit the 60 s deadline
  elapsed_ms: u64,
}
```

## State transitions (daemon)

```text
start → check ─ none → normal startup
           └─ reasons → SETUP MODE (gauge=1, print code)
                 ├─ /setup/finish saved → shutdown setup server → reload config → check
                 ├─ config poll (5 s) passes check → same as above
                 └─ api listen=false → print message, poll only
       normal startup (gauge=0)
```
