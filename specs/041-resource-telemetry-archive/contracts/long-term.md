# Contract: Long-Term Copy, Dashboard and Alerts (`~/dev/home-network`)

All changes are additive. They are deployed with `deploy-monitoring.yml --limit monitoring_server`.

## Long-term instance

| Setting | Value (role default, overridable) |
|---|---|
| Binary | the Homebrew `prometheus` already installed |
| Service | a second LaunchDaemon/LaunchAgent, mirroring `prometheus_run_as_daemon` |
| Listen | `127.0.0.1:9092` (`prometheus_longterm_listen`) |
| Data dir | `{{ prometheus_data_dir }}-longterm` (never shared with the main TSDB) |
| Retention | `prometheus_longterm_retention_time: "1y"` |
| Flags | `--web.enable-remote-write-receiver`, `--storage.tsdb.retention.time`, `--storage.tsdb.path`, `--config.file` |
| Config | `global:` only, no scrape jobs |

The main Prometheus scrapes it as `job_name: prometheus_longterm`, so `up` is known.

## Main Prometheus additions (`prometheus.yml.j2`)

```yaml
rule_files:
  - putio-sync-recording.yml        # alongside the existing alerts file

remote_write:
  - name: putio-longterm
    url: http://127.0.0.1:9092/api/v1/write
    write_relabel_configs:
      - source_labels: [__name__]
        regex: "putio_sync:.+:[a-z]*5m"
        action: keep

remote_read:
  - name: putio-longterm
    url: http://127.0.0.1:9092/api/v1/read
    read_recent: false
    required_matchers:
      app: putio-sync
```

`required_matchers` means only queries that name `app="putio-sync"` reach the long-term
instance. Dashboard and summary queries always include it.

## Recording rules (`putio-sync-recording.yml.j2`)

One group, `putio-sync-5m`, `interval: 5m`. Rules run in order, so later rules may use earlier
ones. Rules shown as "and :max5m" follow the `avg_over_time` / `max_over_time` pattern above.

```yaml
- record: putio_sync:process_cpu_usage:avg5m
  expr: rate(putio_sync_process_cpu_seconds_total{job="putio_sync"}[5m])
- record: putio_sync:process_cpu_usage:max5m
  expr: max_over_time(rate(putio_sync_process_cpu_seconds_total{job="putio_sync"}[1m])[5m:15s])
- record: putio_sync:process_resident_memory_bytes:avg5m
  expr: avg_over_time(putio_sync_process_resident_memory_bytes{job="putio_sync"}[5m])
- record: putio_sync:process_resident_memory_bytes:max5m
  expr: max_over_time(putio_sync_process_resident_memory_bytes{job="putio_sync"}[5m])
- record: putio_sync:process_virtual_memory_bytes:max5m
  expr: max_over_time(putio_sync_process_virtual_memory_bytes{job="putio_sync"}[5m])
- record: putio_sync:process_threads:avg5m            # and :max5m
- record: putio_sync:process_open_fds:avg5m           # and :max5m
- record: putio_sync:process_max_fds:last5m
  expr: last_over_time(putio_sync_process_max_fds{job="putio_sync"}[5m])
- record: putio_sync:process_disk_read_bytes:increase5m
  expr: increase(putio_sync_process_disk_read_bytes_total{job="putio_sync"}[5m])
- record: putio_sync:process_disk_written_bytes:increase5m
  expr: increase(putio_sync_process_disk_written_bytes_total{job="putio_sync"}[5m])
- record: putio_sync:downloads_active:avg5m           # and :max5m
- record: putio_sync:downloaded_bytes:increase5m
  expr: increase(putio_sync_downloaded_bytes_total{job="putio_sync"}[5m])
- record: putio_sync:sync_in_progress:avg5m
  expr: avg_over_time(putio_sync_sync_in_progress{job="putio_sync"}[5m])
- record: putio_sync:start_time_seconds:max5m
  expr: max_over_time(putio_sync_start_time_seconds{job="putio_sync"}[5m])
- record: putio_sync:build_info:max5m
  expr: max_over_time(putio_sync_build_info{job="putio_sync"}[5m])
- record: putio_sync:up:min5m
  expr: min_over_time(up{job="putio_sync"}[5m])
- record: putio_sync:activity_state:5m
  expr: |
      (round(putio_sync:downloads_active:avg5m) >= 1)
    or
      ((putio_sync:sync_in_progress:avg5m >= 0.5) * 0 - 1)
    or
      (putio_sync:sync_in_progress:avg5m * 0)
```

