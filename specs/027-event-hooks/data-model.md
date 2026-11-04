# Data Model: Event Hooks

## Event (core, `events.rs`)

- `Event { schema_version: u32, id: String, occurred_at: DateTime<Utc>, source: Source,
  payload: Payload }`, where `payload` is `#[serde(flatten)]` of
  `#[serde(tag = "type", content = "data")] enum Payload`, so the wire form is
  `{…, "type": "download.finished", "data": {…}}`.
- `EventType`: closed enum with `as_str()`, `ALL`, `FromStr`; `Payload::event_type()`.
- `Source { app: &'static str ("putio-sync"), version, host }`.
- Payload structs: `DownloadFinished`, `DownloadFailed`, `TransferChanged` (shared by the
  three transfer types), `SyncCompleted`, `StorageLow`, `HookTest`. Fields as in
  [contracts/events.md](contracts/events.md).
- Bus: `emit(Payload) -> Arc<Event>` (stamps envelope, counts
  `putio_sync_events_total{type}`, sends), `subscribe() -> broadcast::Receiver<Arc<Event>>`.

## HookConfig (core, `hooks.rs`)

- `name: String`, `enabled: bool` (true), `events: Vec<String>`, `command:
  Option<Vec<String>>`, `url: Option<String>`, `headers: BTreeMap<String, String>`,
  `env: BTreeMap<String, String>`, `timeout_secs: u64` (30).
- `HookConfig::validate(index)`, `matches(EventType)`, `kind() -> HookKind {Command,
  Url}`, `target_label()` (redacted URL or program path).
- `MASK = "********"`, `mask(&[HookConfig])`, `unmask(new, current) -> Result`.

## Hook run (hooks crate)

- `HookRun { event_type, event_id, started_at, duration_ms, result: RunResult {Success,
  Failure, Timeout}, attempts, exit_code: Option<i32>, http_status: Option<u16>, error:
  Option<String>, stdout: String, stderr: String }`, with `stdout`/`stderr` cut to 4 KiB
  plus a `… (N bytes cut)` note.
- `HookStatus { name, enabled, kind, target, events, timeout_secs, queued, runs }`.

## Transfer tracker (download, `transfer_events.rs`)

- `TransferClass { Active, Finished, Failed }` from the Put.io status.
- `TransferEvents { seen: HashMap<i64, TransferClass>, baseline_taken: bool }`;
  `observe(&[Transfer]) -> Vec<(EventType, &Transfer)>` is pure; persistence is the
  caller's (`daemon_flags.transfer_events_seen`, JSON `{id: "a"|"f"|"e"}`).

## Storage low edge (storage)

- `StorageService.low: AtomicBool`; `refresh` returns the sizes as before and emits on
  `false → true`.

## Metrics (telemetry, `hooks.rs`)

| Family | Type | Labels |
|--------|------|--------|
| `putio_sync_events_total` | counter | `type` (closed) |
| `putio_sync_hook_runs_total` | counter | `hook`, `event`, `result` |
| `putio_sync_hook_run_duration_seconds` | histogram | `hook` |
| `putio_sync_hook_events_dropped_total` | counter | `hook` |
