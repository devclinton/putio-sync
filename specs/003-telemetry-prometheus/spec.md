# Feature Specification: Telemetry and Prometheus Monitoring

**Feature Branch**: `003-telemetry-prometheus`

**Created**: 2026-10-25

**Status**: Draft

**Input**: User description: "Telemetry for putio-sync: add operational metrics useful for diagnosing the live NAS instance in future sessions (Put.io API requests/retries/errors, sync runs and durations, download queue depth, downloads completed/failed, bytes and throughput, blocked files, watch-folder scans/uploads/cleanups/failures, database errors, process uptime/build info). Expose them in Prometheus format from the running daemon/server. Verbosity of logging and telemetry must be adjustable via env variables or config. Connect it to the existing Prometheus in ~/dev/home-network (add a scrape job for the NAS putio-sync instance following that repo's conventions)."

## Context

- Feature 002 was diagnosed by reading about 338,000 log lines by hand over SSH. Each
  question, such as "how often does the first request fail?" or "how many files are
  stuck?", needed a new grep. This feature makes those answers a query or a graph.
- The constitution (Principle V, "Observable by Default") already requires Prometheus
  metrics for downloads, bytes, errors, queue depth, Put.io calls and latency, watcher
  activity, and disk space, with bounded labels and a configurable endpoint. None of it
  exists yet.
- The home network already runs Prometheus, Alertmanager, Grafana and Loki (managed in
  `~/dev/home-network`). Other apps there are scraped with a `job_name`, a
  `/metrics` path and `host`/`app` labels.
- The NAS container (`putio-sync` on nas01, 192.168.1.50) runs on the Docker bridge
  network and publishes no ports today.

## Clarifications

### Session 2026-10-25

- Q: When the log level is changed in the config file, should it take effect while the
  daemon is running, or only after a restart? → A: While running. The daemon re-reads the
  config file itself and switches level without a restart (kept at P2).
- Q: Should the metrics endpoint have an optional login setting, or always be open? → A:
  Always open, with no auth setting. Access is limited by where it is bound and published
  (the LAN).
- Q: What exactly should the `basic`/`detailed` metrics setting change? → A: `basic`
  (default) has one overall Put.io latency histogram and totals only. `detailed` adds Put.io
  latency per operation, plus per-worker gauges (state, bytes in flight) labelled by worker
  number.
- Q: What address should the metrics listener bind to by default? → A: All interfaces
  (`0.0.0.0:<port>`). Exposure is controlled by which port the container publishes.
- Q: When putio-sync runs as the API server instead of the daemon, how should it expose
  metrics? → A: Both modes start the same dedicated metrics listener on the metrics port.
  Nothing is added to the API port.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - The daemon reports its health as metrics (Priority: P1) 🎯 MVP

As the NAS operator (or an assistant in a future session), I want the running daemon to
publish metrics about its work, so I can see how it is doing without reading logs.

**Why this priority**: Everything else (scraping, alerts, dashboards) depends on the
daemon publishing metrics. On its own, it already turns a log dig into one HTTP request.

**Independent Test**: Run the daemon against a mock Put.io with a temp watch folder and a
temp download folder. Sync, queue, download and watch-upload one file each. Fetch the
metrics endpoint, and check that each metric family below is present and has the expected
value.

**Acceptance Scenarios**:

1. **Given** the daemon is running with default settings, **When** the metrics address is
   requested, **Then** it answers within 1 s in the standard Prometheus text format,
   without needing credentials.
2. **Given** a sync just ran, **When** metrics are read, **Then** they show:
   - the number of sync runs, split by outcome (success or failure);
   - the duration of the last sync;
   - when the last successful sync finished;
   - the files found, queued, skipped, blocked and failed in the last sync.
3. **Given** Put.io requests were made, **When** metrics are read, **Then** they show:
   - request counts by operation (for example list files, add transfer, delete) and by
     outcome class (success, client error, server error, network error). File content
     downloads are covered by the download metrics in scenario 4, not here;
   - retry counts by reason (network, server error, rate limited);
   - request latency as a distribution.
4. **Given** downloads have run, **When** metrics are read, **Then** they show:
   - the current queue depth by state (pending, downloading, failed);
   - active downloads (which is also the number of busy workers) and the configured
     worker count;
   - completed and failed downloads;
   - total bytes downloaded;
   - download duration as a distribution.
5. **Given** watch scans have run, **When** metrics are read, **Then** they show scans,
   files uploaded, already-uploaded files cleaned up, invalid files and cleanup failures,
   each as a running count.
