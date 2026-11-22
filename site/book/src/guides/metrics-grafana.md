# Watch it with Prometheus and Grafana

**You'll need:** putio-sync running, and Prometheus and Grafana that can reach it.

**You'll end with:** Prometheus collecting putio-sync's metrics, and a Grafana dashboard
showing them.

## 1. Check the metrics are on

Metrics are on by default, on port `9742`. Open `http://<host>:9742/metrics` in a browser;
it shows lines starting with `putio_sync_`. If it doesn't load:

- **In a container**, publish the port: add `9742:9742` to the Compose file's `ports:`, or
  set the **Metrics** port in the Unraid template. On TrueNAS, it's the **Metrics Port**
  (`30570`).
- **Settings › Network and access** shows the metrics addresses, and says when metrics are
  off.

The metrics have no password, and never hold tokens, file names or paths. Keep the port on
your home network.

## 2. Add it to Prometheus

Add a scrape job to `prometheus.yml`, with your host in place of `nas`:

```yaml
scrape_configs:
  - job_name: putio_sync
    static_configs:
      - targets: ["nas:9742"]
        labels: {app: "putio-sync", host: "nas"}
```

Reload Prometheus. Under **Status › Targets**, the `putio_sync` job shows **UP**.

The `app` and `host` labels are what the dashboard filters on, so keep them.

## 3. Import the dashboard

1. Download
   [`putio-sync-resources.json`](https://github.com/devclinton/putio-sync/blob/main/packaging/grafana/putio-sync-resources.json).
2. In Grafana, open **Dashboards › New › Import**, and upload it.
3. Pick your Prometheus data source, and **Import**.

It shows CPU, memory, disk and uptime next to downloads and syncs.

## Useful queries

| What | Query |
|------|-------|
| Download speed | `rate(putio_sync_downloaded_bytes_total[5m])` |
| Files waiting | `putio_sync_queue_items{state="pending"}` |
| Failed downloads today | `increase(putio_sync_downloads_failed_total[1d])` |
| Time since the last good sync | `time() - putio_sync_last_success_timestamp_seconds` |

[Metrics](../metrics.md) lists every metric, and the alert rules that come with them.
