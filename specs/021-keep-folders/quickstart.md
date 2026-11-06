# Quickstart: Keep folders out of empty-folder cleanup

1. `cargo test -p putio-sync-client cleanup` — walk keeps rule- and pattern-kept folders
   and lists every kept folder with its reason.
2. `cargo test -p putio-sync-core` — `keep` resolves with inheritance; bad keep patterns
   fail validation; the cleanup service builds the callback.
3. `cargo test -p putio-sync-api` — settings and rule routes round-trip `keep_patterns`
   and `keep`.
4. UI against the mock backend (`cargo run -p putio-sync-api --example mock_ui`):
   - Put.io page → open a folder → rule card shows "Keep out of cleanup"; press
     Keep folder → row says yes · this folder.
   - Settings → Put.io → Put.io cleanup shows Keep patterns.
   - Put.io page → Remove empty folders → the dialog lists kept folders with reasons.