Every rule keeps the target labels (`job`, `instance`, `host`, `app`). `rate` and `increase`
handle counter resets at restarts.

## Gap fill and restore (documented, run by hand on the monitoring server)

- **Gap fill** (raw still in the 30 days):
  `promtool tsdb create-blocks-from rules --start <t> --end <t> --url http://127.0.0.1:9090 putio-sync-recording.yml`,
  then move the output blocks into the long-term data dir.
- **Restore from archive**: `promtool tsdb create-blocks-from openmetrics <day>.om (gunzipped) <out>`,
  then move the blocks in. Only load days the long-term copy lacks, or load into a spare data
  dir.

## Dashboard: `putio-sync resources` (uid `putio-sync-resources`)

Source: `packaging/grafana/putio-sync-resources.json` in putio-sync. The home-network copy is
`roles/monitoring_grafana/templates/putio-sync-resources-dashboard.json.j2`. It has the same
`host` variable as the `putio-sync` dashboard, and the two link to each other.

Panels:

| Row | Panel | Query (shape) |
|---|---|---|
| Now | Memory in use, CPU, threads, open files / limit, uptime | raw families |
| Over time | CPU avg and peak | `rate(raw[$__rate_interval]) or on(instance) last_over_time(putio_sync:process_cpu_usage:avg5m{app="putio-sync",host=~"$host"}[10m])`; the same for `:max5m` |
| Over time | Memory in use (avg/peak), reserved | `raw or on(instance) last_over_time(thinned[10m])` |
| Over time | Threads, open files vs limit, disk read/write | same pattern |
| Over time | Activity: downloads running, download speed, syncing | raw and thinned, on the same time axis |
| Annotations | Restarts and version changes | `changes(putio_sync:start_time_seconds:max5m[10m]) > 0`, `changes(putio_sync:build_info:max5m[10m])` via label |
| Use by activity | Table: state → time, CPU avg/peak, memory avg/peak over `$__range` | `avg_over_time((putio_sync:process_cpu_usage:avg5m and on(instance) (putio_sync:activity_state:5m == N))[$__range:5m])` for each state; N from 1 to `max_over_time(putio_sync:activity_state:5m[$__range])` |
| Use by activity | CPU and memory per MB/s | `avg_over_time((putio_sync:process_cpu_usage:avg5m / (putio_sync:downloaded_bytes:increase5m / 300 / 1e6) and on(instance) putio_sync:activity_state:5m >= 1)[$__range:5m])` |

The "Use by activity" row always reads the thinned series, so it means the same thing at
every range.

## Alerts (added to the `putio-sync` group in `alerts.yml.j2`)

| Alert | Expression | For | Severity |
|---|---|---|---|
| `PutioSyncMemoryHigh` | `putio_sync_process_resident_memory_bytes > 1073741824` | 30m | warning |
| `PutioSyncMemoryGrowing` | `predict_linear(putio_sync_process_resident_memory_bytes[24h], 604800) > 1073741824 and changes(putio_sync_start_time_seconds[24h]) == 0` | 1h | warning |
| `PutioSyncCpuHigh` | `rate(putio_sync_process_cpu_seconds_total[5m]) > 0.9` | 30m | warning |
| `PutioSyncOpenFilesHigh` | `putio_sync_process_open_fds / putio_sync_process_max_fds > 0.8` | 10m | warning |
| `PutioSyncLongTermBehind` | `max(prometheus_remote_storage_highest_timestamp_in_seconds{job="prometheus"}) - max(prometheus_remote_storage_queue_highest_sent_timestamp_seconds{job="prometheus",remote_name="putio-longterm"}) > 7200` | 5m | warning |
| `PutioSyncLongTermDown` | `up{job="prometheus_longterm"} == 0` | 2h | warning |

Each one gets a firing and a silent case in `specs/041-resource-telemetry-archive/alert-tests.yml`.
The monitoring server's own disk is already covered by `DiskSpaceLow`, so FR-012 adds nothing.

## App page (`apps/putio-sync.md`) additions

- Retention: raw for 30 days, thinned for 1 year, archive files kept forever by default.
- Disk: the long-term copy is a few MB a year. Check with `du -sh <data dir>-longterm`.
- Checks: `up{job="prometheus_longterm"}`, and the `PutioSyncLongTermBehind` alert.
- If it stopped: restart the service, then gap-fill (above) for anything still in the raw 30
  days.
- Archive: the schedule on the NAS, the archive folder, and restore steps.
