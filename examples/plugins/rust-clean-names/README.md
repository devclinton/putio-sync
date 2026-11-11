# rust-clean-names

The [`clean-names`](../clean-names/) **rename** rule written in Rust and built to
WebAssembly (feature 029): `Show.S01E02.1080p.WEB.x264-GRP.mkv` becomes `Show S01E02.mkv`,
and `The.Movie.2019.1080p.BluRay.x264.mp4` becomes `The Movie (2019).mp4`. It also logs a
line per finished download it renamed.

`src/putio.rs` is the binding: argument and result passing, `putio::call` for the host
API, and the `putio-plugin` custom section that carries `plugin.toml`, so `plugin.wasm`
installs on its own. Copy it to start a plugin of your own.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown --manifest-path examples/plugins/rust-clean-names/Cargo.toml
cp examples/plugins/rust-clean-names/target/wasm32-unknown-unknown/release/rust_clean_names.wasm examples/plugins/rust-clean-names/plugin.wasm
```

The checked-in `plugin.wasm` (about 120 KB) is loaded by
`putio-sync-plugins/tests/examples.rs`. Rebuild it after changing `src/` or
`plugin.toml`.

## Install

Copy this folder (`plugin.toml` and `plugin.wasm` are enough) into your plugins folder, or
upload `plugin.wasm` in Settings › Plugins › Install a plugin. Then enable it.
