# Quickstart: Validate the Local CI Gate

## Prerequisites

- rustup installed. `rust-toolchain.toml` makes rustup install 1.96.0 with rustfmt and
  clippy automatically.

## 1. Full gate

```bash
make ci
```

Expected: the format, clippy, tests, and release build stages each print `SUCCESS`, and the
command exits 0.

## 2. Router regression

```bash
cargo test -p putio-sync-api
```

Expected: `test_create_app`, `test_router_creation`, the queue route tests, and
`server_bind` all pass.

Negative check: temporarily change a queue route back to `/*path/move_up` and rerun. The
tests must fail.

## 3. Server starts (manual)

```bash
cargo run -p putio-sync-cli -- server --port 8099
```

```bash
curl -s localhost:8099/health
```

Expected: `{"status":"ok","version":"..."}`, with no token needed. Any other route without a
token returns 401.

## 4. Remote CI shape

Read `.github/workflows/ci.yml` and `docker.yml`, and check that:

- push and PR runs trigger one `ubuntu-latest` job that calls `scripts/ci-local.sh`
- `workflow_dispatch` with `full: true` adds the macOS, Windows, and beta jobs
- `docker.yml` has no `pull_request` trigger and builds `linux/amd64` on `main`
