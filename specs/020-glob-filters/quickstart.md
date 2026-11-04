# Quickstart: Glob Filters with AND and OR

## Automated

```bash
cargo test -p putio-sync-core pattern
cargo test -p putio-sync-core --test filter_cases
cargo test -p putio-sync-core rules
cargo test -p putio-sync-api --test rules --test settings --test ui_js
TMPDIR=<scratch> make ci
```

Expected: the shared table passes in both the Rust test and `node --test`; rule tests
show pattern refusals with their reasons.

## Manual (mock UI)

1. `cargo run -p putio-sync-api --example mock_ui`, open the Put.io page, open a folder
   with a sample file.
2. Edit the folder rule, set Never download to `*sample*`, press Preview: the sample is
   listed as blocked with "matches never-download pattern `*sample*`". Save: its badge
   says blocked.
3. Settings › Download: set Only download to `*.mkv,*.mp4`, press Preview: counts for the
   whole account and a sample of files that download.
4. Type `"abc` in Never download and save: the error names the setting and the unclosed
   quote.
5. Set a rule with Only download `*.mkv` and Delete blocked files: the Settings banner and
   the doctor Config check warn.
