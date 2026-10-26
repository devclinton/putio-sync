# UI Contract: Stats Page

Route: `#/stats`, optional query
`?range=&cat=&ext=&source=&tracker=&private=&origin=&watch=&folder=`. Nav link
"Stats" after "History".

## Layout (top to bottom; one column on phones, two from 900 px)

1. **Head**: "Stats" and a one-line summary of the active filters.
2. **Filters** (toolbar, wraps on phones):
   - range: segmented buttons `24h 7d 30d 90d 1y All` (`aria-pressed`);
   - type: `<select>` with "All types", the seven categories, then the extensions from
     `options.extensions`;
   - source: `<select>` "All sources", Torrent, URL, Other, Unknown;
   - tracker: `<select>` "All trackers" + `options.trackers` (hidden when empty);
   - privacy: `<select>` "Public and private", Private, Public;
   - origin: `<select>` "All origins", "Watch folder" (then one entry per watch folder),
     "Added in putio-sync", "Added on Put.io", "Unknown";
   - Put.io folder: `<select>` "All folders" + `options.folders` (hidden when empty);
   - "Clear" button when any filter other than range is set.
3. **Totals**: four tiles — Files, Downloaded, Time downloading, Average speed.
4. **Bandwidth** card: bar chart of `series[].bytes`; y-axis max label as bytes, x-axis
   start, middle and end labels as local dates/times; readout line under it:
   "Tue 21 Oct, 18:00–00:00 · 4.2 GB · 3 files · 1.6 MB/s avg".
5. **When** card: 7 × 24 heatmap, toggle "Bytes / Files"; day labels Mon…Sun, hour labels
   0, 6, 12, 18; readout "Saturday 21:00 · 12 files · 38.1 GB"; a five-step legend.
6. **File types** card: one row per category (name, bar for bytes share, files, bytes),
   top extensions as small text; clicking a row sets the type filter.
7. **Origin** card: two bar lists (Origin with watch folders listed by name under
   "Watch folder"; Put.io folder); clicking a row sets that filter.
8. **Sources** card: three bar lists (Type, Tracker, Privacy); clicking a row sets that
   filter. A note: "Recorded for downloads queued since this version; earlier ones are
   unknown. Put.io doesn't report DHT; private torrents don't use it."

## Behaviour

- Fetches `GET /stats/summary` on mount and on every filter change (no polling; a
  "Refresh" button re-fetches). A late response for an older filter set is ignored.
- Filter changes rewrite the hash with `history.replaceState`; reload restores them.
- Empty result: each card says "Nothing in this range" or "Nothing matches the filters".
- Error: the cards are replaced by one error card with Retry, like History.
- Charts: pointer hover or tap selects a bar/cell; when a chart has focus, ←/→ (and ↑/↓
  for the heatmap) move the selection. Each chart has an `aria-label` summarising it.
- Fits 375 px without sideways scroll: the bar chart scales to the card width; heatmap
  cells shrink (min 8 px) and hour labels thin to 0/6/12/18.
