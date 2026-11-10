# Implementation Plan: 053 Release Packaging

**Spec**: [spec.md](spec.md) · **Research**: [research.md](research.md)

## Summary

One code change: reqwest switches from OpenSSL to rustls, so Linux builds can be static
(research R1). Everything else is packaging and workflows. `Dockerfile.dhi` builds the
static binary and four hardened images; `release.yml` becomes the single release entry
point that drafts, fills and publishes a GitHub release.

## Layout

```text
Cargo.toml                         # reqwest: rustls-tls + native roots, no default-tls
Dockerfile.dhi                     # binary, runtime, runtime-ffmpeg, dev, dev-ffmpeg stages
.gitattributes                     # *.cmd with CRLF
packaging/
├── macos/
│   ├── build-pkg.sh               # <version> <binary> [out] -> putio-sync-<v>-macos-arm64.pkg
│   ├── distribution.xml           # choices: putio-sync (required), Start at login
│   ├── com.devclinton.putio-sync.plist
│   ├── scripts/postinstall        # bootstrap the agent for the console user
│   ├── uninstall.sh
│   └── resources/welcome.html
└── windows/
    ├── putio-sync.iss             # Inno Setup 6; /DAppVersion /DArch /DSourceExe
    └── putio-sync-service.cmd     # what the SYSTEM task runs (log redirect)
.github/workflows/
├── release.yml                    # tag -> draft -> builds -> publish -> homebrew, winget
├── release-binaries.yml           # linux x2 (Dockerfile.dhi), macos, windows x2, SHA256SUMS
├── docker-dhi.yml                 # per-arch build, push by digest, merge into prefixed tags
├── snap.yml                       # amd64 + arm64 matrix, workflow_call
└── debian.yml, homebrew.yml, winget.yml   # workflow_call, inputs.version
Makefile                           # dist-linux, dist-macos, docker-dhi
```

## Release assets

| Asset | Built on |
|-------|----------|
| `putio-sync-<v>-linux-x86_64.tar.gz`, `-linux-aarch64.tar.gz` | ubuntu-latest, ubuntu-24.04-arm |
| `putio-sync-<v>-macos-arm64.tar.gz`, `.pkg` | macos-latest |
| `putio-sync-v<v>-{x86_64,aarch64}-pc-windows-msvc.zip` (names kept for winget) | windows-latest |
| `putio-sync-<v>-windows-{x64,arm64}-setup.exe` | windows-latest |
| `putio-sync_<v>_{amd64,arm64}.deb` + `.sha256` | debian.yml (unchanged build) |
| `SHA256SUMS` over the binaries and installers | ubuntu-latest |

## Image tags (ghcr.io/devclinton/putio-sync)

| Stage | Tags on `v1.2.3` | Manual run on main |
|-------|------------------|--------------------|
| alpine `Dockerfile` (unchanged) | `1.2.3`, `1.2`, `1`, `latest` | `main` |
| `runtime` | `dhi-1.2.3`, `dhi-1.2`, `dhi` | `dhi-main` |
| `runtime-ffmpeg` | `dhi-ffmpeg-1.2.3`, `dhi-ffmpeg-1.2`, `dhi-ffmpeg` | `dhi-ffmpeg-main` |
| `dev` | `dev-1.2.3`, `dev-1.2`, `dev` | `dev-main` |
| `dev-ffmpeg` | `dev-ffmpeg-1.2.3`, `dev-ffmpeg-1.2`, `dev-ffmpeg` | `dev-ffmpeg-main` |

dhi images don't build on pushes to main (Actions minutes); run the workflow by hand.

## Constitution check

- Tests: the reqwest change is covered by the existing suite (`make ci`); the TLS path is
  exercised by running the static binary against a real HTTPS endpoint.
- Packaging is checked by building it: the static binary and images with buildx, the .pkg
  with pkgbuild/productbuild, and the workflows with actionlint.
- No change to what the NAS deploys from (`:main`).
