# Research: Log Viewer in the UI (047)

## How logging is set up today (checked 2026-11-05)

- `putio_sync_core::logging::init_logging` builds a `Registry` whose first layer is
  `reload::Layer<EnvFilter>`, followed by the file and console fmt layers. The handle lives
  in `FILTER_HANDLE`, and the directive in effect is in `CURRENT_DIRECTIVE`.
- `apply_level(&LoggingConfig)` is the only writer of the handle. The config reload poller
  (`config::spawn_reload_poller`, every 10 s, `daemon` and `server`) calls it when the file
  changes. It does nothing while `RUST_LOG` is set.
- `RUST_LOG` replaces the config directive at startup (`LevelSource::Env`).
  `PUTIO_SYNC_LOG_FORMAT` overrides the console format.
- Debug log file: `[logging] file_enabled`/`file_path`, with daily rotation. The config lint
  (`assistant.rs`, `verbose_log_file`) warns when the file is on at debug or trace. That
  warning is about disk use; the new doctor check is about the level in effect.
- `doctor` and `setup` use `init_stderr`, not `init_logging`, so they never capture.
- 041 telemetry: `putio-sync-core` depends on `putio-sync-telemetry`, so core can count
  metrics directly through `putio_sync_telemetry::global()`.
- The UI signs in with a bearer token from `localStorage` (`api.js`). It does not use SSE or
  WebSocket anywhere today.

## R1. Where lines are captured

**Decision**: a `tracing_subscriber::Layer` (`CaptureLayer`) added right after the reload
filter in every branch of `init_logging`.

**Rationale**: a layer behind the global filter only sees enabled events, which gives FR-003
(debug and trace kept only while enabled) for free. Every output combination already shares
the filter, and the capture layer works the same with any of them.

**Alternatives**: tailing the log file (needs the file to be on, and gives no structure);
a per-layer filter with its own level (would show lines the console doesn't, which breaks the
"what is logged" model).

## R2. Buffer and live fan-out

**Decision**: a `std::sync::Mutex<VecDeque<Arc<LogLine>>>` with a capacity, plus a
`tokio::sync::broadcast::Sender<Arc<LogLine>>` (capacity 1024).
- Each line gets a `seq` from a `u64` counter.
- Push: lock, pop the front if full (count `evicted`), push, unlock, then
  `send` (non-blocking; with no receivers it errors, which is ignored).

**Rationale**: logging must never await (FR-004). A short std mutex hold is cheaper than any
async structure. Broadcast gives each viewer its own cursor, and a slow viewer only lags
itself.

**Lag handling**: when a receiver gets `Lagged(n)`, the stream backfills from the buffer every
line after the last seq it sent. Lines no longer in the buffer are reported in one `skipped`
event and counted as `lagged`. A reconnect with `after=<seq>` does the same.

**Alternatives**: a polling-only endpoint. It is simpler, but the spec asks for live tail, and
SSE costs little.

## R3. Redaction without a regex crate

**Decision**: `log_redact::redact(&str) -> String` runs three passes over the message and every
field value. Field names are checked too.
1. **Known values**: every configured secret of 6 or more characters (FR-007, the same
   minimum as the hooks), longest first, is replaced by `********`. The list sits in an
   `RwLock<Arc<Vec<String>>>` set by `log_redact::set_secrets`.
2. **Patterns**: a hand-written scanner finds `=` and `:` separators. If the identifier
   before one (optionally quoted) contains `token`, `secret`, `password`, `passwd`,
   `api_key`, `apikey`, `authorization`, `cookie`, `signature` or `credential` (ignoring
   case), the value after it is masked: a quoted string, `Some("…")`, or a run up to
   whitespace or `,;&)"'`. A value of `Bearer`/`Basic`/`Token` takes the next word too.
   Separately, `Bearer <x>` and `Basic <x>` anywhere become `Bearer ********`.
3. **URLs**: `redact::text`, the existing shortener, drops user info, the query and deep
   paths.

A field whose own name is sensitive is masked whole, unless its value is `true`, `false` or
empty.

