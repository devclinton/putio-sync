# Research: Radarr/Sonarr Settings and Auto-Setup

Decisions for [plan.md](plan.md). Each has the decision, why, and what else was considered. The
*arr API calls are listed in [contracts/arr-api.md](contracts/arr-api.md).

## R1. Writing `[arr]` from the UI

**Decision**: Add a dedicated writer in `putio-sync-core`:
`Config::save_arr_section(path, &ArrConfig)`. It replaces the `[arr]` table (and its
`[[arr.servers]]` array) in the `toml_edit` document and leaves every other table, comment and
secret as on disk. The handler builds the new `ArrConfig` from the file-backed config in memory
(`AppState.config`, which holds file values, not env overrides). It merges in secrets the
request left empty: the current password, and each application's current API key. The generic
`save_keeping_file_credentials` path keeps dropping `arr` from `settings_table()`, so the
existing Settings forms still never touch `[arr]`. Only the comment there changes.

**Rationale**: `settings_table()` drops `arr` on purpose so the generic save can't clobber
secrets it doesn't show. A separate writer that only knows `[arr]` keeps that guarantee for
every other form. Merging secrets in the handler matches how the Put.io token form works.

**Alternatives**: Let `[arr]` through the generic writer. Rejected: that writer serializes the
in-memory view, which has redacted secrets. Another option was a separate `arr.toml` file.
Rejected: that's a second file to mount and back up, and it breaks the single-config model in
the constitution.

## R2. Applying changes without a restart

**Decision**:

- Build `ArrService` whenever a database exists, not only when `[arr].enabled` is true.
- Store its settings in `std::sync::RwLock<Arc<ArrSettings>>` and add `ArrService::apply(settings)`.
- `transmission_rpc` reads `enabled` and `password` on each request. It returns 404 when off,
  as today, and 503 with no password, as today.
- The settings handlers call `apply` after a successful write, in the same place
  `settings::update` swaps the config.
- A config hot reload from file goes through the same path.

**Rationale**: The service has no background work of its own (the RPC drives `refresh_if_stale`).
Building it while disabled costs nothing. Turning the client off doesn't touch tracked items,
which matches the spec edge case and feature 011.

**Alternatives**: Rebuild `AppState.arr` on change, which would mean `AppState` needs an
`RwLock<Option<…>>`. Rejected: every RPC would take a lock, and in-flight refreshes would hold
a stale service. A restart prompt was the other option. Rejected: SC-003 forbids it.

## R3. Config shape stays compatible

**Decision**: Keep `[[arr.servers]]` with `kind`, `url` and `api_key`.

- Add an optional `name`. It defaults to "Radarr"/"Sonarr", plus " (2)" when that name is taken.
- `category` becomes `Option<String>` and is ignored. It's kept only for the FR-015 warning.
- Validation drops the category rules. It requires a parseable http(s) URL, and no two servers
  may share a normalized URL.
- The form's host, port, SSL and URL base are joined into `url` on the server (`reqwest::Url`)
  and split back for display.

**Rationale**: Old configs keep loading (FR-015). One `url` field is what feature 011 already
parses, tests and overrides with env.

**Alternatives**: Store host, port, ssl and url_base as separate keys. Rejected: that's a
migration for no gain.

## R4. Reading the category from the *arr

**Decision**:

- `ArrService` keeps an in-memory map from the server URL to `ClientInfo`. `ClientInfo` holds
  the categories, `client_present`, `read_at` and `last_error`.
- Fill it by calling `GET /api/v3/downloadclient` on each application. Keep the clients that
  "point at putio-sync" (R6) and read `movieCategory` (Radarr) or `tvCategory` (Sonarr).
- When to read:
  - when an application is saved or tested;
  - after a setup;
  - at startup;
  - every 15 minutes, in a task spawned with the service (`ArrService::spawn_category_refresh`,
    in `putio-sync-arr`).
- A failed read keeps the last value and records the error and its age, which the UI shows.
- `blocklist_server(category)` picks the server whose cached categories contain the category.
  Without a cache hit it falls back to a legacy typed `category`, then gives up and logs it,
  as today.

**Rationale**: Blocklisting must pick the right *arr (FR-014, SC-004). A 15-minute cadence is
cheap: one GET per application. It catches a category changed inside the *arr long before it
matters, since failures take minutes to hours to surface.

**Alternatives**: Read the category on demand at blocklist time. Rejected: if the *arr is down
the failure has no target, and an operator can't see the category before something fails.
Persisting the cache in SQLite was rejected as not needed (a restart re-reads it at startup).

## R5. Which host the *arr should use (FR-022)

**Decision**: Build candidate hosts, in this order, with duplicates dropped:

1. The literal `putio-sync`. This is the usual container name, and Docker's DNS resolves it on a
   user-defined network.
2. The container hostname from `/etc/hostname` or `HOSTNAME`. Docker's DNS also resolves the
   short container ID.
