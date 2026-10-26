# Quickstart: validating feature 008

Everything runs offline against mocked Put.io (Constitution III).

## Automated

```bash
make ci
```

Covers: `clean_transfers` in `putio-sync-client`; the four bulk routes, the delete guard over
several ids, dedup, empty lists and the clear fallback in
`putio-sync-api/tests/bulk_files.rs` and `bulk_transfers.rs`.

## By hand in the UI

```bash
cargo run -p putio-sync-api --example mock_ui
```

On `#/putio/100` (Movies):

1. Big.Buck.Bunny is queued: its checkbox is disabled.
2. Tick Extras and setup.exe → "2 selected"; Delete → confirm says 2 items → toast, list reloads
   (the mock keeps listing them).
3. Select-all ticks every row except the queued one; the header box shows mixed when partly
   selected. Type in the filter → only visible rows stay selected.
4. Download with two selected → one summed toast.

On `#/transfers`:

1. Tick two rows → Retry / Remove / Clear selection; the selection survives the 5 s refresh.
2. Remove → confirm with the count → toast.
3. Clear completed → confirm says 2 (one completed, one seeding) → toast. Clear failed → 1.
   Clear all finished → 3.
