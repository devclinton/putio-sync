# Quickstart: validating feature 007

Everything runs offline against mocked Put.io (Constitution III).

## Automated

```bash
make ci
```

Covers: tracker transitions with an injected clock (`putio-sync-core`), `current_ratio` as text
and new fields (`putio-sync-client`), the stalled gauge in the 003 metrics contract
(`putio-sync-telemetry`), the daemon check task against mockito, and the re-add route with all
failure cases (`putio-sync-api/tests/transfer_readd.rs`).

## By hand in the UI

```bash
cargo run -p putio-sync-api --example mock_ui
```

The mock serves one stalled transfer (97%, availability 92%) with a pre-seeded stall record,
one downloading normally, and one without a magnet link. Expected on `#/transfers`:

1. The stalled one shows `stalled …h` and "availability 92%", plus Copy magnet and Re-add.
2. The normal one shows Copy magnet only.
3. The one without a magnet shows neither.
4. Re-add → confirm → toast names the new transfer; cancel the dialog → nothing happens.

## Config

```toml
[transfers]
stall_threshold_hours = 6
stall_check_interval_secs = 600
```
