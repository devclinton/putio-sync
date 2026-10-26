# Quickstart: Validating Telemetry and Prometheus Monitoring

These steps prove the feature works. The automated checks run without Put.io
(Principle III). Steps 5 and 6 run against the live NAS, after the operator has deployed.

## Prerequisites

- Rust toolchain from `rust-toolchain.toml`. `make ci` passes on the branch.
- For step 4: `promtool` (`brew install prometheus`) and the `~/dev/home-network`
  checkout.

## 1. Local gate

```bash
make ci
```

Expected: fmt, clippy, tests and the release build all pass. The new tests cover:

- **putio-sync-telemetry:** every family is present at `basic`; detailed-only families
  appear only at `detailed`; the closed label sets and redaction invariants from
  [contracts/metrics.md](contracts/metrics.md#invariants-tested) hold.
- **putio-sync-client:** a mockito server that fails twice then succeeds gives
  `api_requests_total{outcome="server_error"} == 2`,
  `{outcome="success"} == 1` and `api_retries_total{reason="server_error"} == 2`.
- **putio-sync-download:** one synthetic download gives `downloads_completed_total == 1`,
  `downloaded_bytes_total == payload size` and `sync_runs_total{result="success"} == 1`.
  A blocked `.exe` increments `files_blocked_total` once across two syncs.
- **putio-sync-file-manager:** an upload, a cleanup and an invalid file each increment
  their counter.
- **putio-sync-core:**
  - each env override beats the config value, and the source is reported;
  - invalid values give the documented error;
  - the reload poller applies a level change from a temp config within one poll;
  - a change is ignored when `RUST_LOG` is set;
  - an invalid edit keeps the current level.
- **Listener:** binding a port that's already taken returns an error and doesn't panic.
  A scrape during a slow synthetic download returns in under 1 s.

## 2. Run the daemon locally against a mock

Use a temp config pointing at a mock Put.io base URL, or no token and the watch loop only.
Then run:

```bash
cargo run -p putio-sync-cli -- --config /path/to/temp-config.toml daemon
```

In a second shell:

```bash
curl -s localhost:9742/metrics | grep -E '^putio_sync_(build_info|start_time|queue_items|disk_free)'
```

Expected:

- `build_info` shows the version and a commit, or `unknown`;
- `start_time_seconds` is set;
- the `queue_items` gauges are present straight after startup;
- `disk_free_bytes` is greater than 0.

## 3. Verbosity

1. Run with `PUTIO_SYNC_METRICS_DETAIL=detailed`. `putio_sync_worker_state{worker="0"}`
   appears.
2. Run with `PUTIO_SYNC_METRICS_ENABLED=false`. You get `Metrics disabled source=Env` at
   startup, and `curl localhost:9742/metrics` is refused.
3. While it runs, change `[logging] level` from `info` to `debug` in the temp config.
   Within 30 s, you get `Log level changed old=info new=debug source=Config`, followed by
   debug lines.
4. Run again with `RUST_LOG=info` and edit the level. You get the "ignored; RUST_LOG is
   set" line.
5. Set `detail = "max"`. Startup fails with
   `invalid metrics.detail 'max': expected one of basic, detailed`.

## 4. Alert rules

Render only the `putio-sync` group from the home-network template. Ansible's Python has
jinja2 and PyYAML, and rendering turns `{{ '{{' }}` into `{{`:

```bash
"$(head -1 "$(which ansible)" | sed 's/^#!//')" -c "import jinja2, yaml, os; t = jinja2.Template(open(os.path.expanduser('~/dev/home-network/roles/monitoring_prometheus/templates/alerts.yml.j2')).read()); g = [g for g in yaml.safe_load(t.render(groups={}))['groups'] if g['name'] == 'putio-sync']; open('/tmp/putio-sync-alerts.yml', 'w').write(yaml.safe_dump({'groups': g}, sort_keys=False))"
```

```bash
promtool check rules /tmp/putio-sync-alerts.yml
```

```bash
promtool test rules specs/003-telemetry-prometheus/alert-tests.yml
```

Expected: `SUCCESS: 6 rules found`, then `SUCCESS`. Each alert in
[contracts/alerts.md](contracts/alerts.md) fires on its "bad" series and stays silent on
the "good" one.

## 5. Home-network checks

```bash
make -C ~/dev/home-network check
```

Expected: `✓ All offline checks passed`. The new scrape job, alert group and dashboard
template parse.

## 6. After deploy (operator)

1. Publish container port `9742` in the Unraid putio-sync template, and pull the new image.
2. Deploy the monitoring stack from home-network (that repo's monitoring playbook).
3. Check that the Prometheus targets page shows `putio_sync` (nas01) as **UP**.
4. Run `putio_sync_queue_items` and `increase(putio_sync_sync_runs_total[1h])` in
   Prometheus. Both return data.
5. Open the Grafana putio-sync dashboard. No panel shows an error.
6. After a week, check SC-003 (target up ≥99%) and SC-004 (no false-positive alerts).
