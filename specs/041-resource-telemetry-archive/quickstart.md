# Quickstart: Checking CPU and Memory Telemetry, Kept Long Term

These steps prove the feature works. Details live in [contracts/](contracts/) and
[data-model.md](data-model.md); this page doesn't repeat them.

## 1. Code checks (putio-sync)

```bash
make ci
```

Expected: everything passes, including the `/proc` parser fixtures, the scrape tests from
[contracts/metrics.md](contracts/metrics.md#invariants-tested), the archive tests, and
`scripts/check-grafana-dashboard.sh` on `packaging/grafana/putio-sync-resources.json`.

## 2. Local scrape

Run the daemon with `[metrics] enabled = true`, then:

```bash
curl -s localhost:9742/metrics | grep -E '^putio_sync_(process_|sync_in_progress)'
```

- **Linux (or the Docker image)**: all 8 `putio_sync_process_*` families plus
  `putio_sync_sync_in_progress`. CPU, memory and threads are above 0.
- **macOS**: only `putio_sync_sync_in_progress`, and one WARN line,
  `Process metrics unavailable ... os=macos`, in the log, not one per scrape.

Start a download and scrape again. `sync_in_progress` reads 1 during a sync run, and CPU
seconds go up.

## 3. Recording rules and alerts (promtool)

```bash
promtool check rules putio-sync-recording.yml
promtool test rules specs/041-resource-telemetry-archive/alert-tests.yml
```

Expected: rules are valid. Each of the 6 alerts in
[contracts/long-term.md](contracts/long-term.md#alerts-added-to-the-putio-sync-group-in-alertsymlj2)
has a firing case and a silent case, and both pass. The activity-state cases (downloading 2,
syncing, idle, downloading beats syncing) pass too.

## 4. Home-network checks

In `~/dev/home-network-new`:

```bash
make check
```

Then deploy with `deploy-monitoring.yml --limit monitoring_server`. On the monitoring server:

- `curl -s 127.0.0.1:9092/-/ready` returns ready.
- `up{job="prometheus_longterm"}` is 1 in the main Prometheus.
- After 10 minutes, `count({__name__=~"putio_sync:.+",app="putio-sync"})` is about 22.
- `curl -s 127.0.0.1:9092/api/v1/label/__name__/values` lists only `putio_sync:` names (plus
  Prometheus's own).

## 5. Long range on the dashboard

Open `putio-sync resources`:

- At **last 6 hours**, the over-time panels show raw detail.
- At **last 90 days**, once more than 30 days of thinned data exist (or after a restore, step
  7), the panels show one unbroken line. The `or` fallback fills the time before the raw
  30 days.
- "Use by activity" shows rows for idle, syncing and each "downloading N at once" seen.

Before 30 days of history exist, check the fallback by setting the main Prometheus retention
low on a test copy, or by querying the thinned series alone in Explore.

## 6. Archive runs (on the NAS, read-only checks first)

Before scheduling, run once by hand:

```bash
docker exec putio-sync putio-sync --config /config/config.toml archive metrics --prometheus http://192.168.1.51:9090
docker exec putio-sync putio-sync --config /config/config.toml archive db
docker exec putio-sync putio-sync --config /config/config.toml archive summary --prometheus http://192.168.1.51:9090
```

Expected:

- `metrics` prints one `saved <day> points=…/288` line per finished day. A second run prints
  nothing new.
- `db` writes `db/putio-sync-db-<today>.sqlite.gz`. A second run prints `already saved`.
  `gunzip -c … | sqlite3 /dev/stdin 'select count(*) from download_history'` matches the
  manifest's `rows`.
- `summary` writes the 3 files. Running it twice gives byte-identical files (`cmp`). With
  `--prometheus` left out, resource columns are blank, not 0.
- Stop the monitoring server's Prometheus, then run `metrics`: exit code 2, one stderr line, no
  files changed.

Then install `scripts/archive/unraid-user-script.sh` as a User Scripts entry
([contracts/archive-cli.md](contracts/archive-cli.md#unraid-wrapper-scriptsarchiveunraid-user-scriptsh)).
The User Scripts plugin must be installed first; that is the operator's step.

## 7. Restore and gap fill (monitoring server)

- **Restore**: pick an archived day. Gunzip it, then run `promtool tsdb create-blocks-from
  openmetrics` into a spare dir and start a throwaway Prometheus on it. The day's series match
  the dashboard for that day.
- **Gap fill**: stop the long-term instance for 1 hour. `PutioSyncLongTermDown` stays silent
  (its `for` is 2h), and `PutioSyncLongTermBehind` fires. Start it again, run the
  `create-blocks-from rules` step in
  [contracts/long-term.md](contracts/long-term.md#gap-fill-and-restore-documented-run-by-hand-on-the-monitoring-server)
  for the gap, and move the blocks in. The hole is gone at "last 7 days".

## 8. Done when

- Steps 1–4 pass.
- Step 5 shows one unbroken line once data allows.
- Step 6 runs clean twice in a row.
- A restore from step 7 has worked at least once.
- `CHANGELOG.md` has an entry.