6. **Given** the daemon is running, **When** metrics are read, **Then** they show:
   - the process start time;
   - build information (version and commit) as labels on a constant metric;
   - free and total disk space for the download folder;
   - database error counts by operation.
7. **Given** any metric, **When** its labels are examined, **Then** none contain a file
   name, file id, path, URL or token. The set of label values is fixed and small.

---

### User Story 2 - Home Prometheus scrapes the NAS instance (Priority: P1)

As the operator, I want the home Prometheus to collect the NAS daemon's metrics, so history
is kept and can be queried after the fact.

**Why this priority**: Metrics that nobody stores can't answer "what happened last week?",
which was the main question in feature 002.

**Independent Test**: In `~/dev/home-network`, the repo's own check (`make check`)
passes with the new scrape job. After the operator deploys, the Prometheus targets page
shows the putio-sync target as up.

**Acceptance Scenarios**:

1. **Given** the home-network repo, **When** the change is applied, **Then**:
   - the Prometheus config has a putio-sync scrape job for the NAS, with `host` and `app`
     labels in the same style as the other app jobs;
   - the repo's validation passes.
2. **Given** the NAS container publishes the metrics port, **When** Prometheus scrapes it,
   **Then** the target is up, and queries for sync runs and queue depth return data.
3. **Given** the home-network repo, **When** someone looks for how putio-sync is monitored,
   **Then** an app page (following the repo's app template) says:
   - where metrics are served;
   - which port the NAS must publish;
   - how to verify the target;
   - which queries answer the common questions (syncs failing, queue stuck, retries
     spiking, executables blocked).

---

### User Story 3 - Alerts for the failures we've seen (Priority: P2)

As the operator, I want to be told when putio-sync stops working, instead of finding out
weeks later from the logs.

**Why this priority**: It is the payoff of US1 and US2, but the system is still useful
without it.

**Independent Test**: The repo's alert-rule validation passes. Each rule's expression,
evaluated against synthetic series (for example, with no successful sync for 2 hours),
fires. It does not fire on healthy series.

**Acceptance Scenarios**:

1. **Given** the NAS target is down for 10 minutes, **Then** an alert fires.
2. **Given** no successful sync for more than 4× the configured sync interval (at least 30
   minutes), **Then** an alert fires.
3. **Given** pending queue items exist but no download has completed for 2 hours, while
   workers are active, **Then** a "queue stuck" alert fires.
4. **Given** Put.io retries exceed a threshold (default: 20 per hour, sustained for 1
   hour), **Then** a warning fires.
5. **Given** free disk space in the download folder drops below 10%, **Then** an alert
   fires.
6. **Given** database errors are greater than 0 over 15 minutes, **Then** a warning fires.

---

### User Story 4 - Verbosity is adjustable and visible (Priority: P2)

As the operator, I want to turn logging and telemetry detail up or down from config or
environment, so I can investigate without a rebuild and stay quiet the rest of the time.

**Why this priority**: Feature 002 showed that `RUST_LOG` was silently overriding the
config. Investigating needs a reliable knob.

**Independent Test**:
- Start the daemon with different combinations of config and environment settings, and
  check the effective log level and metrics settings in the startup log and on the metrics
  endpoint.
- Change the log level in the config file while the daemon runs, and check that the new
  level takes effect without a restart.

**Acceptance Scenarios**:

1. **Given** a log level in the config only, **Then** that level is used, and the startup
   log reports `source=Config`.
2. **Given** a log level in the environment, **Then** it overrides the config, and the
   startup log reports `source=Env`.
3. **Given** the log format is set to JSON (in config or environment), **Then** every log
   line is a single JSON object with the structured fields. This makes the logs usable by
   the existing Loki stack.
4. **Given** metrics are disabled in config or environment, **Then** no metrics listener
   is opened, and the startup log says so.
5. **Given** the metrics bind address or path is set in config or environment, **Then** the
   endpoint is served there. Environment wins over config, consistent with the log level.
6. **Given** the daemon is running and the config log level changes, **Then** the new level
   applies within 30 s, with no restart, signal or command. A log line records the old level, the new level and
   the source. If the environment set the level, the line says the config change is
   ignored.
7. **Given** metrics detail is set to "detailed", **Then** two extra sets are exposed:
   - Put.io request latency split by operation;
   - per-worker gauges for worker state (idle or downloading) and bytes in flight, labelled
     by worker number (0 up to the configured worker count minus 1).

   They still carry no names, ids or paths. With the default "basic", only the overall
   Put.io latency histogram is exposed, with no operation label and no per-worker series.
8. **Given** any setting above, **Then** it is documented in `config.example.toml` and the
   README, including the environment variable name.

---

### User Story 5 - A dashboard for a quick look (Priority: P3)

As the operator, I want one Grafana dashboard for putio-sync, so a glance answers "is it
healthy, and what has it done today?"

**Why this priority**: It's a convenience. Prometheus queries already answer the
questions.

**Independent Test**: The dashboard JSON is provisioned by the home-network Grafana role,
and the repo's validation passes. After a deploy, the dashboard loads with no panel errors
against live data.

**Acceptance Scenarios**:

1. **Given** the dashboard is opened, **Then** it shows:
   - target up/down;
   - last successful sync age;
   - sync outcomes over time;
   - queue depth by state;
   - download throughput and completions;
   - Put.io request rate, error rate, retry rate and latency;
   - watch uploads and cleanups;
   - blocked files;
   - disk free.

---

### Edge Cases

- **Metrics port already in use**: the daemon logs one error naming the address and keeps
  running without metrics. It does not exit. Downloads matter more than telemetry.
- **Metrics requested while a sync or download is running**: the response is not delayed
  by the work (under 1 s), and the values are consistent at the moment they are read.
- **Restart**: counters start again from 0. The start-time metric changes so rates are
  computed correctly across restarts. Gauges such as queue depth are recomputed from the
  database at startup, not left at 0 until the first sync.
- **Disk space can't be read** (for example, the folder is missing): the disk metrics are
  absent rather than reported as 0, and a single warning is logged.
- **API server mode and daemon mode**: both expose the same metric names, so dashboards
  work whichever mode the NAS runs. Both start the same dedicated metrics listener, and
  the API port does not serve metrics.
- **Invalid verbosity value** (for example `level = "loud"` or detail `"max"`): startup
  fails with a clear message naming the setting and its allowed values. Today's behaviour
  for an invalid log level is kept.
- **Very long uptime**: counters do not overflow or reset in practice (months of operation
  at NAS rates).
- **Config file edited into an invalid state while running**: the daemon keeps its current
  log level and other behaviour, and logs one error. It does not crash or fall back to
  defaults.
- **Metrics disabled**: no listener is opened and nothing is served. Instrumentation
  still updates in-memory counters, which costs a few atomic operations per event. That
  overhead is negligible and is covered by the SC-002 throughput check. No separate no-op
  path is added.

## Requirements *(mandatory)*

### Functional Requirements

**Metrics content**

- **FR-001**: The daemon MUST publish these metric families with the `putio_sync_` prefix:
  - Put.io API requests by operation and outcome class;
  - API retries by reason;
  - API request latency (one overall histogram; per operation only at `detailed`, see
    FR-009);
  - sync runs by outcome;
  - last sync duration;
  - last successful sync time;
  - per-sync file counts (found, queued, skipped, blocked, failed);
  - queue depth by state;
  - active downloads and configured workers;
  - downloads completed and failed;
  - downloaded bytes;
  - download duration;
  - blocked files;
  - watch scans, uploads, cleanups, invalid files and cleanup failures;
  - database errors by operation;
  - disk free and total for the download folder;
  - process start time;
  - build info.
- **FR-002**: Metric names MUST follow Prometheus conventions: a `_total` suffix for
  counters, and base units (`_bytes`, `_seconds`).
- **FR-003**: Label values MUST come from fixed, documented sets. No metric may carry a file
  name, file id, path, URL, username or token.
- **FR-004**: Queue-depth gauges MUST reflect the database state within one sync interval,
  including right after startup.

**Exposure**

- **FR-005**: Metrics MUST be served over HTTP in the Prometheus text exposition format, at
  a configurable bind address and path. They are enabled by default, with defaults
  documented. The default bind address is all interfaces (`0.0.0.0`) on the dedicated
  metrics port, so a published container port works with no extra setting. The README
  warns that anything that can reach the port can read metrics.
- **FR-006**: The metrics endpoint MUST be readable without credentials. There is no auth
  setting for it, and access is controlled by the bind address and the published port. It
  MUST NOT expose any Put.io data or secrets, and MUST be read-only.
- **FR-007**: The API server mode MUST start the same dedicated metrics listener as the
  daemon, with the same settings and metric names. The API port MUST NOT serve metrics.
- **FR-008**: Failure to open the metrics listener MUST NOT stop the daemon or
  the API server. It is logged
  once at ERROR.

**Verbosity**

- **FR-009**: These settings MUST be settable both in config and through an environment
  variable, with the environment taking precedence and the effective source logged at
  startup:
  - log level (including per-module directives);
  - log format (`compact`, `pretty` or `json`, the existing values);
  - metrics enabled;
  - metrics bind address and path;
  - metrics detail (`basic` or `detailed`). `basic` is the default. `detailed` adds Put.io
    latency per operation and per-worker state and bytes-in-flight gauges, labelled by worker
    number. It changes nothing else.
- **FR-010**: A log-level change in the config file MUST take effect while the daemon
  runs, within 30 s, without a restart, signal or command. The daemon notices the change
  itself. Exceptions:
  - if the environment set the level, the change is ignored, with one log line saying so;
  - if the edited file can't be read or the new level is invalid, the current level is
    kept, with one ERROR line naming the problem.

  Only the log level (including per-module directives) is applied live. Other settings in
  FR-009 still need a restart.
- **FR-011**: Invalid values for any verbosity setting MUST fail startup with a message
  naming the setting and its allowed values.

**Home network integration** (changes in `~/dev/home-network`)

- **FR-012**: Add a Prometheus scrape job for the NAS putio-sync instance, following that
  repo's existing job conventions (`job_name`, `/metrics`, and `host`/`app` labels).
- **FR-013**: Add alert rules for: target down, no recent successful sync, queue stuck,
  sustained Put.io retries, low disk space and database errors. They go in that repo's
  existing alert-rule template, with severities and durations as in US3.
- **FR-014**: Add an app page following that repo's app template. It documents:
  - the endpoint;
  - the port the NAS container must publish;
  - verification steps;
  - useful queries.
- **FR-015**: Add a provisioned Grafana dashboard for putio-sync to that repo's Grafana
  role. (P3)
- **FR-016**: All home-network changes MUST pass that repo's `make check`. Deploying to the
  monitoring server and changing the NAS container are done by the operator, not by this
  feature.

**Documentation**

- **FR-017**: All new settings and environment variables MUST be documented in
  `config.example.toml` and the README, along with:
  - the full metric list, with type, labels and meaning;
  - which questions each metric answers.

### Key Entities

- **Metric family**: a named measurement. Each has:
  - a type (counter, gauge or histogram);
  - a help text;
  - a fixed set of label names, each with a documented closed set of values.
- **Telemetry settings**: whether metrics are on, where they are served, and how detailed
  they are, plus log level, log format and where each value came from (config or
  environment).
- **Scrape target**: the NAS instance as seen by the home Prometheus. It has an address, a
  job name, `host`/`app` labels, and the alert rules and dashboard that use it.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Each question below can be answered by one query or one dashboard panel, in
  under 1 minute, without SSH or log reading. Each was answered in feature 002 by grepping
  logs.
  - How many syncs failed in the last week?
  - How often were Put.io requests retried, and why?
  - How many files are pending, and is the queue moving?
  - How many executables were blocked?
  - How many watch files were cleaned up?
  - Were there any database errors?
- **SC-002**: The metrics endpoint answers in under 1 s while downloads are running. With
  metrics on, there is no measurable drop in download throughput (within 2%) compared to
  metrics off, on the same synthetic download.
- **SC-003**: After deploy, the home Prometheus shows the putio-sync target up at least 99%
  of the time over a week, excluding the expected weekly backup restart.
- **SC-004**: Every alert in US3 fires in its synthetic test and stays silent on healthy
  data. The test checks 0 false positives against a week of real series after deploy.
- **SC-005**: An operator can switch logs to debug, then back, using only the config file
  and without a restart. The change shows up in the logs within 30 s.
- **SC-006**: No metric label value contains a file name, file id, URL or path, and the
  token appears nowhere in the metric output. An automated check over the full metric
  output verifies both. Log lines keep their file names (Principle V) and must never
  contain the token (Principle VI, already enforced by redaction).

## Assumptions

- Metrics are on by default, on a dedicated port. The port is chosen in the plan, isn't
  used by other apps in the home-network repo, and is documented. Metrics are
  always unauthenticated (no auth option), because they contain no Put.io data. This matches the
  constitution, which allows unauthenticated health and metrics endpoints.
- The NAS container change (publishing the metrics port in the Unraid template) and the
  deploy of the monitoring stack are done by the operator. This feature documents both
  steps, and the scrape job assumes the documented port on 192.168.1.50.
- Which image the NAS runs (Docker Hub vs GHCR) is still open, and is out of scope here.
  It's handled with the image feature.
- Shipping logs to Loki is out of scope. JSON log format is offered so that it can be
  added later without changes here.
- Tracing spans or distributed tracing export (for example OTLP) are out of scope. Metrics
  and structured logs are enough for one daemon on one host.
- Metrics are kept in memory only. History lives in Prometheus, so the daemon persists
  nothing new.
- The web UI (feature 004) may later read the same metrics. This feature doesn't add UI.
