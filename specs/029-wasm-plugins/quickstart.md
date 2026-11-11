# Quickstart: WebAssembly Plugins

Run from the repository root.

## Tests

```bash
cargo test -p putio-sync-plugins --test wasm --test examples
cargo test -p putio-sync-plugins --lib
cargo test -p putio-sync-api --test plugins --test ui_assets --test ui_i18n
node --test putio-sync-api/tests/ui/plugins.test.mjs
```

Expected: the WAT modules time out at 1 s, hit the 32 MiB memory limit, fail with their
message, call the host and show as Broken for bad imports; both examples load and answer
(the Go one only when `go` is installed, otherwise the test says it skipped it); installs
from files, a module and URLs work and bad ones say why.

## By hand

1. Build the Rust example (see `examples/plugins/rust-clean-names/README.md`).
2. Start the daemon with a `plugins_dir`, open Settings › Plugins › **Install a plugin**,
   and upload `examples/plugins/rust-clean-names/plugin.wasm`.
3. The plugin is listed off. **Enable** it; it shows
   as Enabled and **Send test event** answers with a run.
4. Sync a file named like `Show.S01E02.1080p.WEB.x264-GRP.mkv`: it lands as
   `Show S01E02.mkv`.
5. Install it again without **Replace a plugin of the same name**: refused with
   `a plugin named "rust-clean-names" is installed`.
