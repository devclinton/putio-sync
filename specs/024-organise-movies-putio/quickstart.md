# Quickstart: Organise Movies on Put.io

1. `cargo test -p putio-sync-core organise` — naming contract and real Radarr names.
2. `cargo test -p putio-sync-download --test organise` — a film planned, applied, undone; a sample skipped.
3. `cargo test -p putio-sync-api --test organise` — settings round-trip with `movies`.
4. `node --test putio-sync-api/tests/ui/organise.test.mjs`.
5. By hand: set `[organise] movies = true` with stream mode on, sync a film, open Put.io ›
   Organise, check the planned move, Apply, then Undo.
