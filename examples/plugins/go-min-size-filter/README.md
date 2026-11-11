# go-min-size-filter

The [`min-size-filter`](../min-size-filter/) **filter** rule written in Go and built to
WebAssembly (feature 029): videos smaller than **Smallest video (MB)** (default 50) are not
downloaded, so samples, trailers and extras stay on Put.io.

`putio.go` is the binding: `//go:wasmimport` for the host's `call`, `reply` and `fail`,
`//go:wasmexport` for `putio_alloc` and `putio_free`, and JSON helpers. Copy it to start a
plugin of your own.

## Build

With Go 1.24 or later:

```bash
GOOS=wasip1 GOARCH=wasm go -C examples/plugins/go-min-size-filter build -buildmode=c-shared -trimpath -ldflags="-s -w" -o plugin.wasm .
```

With TinyGo (0.34 or later, for `//go:wasmexport`), which gives a much smaller module:

```bash
tinygo build -C examples/plugins/go-min-size-filter -target=wasip1 -buildmode=c-shared -no-debug -o plugin.wasm .
```

`plugin.wasm` is not checked in. `putio-sync-plugins/tests/examples.rs` builds it with `go`
and loads it; without Go the test says so and checks only the Rust example. The TinyGo
build has not been tested here.

## Install

Copy this folder with its `plugin.wasm` into your plugins folder, or install
`plugin.toml` and `plugin.wasm` together in Settings › Plugins › Install a plugin. Then
enable it.
