# Quickstart: validate 056

1. `cargo test -p putio-sync-core profile` — plan table per kind, keys round-trip, kept
   hand-made changes.
2. `cargo test -p putio-sync-api --test setup_profile` — detect, review, finish writes the
   plan and `[profile]`, Settings preview/PUT keep a hand-changed value.
3. `node --test putio-sync-api/tests/ui/profile.test.mjs` — code from the fragment, reason
   text for each code.
4. Manual: `PUTIO_SYNC_CONFIG=/tmp/x/config.toml putio-sync daemon` with no config; open the
   printed `…/setup#code=…` link; the address bar drops the code; pick a language, link
   Put.io, tick kinds, review, Start syncing; the config has `[profile]` and the rules.
5. Settings › Usage profile: tick another kind, preview lists it; change a value by hand,
   run again, it is listed as kept.
