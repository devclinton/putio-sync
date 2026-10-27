# Quickstart: checking per-folder file rules

Nothing here touches the live Put.io account or the NAS.

## 1. Automated

```bash
TMPDIR=<scratchpad> make ci
```

Covers: rule resolution and presets (`putio-sync-core` `rules` tests), config load/validate/save round trips with comments (`config` tests), `enqueue_tree` decisions, deletes and per-file delete-after-download / skip-existing against mockito (`putio-sync-download`), the `rule_decisions` repository (`putio-sync-db`), the `/rules` routes (`putio-sync-api` tests), and the metric (`putio-sync-telemetry`).

## 2. UI against the mock

```bash
cargo run -p putio-sync-api --example mock_ui 15532
```

Sign in with the printed token, then:

1. `#/putio/200` (Shows): the rule card shows "Video only" set on this folder; `Example.Show.S01E01.nfo` has a "blocked" badge.
2. `#/putio/100` (Movies): the card shows the global defaults. Edit → preset "Video only", turn "Delete blocked files" on → Preview lists `setup.exe` as "would be deleted" → Save asks to confirm → the card now says "this folder".
3. `#/putio/110` (Movies/Extras): the card shows "Video only" inherited from Movies.
4. Back on Movies: Clear rule → the card is back to global.
5. `#/settings`: Folder rules lists Shows (and Movies if not cleared) with Remove; Presets lists the five built-ins; add `comics` with `cbz, cbr`; it then appears in the Put.io preset list; edit "Video only", then Reset. Recently blocked shows the seeded `setup.exe` entry with the rule folder and reason.
6. The mock's config file (path printed at start) holds `[[download.rules]]` and `[download.presets.comics]`, and its seeded comment line is still there.
7. Check at 375 px wide and in dark mode: no sideways scroll, the rule form stacks.
