# Quickstart: Stats Page

How to check feature 006 end to end without the live Put.io account or the NAS.

## Automated

```bash
TMPDIR="$PWD/target/tmp" make ci
```

Covers: aggregation (buckets, spreading, time zone, filters), host parsing, source
matching in `enqueue_tree`, the `download_sources` repository, `GET /stats/summary`
(auth, validation, sections), the passkey sweep and the UI asset budget.

## Browser check against the mock server

```bash
cargo run -p putio-sync-api --example mock_ui 15532
```

Port 15432 may be in use locally; pick any free port. The example prints the URL and the
mock token.

1. Sign in, open **Stats** from the nav. Range 7d shows totals, a 28-bar chart, a heatmap
   brighter in the evenings, type and source breakdowns.
2. Switch through `24h`, `30d`, `90d`, `1y`, `All`: bar counts 24, 30, 90, 53 and whatever
   fits the seeded 120 days (days or weeks).
3. Pick **Video** in the type filter: totals drop, subtitles disappear from the type card.
   Pick **Private** in privacy: only the private tracker remains in the tracker list.
4. Reload: the same filters come back. Copy the URL to a new tab: same view.
5. Hover and tap bars and heatmap cells: the readout lines update. Tab to a chart and use
   the arrow keys.
6. Phone width (375 px): no sideways scroll; heatmap readable.
7. Pick a filter that matches nothing (e.g. URL + a tracker): every card says nothing
   matches; Clear restores.

## Expected response sizes

`curl -s -H "Authorization: Bearer <mock token>" "http://127.0.0.1:15532/stats/summary?range=all" | wc -c`
stays well under 50 KB.
