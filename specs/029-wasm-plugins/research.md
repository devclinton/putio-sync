# Research: WebAssembly Plugins

## R1. Runtime: wasmtime, wasmer or an interpreter

**Decision**: wasmi 2.0 (pure-Rust interpreter), features `std`, `validate`,
`auto-dispatch`, no default features.

**Measured** (release profile, aarch64-apple-darwin, rustc 1.96: a small test binary that
loads and runs one module, built without a runtime and with each one):

| Build | Binary | Growth | Clean build of the added crates |
|---|---|---|---|
| Without a runtime | 409,744 B | — | — |
| wasmi 2.0.0 | 1,828,640 B | about +1.4 MB | 13.2 s wall |
| wasmtime 49.0.5 (Cranelift) | 8,016,512 B | about +7.6 MB | 58.6 s wall, 256 s CPU |

wasmtime 50 needs a newer rustc than the 1.96 the release uses.

**Rationale**:

- Size: +1.4 MB against +7.6 MB, and SC-005 caps growth at 3 MB.
- Targets: the release builds many targets, including armv7, aarch64 and x86_64 musl,
  Windows and macOS. wasmi is plain Rust with no JIT, so it builds wherever the rest
  does. Cranelift has no backend for 32-bit arm (armv7) at all, so wasmtime there means
  Pulley (its interpreter) anyway, at the larger size. wasmer's LLVM/Cranelift backends
  have the same problem, plus a C toolchain for some of them.
- Compile time: the release builds every target in CI; wasmtime adds about a minute of
  wall time (four of CPU) per target, wasmi about 13 s.
- Speed: plugins do small JSON work per event or per file (a filter, a rename), bounded
  by a time limit of seconds or 500 ms. An interpreter is plenty; nothing here is
  compute-bound.
- Limits: wasmi has fuel (deterministic metering) and a `ResourceLimiter` for memory and
  tables, which map onto `timeout_secs` and `memory_mb`.

**Alternatives considered**:

- **wasmtime**: fastest, best WASI and component-model support; rejected on size, build
  time and armv7.
- **wasmer**: similar size to wasmtime with Cranelift; the singlepass backend is x86/arm64
  only; rejected for the same reasons.
- **wasm3** (C interpreter): needs a C toolchain per target and an FFI crate; rejected.

## R2. Interface: plain WASI core module, component model or Extism

**Decision**: a core module with putio-sync's own small ABI: JSON text through linear
memory, three imports (`putio.call`, `putio.reply`, `putio.fail`) and exports named like the
Lua functions. A WASI preview 1 shim with no real access lets ordinary `wasm32-wasip1`
builds (Go, TinyGo, Rust) load. See [contracts/wasm-abi.md](contracts/wasm-abi.md).

**Rationale**:

- One host import with JSON mirrors the Lua API one to one (`{"fn": "http.request", ...}`),
  so the host dispatch is shared with Lua (`host.rs`) and a binding in a new language is a
  page of code (the Go and Rust examples each have one).
- Core modules are what every toolchain emits today, including stock Go
  (`-buildmode=c-shared` with `//go:wasmexport`, Go 1.24+), TinyGo and Rust
  (`wasm32-unknown-unknown` or `wasm32-wasip1`). JavaScript works through Javy-style
  builds.
- The component model needs wasmtime (wasmi has no component support), WIT tooling per
  language, and its Go support goes through TinyGo-only `wit-bindgen-go`. It would tie the
  runtime choice to the largest option.
- Extism has a good multi-language PDK, but its host SDK is built on wasmtime (size, as
  above), and its ABI (extism kernel memory, `extism:host/env`) would mean plugin authors
  learn Extism's model instead of the Lua API they already have docs for.

**Alternatives considered**: component model with WIT (rejected: runtime and tooling);
Extism (rejected: wasmtime underneath, different API shape); one import per API call
(rejected: more code per language binding for no gain at these call rates).

## R3. Time limits on an interpreter

**Decision**: fuel in slices of 4,000,000 instructions. When a slice runs out, the host
checks the call's deadline: past it, the call fails with `time limit reached (N s)`;
otherwise it refuels and resumes.

**Rationale**: wasmi has no epoch interruption; fuel is cheap and resumable. Checking the
wall clock only between slices keeps overhead small and still stops a loop within a few
milliseconds of the limit (SC-002 allows 0.5 s).

**Findings during implementation**:

- Lazy compilation (wasmi's default) charges fuel for compiling a function on its first
  call, and running out there is not resumable. The engine compiles eagerly at load.
- Calling `putio_alloc` from inside a host call nests a guest call in a resumable call,
  which wasmi refuses when fuel runs out. So the host keeps each reply and the plugin
  copies it out with `putio.reply` after `putio.call` returns its length: no plugin code
  ever runs inside a host call.
- Without `auto-dispatch`, wasmi 2 uses tail-call dispatch even at opt-level 0, which
  overflowed the 8 MiB worker stack in debug builds and tests.

## R4. Memory limits

**Decision**: a `ResourceLimiter` that allows memories and tables to grow to
`memory_mb` in total; a refused grow makes `memory.grow` return -1, and a module that then
traps fails with `memory limit reached (N MiB)`.

## R5. File permissions

**Decision**: `[permissions.files]` with `read`, `write`, `read_from_config`,
`write_from_config`; `paths_needed` as sorted `r:`/`w:` strings; `approved_paths` written on
the enable-and-save, exactly like hosts. Checks canonicalize the deepest existing parent,
so `..`, symlinks and relative paths cannot escape. The same `putio.fs` calls are added to
Lua, since the host API is shared.

**Alternatives considered**: WASI preopens (rejected: Lua would not get them, and the
shim would then give real file access through a second door); a per-folder prompt
(rejected: the existing approval step is the save).

## R6. Install

**Decision**: `POST /plugins/install` with a URL, a base64 module, or a manifest plus
entry. A module carries its `plugin.toml` in a `putio-plugin` custom section, so one file
installs. Files are written to a hidden folder (skipped by discovery) and renamed into
place; replacing moves the old folder aside first. Downloads: http(s), 16 MiB, 30 s, at
most 3 redirects on the same host.

**Alternatives considered**: zip/tar packages (rejected: another parser and path-escape
surface for two files); a plugin catalogue (out of scope).
