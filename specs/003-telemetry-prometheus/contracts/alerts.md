# Contract: Prometheus Alerts and Scrape Job (home-network)

## Scrape job (`group_vars/monitoring_server/main.yml`, appended to `prometheus_extra_scrape_configs`)

```yaml
  # putio-sync daemon on the NAS (Unraid Docker template, not Ansible-managed).
  # The container must publish 9742; see apps/putio-sync.md.
  - job_name: putio_sync
    metrics_path: /metrics
    static_configs:
      - targets: ["192.168.1.50:9742"]
        labels: {host: "nas01", app: "putio-sync"}
```

## Alert group (`roles/monitoring_prometheus/templates/alerts.yml.j2`)

The sync interval is the CLI's `--download-interval` (default 300 s). It isn't exported as
a metric, so the rule hard-codes "4× interval, minimum 30 min" as 1800 s, assuming the
default.

- T030 checks the NAS container's value first.
- If the interval is above 450 s, the threshold becomes 4 × the interval.
- Either way, the value used is recorded in the rule's description and on
  `apps/putio-sync.md`.

| Alert | Expr | For | Severity |
|---|---|---|---|
| `PutioSyncDown` | `up{job="putio_sync"} == 0` | 10m | critical |
| `PutioSyncNoRecentSuccess` | `time() - putio_sync_last_success_timestamp_seconds > 1800` | 5m | warning |
| `PutioSyncQueueStuck` | `putio_sync_queue_items{state="pending"} > 0 and putio_sync_downloads_active > 0 and increase(putio_sync_downloads_completed_total[2h]) == 0` | 15m | warning |
| `PutioSyncApiRetriesHigh` | `increase(putio_sync_api_retries_total[1h]) > 20` | 1h | warning |
| `PutioSyncDiskLow` | `putio_sync_disk_free_bytes / putio_sync_disk_total_bytes < 0.10` | 15m | critical |
| `PutioSyncDbErrors` | `increase(putio_sync_db_errors_total[15m]) > 0` | 0m | warning |

- `PutioSyncNoRecentSuccess` also fires if the gauge is absent for 30 min after startup.
  This is done with
  `or (time() - putio_sync_start_time_seconds > 1800 unless putio_sync_last_success_timestamp_seconds)`.
- The retry rule's expression sums by instance:
  `sum by (instance) (increase(putio_sync_api_retries_total[1h])) > 20`.
- Annotations follow the existing rules in that file (`summary`, `description`).
  Prometheus template braces are escaped the way that file already does it:
  `{{ '{{' }} $labels.host {{ '}}' }}`.

## Synthetic rule tests (SC-004)

A `promtool test rules` file (kept in putio-sync at `specs/003-telemetry-prometheus/alert-tests.yml`)
has one firing series and one healthy series for each alert. It runs against the rendered
alert group, following the steps in [quickstart.md](../quickstart.md#4-alert-rules).