3. The host the browser used, from the `Host` header of the setup request, without its port.
4. The Docker gateway, read from `/proc/net/route` (the default route's gateway). It's the NAS
   when the port is published.
5. The machine's primary address. This is the `local_addr()` of a UDP socket "connected" to
   `192.0.2.1`; no packet is sent.

The operator's override, if given, replaces the list.

For each candidate, build the full client body and call the *arr's `POST /downloadclient/test`.
The first pass wins. Each test has a timeout of about 5 s, and setup stops after 5 candidates.

**Rationale**:

- The *arr's own test is the only reliable way to know what the *arr can reach from its
  network namespace.
- Testing never creates anything, so trying several hosts leaves nothing behind.
- All of this uses `std`, so there's no new dependency.

**Alternatives**: The `if-addrs` or `local-ip-address` crates were rejected: a new dependency
for something `/proc/net/route` and the UDP trick already cover. Using the browser host only was
rejected because it fails behind a reverse proxy.

## R6. Recognising putio-sync's own client in an *arr

**Decision**: A download client "points at putio-sync" when all of these hold:

- `implementation == "Transmission"`;
- `urlBase`, normalized with slashes, is `/transmission/`;
- `port` is putio-sync's API port;
- either `host` is one of the R5 candidate hosts, or `name` starts with `putio-sync`.

When setup runs, the first matching client is updated in place. A client named `putio-sync`
that doesn't match isn't touched, and setup uses `putio-sync (2)` and so on (US5 #2).

**Rationale**: The URL base and port are specific to putio-sync, and the name catches an earlier
auto-setup whose host was later changed.

## R7. Test before write, so there's almost nothing to roll back (FR-023)

**Decision**:

- Setup always calls `POST /downloadclient/test` with the final body before any write.
- It creates the client with `POST /downloadclient`, or updates it with `PUT /downloadclient/{id}`.
  Neither call uses `forceSave`, so the *arr re-tests it.
- If that write fails after a passing test (rare: the *arr went away, or a race), and it was a
  create, setup deletes any client it created. If it was an update, setup PUTs back the body it
  fetched before (with `forceSave=true`).
- Torrent Blackhole is disabled only after putio-sync's client is saved. That's a GET of its body
  with `enable=false`, then a PUT with `forceSave=true`. The blackhole's own folder test isn't
  putio-sync's problem.

**Rationale**: Testing first means a failed setup never leaves a failing client (SC-006),
without depending on rollback.

**Verified** (Radarr 6.4.4, 2026-10-29, quickstart step 6): a fetched body has the password
masked as `********`. A PUT of that body with only the category changed returned 202, and the
client's Test passed afterwards, so Servarr keeps the stored password for the masked value. The
restore path can send the fetched body as is.

## R8. "Already set up" (US5 #6)

**Decision**: Setup is "already set up" when the matching client is enabled, its host, port,
SSL, URL base, username, category and priority equal the new body, and the *arr's test of the
saved client passes. The password can't be compared because it's masked, but a passing test
proves it. Setup then makes no write.

## R9. Discovery (FR-017)

**Decision**:

- **Candidates**: each saved application's address, plus the hosts `radarr`, `sonarr`, the R5
  gateway, `localhost`, and the browser host. Each of these hosts is tried on ports 7878 and 8989,
  over http.
- **Probe**: run all candidates at once in a `tokio::task::JoinSet` with a 3 s timeout each and
  a 10 s overall deadline. Each probe sends `GET {base}/ping`, which needs no key and returns
  `{"status":"OK"}`. If that passes, it sends `GET {base}/` and reads the HTML `<title>`
  (`Radarr` or `Sonarr`) to get the kind. `initialize.json` is never read, because it carries
  the API key.
- **One instance at two addresses**:
  - A hit from an earlier tier (saved application, then container name, then the host
    addresses) hides later hits of the same kind on the same port.
  - This covers the NAS case, where `radarr:7878` and `gateway:7878` are the same Radarr.
  - Two genuinely separate instances of the same kind and port on different hosts would be
    merged. Manual entry covers that rare case.
- **Concurrency**: a `tokio::sync::Mutex` held for the run gives one discovery at a time. A
  second request gets 409.
- **Result**: the version is filled in later, once a key is entered (`system/status`).

**Rationale**: The ping and title endpoints need no key and don't expose secrets. A bounded
list meets "no sweep". A real instance identity needs the key, so dedupe works from where each
hit was found.

**Alternatives**: mDNS, the Docker socket and a subnet scan were rejected, because the spec
puts them out of scope.

## R10. Path-mapping check (FR-025)

**Decision**:

1. Fetch `GET /api/v3/remotepathmapping`. A mapping whose `host` equals the client's host and
   whose `remotePath` is a prefix of the download folder rewrites the folder to its `localPath`.
2. List the parent: `GET /api/v3/filesystem?path={parent}&allowFoldersWithoutTrailingSlashes=true`.
3. Check that the folder's name is among `directories[].name`.

The check returns one of `visible`, `not_visible` (with the mapped path and the mappings found)
or `unknown` (with the error). It runs on Test and after setup.

**Rationale**: Listing a missing folder directly gives an ambiguous empty answer. Listing the
parent and looking for the child doesn't.

## R11. Passwords

**Decision**:

- **Form Generate**: client-side, `crypto.getRandomValues`, 32 characters from a 62-character
  alphabet. The browser sends it with Save, like any typed password.
- **Auto-setup**: server-side, `Uuid::new_v4().simple()` twice and joined: 64 hex characters,
  244 random bits. The api crate already depends on `uuid` with v4, which draws from the OS
  generator. It's never put in a response (FR-023a).

**Rationale**: No new dependency (`rand` is only transitive today). Both methods use the OS
CSPRNG.

## R12. Minimum *arr version

**Decision**: Require `system/status.version` major ≥ 3 for Radarr and Sonarr, and use
`/api/v3/` for every call. Anything older gets `unsupported_version` with the version found and
"3.0".

## R13. Metrics

**Decision**: Add one counter, `putio_sync_arr_setup_total{kind, result}`. `kind` is
radarr/sonarr. `result` is created, updated, unchanged or one of the FR-026 failure reasons, a
fixed set. Everything else is logged with `tracing` fields (kind, url, client_id, result), never
keys or passwords.

**Rationale**: Principle V. The labels are bounded.