**Rationale**: known values catch secrets in any shape (a token echoed in an error). The
patterns catch secrets that aren't configured (an `Authorization` header logged by a library
at trace). URL shortening matches what 027 already does for webhook URLs. All three are
cheap. Principle VII argues against adding `regex` for this.

**Secret sources** (`Config::secret_values`):
- `auth.token`, `auth.password`, `api.auth_token`, `arr.password`, each `arr.servers[].api_key`;
- `notifications.webhook_url`, both whole and as its URL path and query parts;
- hook `url`s (whole), hook header and env values, plugin `secrets` values;
- the environment values of `PUTIO_TOKEN`, `PUTIO_SYNC_PUTIO_TOKEN`, `PUTIO_SYNC_API_TOKEN`,
  `PUTIO_SYNC_ARR_PASSWORD`, `PUTIO_SYNC_RADARR_API_KEY`, `PUTIO_SYNC_SONARR_API_KEY` and
  `PUTIO_CLIENT_SECRET` (the Put.io app secret).

They are set in `main` before `init_logging`, again in `AppState::new`, and again after every
settings save or import.

## R4. Level overrides

**Decision**: a `LevelState { base, source, global: Option<Override>, modules: BTreeMap<String,
Override> }` sits behind a mutex next to `FILTER_HANDLE`. The effective directive is built as:
- the overall level: `global`, or else the bare level items of `base`;
- the base's `target=level` items, minus targets that have a page override;
- the page's module overrides.

The result is parsed into a new `EnvFilter` and swapped in through the existing handle.
`apply_level` (the poller) now replaces `base` and recomputes. It still returns `IgnoredEnv`
under `RUST_LOG`.

Page overrides apply on top of `RUST_LOG` (clarified in the spec).

Expiry: `set_override` spawns a tokio task that sleeps until the end and then calls
`expire(now)`. That call drops every override whose end has passed and re-applies, so a
replaced or cleared override's timer does nothing harmful. Tests call
`LevelState::expire(now)` directly.

Validation: the level is one of off, error, warn, info, debug, trace; the target matches
`[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*`; minutes are 1 to 1,440; at most 20
module overrides.

**Alternatives**: writing to the config file. Rejected: "keep" is clarified as until restart,
and Settings already writes the file.

## R5. SSE with header auth

**Decision**: `GET /logs/stream` returns `axum::response::Sse` over a
`futures_util::stream::unfold` built on the broadcast receiver, with `KeepAlive` every 15 s.
The page reads it with `fetch()` and an `Authorization` header, then parses `event:`/`data:`
lines itself (about 30 lines of JS), because `EventSource` cannot send headers.

**Alternatives**: a `?token=` query parameter for `EventSource`. Rejected: it puts the token in
URLs and logs, which is the very thing this feature must not do. WebSocket: the UI doesn't use
one, and it needs more code on both sides.

## R6. Links from lines

There are no per-item detail pages. Links go to the closest existing view:

| Field | Link |
|-------|------|
| `file_id` | `https://app.put.io/files/<id>` (new tab) |
| `folder_id`, `parent_id` | `#/putio/<id>` |
| `transfer_id` | `#/transfers` |
| `hook` | `#/settings` (Event hooks) |
| `plugin` | `#/settings` (Plugins) |
| `run_id` (verify) | `#/history` (Verification panel) |

Only fields whose values are digits (the ids) or match the hook/plugin name rule get a link.

## R7. Bundle budget

The UI is 378,001 bytes of the 393,216-byte cap (`tests/ui_assets.rs`). Budget: `pages/logs.js`
≤ 8 KB, CSS ≤ 1 KB, nav and route a few bytes. Write terse code and keep comments short; no
helper module.

## R8. Doctor and metrics

- `CheckId::LogLevel` ("log_level", "Log level", area Local).
- While logging is set up in this process, the check reads `logging::level_status()`.
  Otherwise (CLI `doctor`) it builds the status from `ctx.config.logging` and `RUST_LOG`.
- A warning when any directive is debug or trace, with a hint per source: "Revert it on the
  Logs page", "Set [logging] level", or "Unset RUST_LOG".
- `putio_sync_log_lines_dropped_total{reason="evicted"|"lagged"}`, created at zero in both
  labels.
