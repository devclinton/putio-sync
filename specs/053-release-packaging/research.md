# Research: 053 Release Packaging

## R1. Can the Linux binary be fully static?

**Yes.** Native code in the dependency tree: `libsqlite3-sys` (sqlx's sqlite feature turns on
`bundled`), `mlua-sys` (`vendored`, Lua 5.4), `ring` (C and assembly), and `openssl-sys`
through reqwest's default `native-tls`. The first three compile from source with the musl C
compiler. Switching reqwest to `default-features = false` with `rustls-tls` +
`rustls-tls-native-roots` (plus the other defaults: `charset`, `http2`,
`macos-system-configuration`) drops `openssl`, `openssl-sys` and `native-tls` from
`Cargo.lock`. `openssl-probe` stays; it is pure Rust (it only looks for CA bundle paths).

Roots: webpki-roots compiles Mozilla's list into the binary, so a host with no CA bundle
still works; native roots add the OS store (Windows, macOS Keychain, `/etc/ssl` on Linux)
so a corporate CA keeps working.

The `*-unknown-linux-musl` targets link statically by default (`crt-static`). The old
`Dockerfile` passes `-C target-feature=-crt-static` on purpose, for alpine's dynamic musl;
`Dockerfile.dhi` doesn't, and fails the build if `readelf -d` finds any `NEEDED` entry.

ffmpeg is not a link dependency: subtitle transcription runs it as a child process found
through `transcription.ffmpeg_path` or `PATH` (`putio-sync-subtitles/src/audio.rs`).

## R2. Where the static Linux build runs

`dhi.io/rust:1.96-alpine3.22-dev` matches `rust-toolchain.toml`. It has gcc, musl's
`libc.a` and apk, but no rustup, so it can only build for its own architecture. Cross
builds would need zig or a cross gcc plus a second Rust std, none of which the image has.
So each architecture builds natively: `ubuntu-latest` for x86_64 and `ubuntu-24.04-arm` for
aarch64. The same `binary` stage of `Dockerfile.dhi` serves the release tarballs and the
images, so what was tested locally is what ships.

## R3. Docker Hardened Images

- Pulling `dhi.io/*` needs `docker login dhi.io` with a Docker account (free tier). CI uses
  `DHI_USERNAME` / `DHI_TOKEN` secrets.
- Runtime base: `dhi.io/static:20250419-debian13` (amd64 and arm64): CA bundle, tzdata,
  `/etc/passwd`, no shell, default user 65532, no entrypoint. No `alpine3.22` variant of
  `static` exists, and a static binary doesn't care which distribution the base comes from.
- Dev base: `dhi.io/alpine-base:3.22-dev` has a shell and apk, runs as root, and has no
  tzdata, so the dev stage adds it.
- **No DHI ffmpeg image** (no `dhi.io/ffmpeg` tag resolves). The `-ffmpeg` stages copy
  `/ffmpeg` and `/ffprobe` from `mwader/static-ffmpeg:8.0.1`: static musl builds for amd64
  and arm64 that need no libc, so they run on the distroless base. The maintainer asked
  about a "wheel": `imageio-ffmpeg` wheels do bundle a static ffmpeg, but they need Python
  to install and ship amd64 Linux only, so they bring nothing here. Override with
  `--build-arg FFMPEG_IMAGE=...` to use another source.
- User 99:100 (Unraid's nobody:users) as in the alpine image, so one template works for both.

## R4. Windows installer: Inno Setup, not WiX

WiX v3 is no longer preinstalled on `windows-2025` runners, and WiX's `ServiceInstall`
needs a real Windows service. putio-sync is a console program with no service control
handler, so `sc create` would fail with error 1053. Inno Setup 6 (Chocolatey on the
runner) builds both architectures from one script (`ArchitecturesAllowed=arm64` or
`x64compatible`). Ways to run it:

- **Default: a SYSTEM scheduled task at startup** (`schtasks /SC ONSTART /RU SYSTEM`). It
  runs hidden, before anyone signs in, which makes it the closest thing to a service. It
  goes through `putio-sync-service.cmd` so the log lands in `%ProgramData%\putio-sync`.
- **At sign-in**: a Startup-folder shortcut, minimized. A console program can't start
  hidden from a Run key or a sign-in task without a helper (VBScript is being removed from
  Windows), so the window stays, minimized.
- Start menu: an `.url` internet shortcut to `http://localhost:15432/`.

The arm64 binary is cross-compiled on the x64 runner (MSVC ships the ARM64 tools), as
before. It isn't run in CI.

## R5. macOS

`pkgbuild` + `productbuild` with two components: the binary (required) and the LaunchAgent
(a choice, on by default). The agent runs through `/bin/sh -c` so it can log to
`$HOME/Library/Logs`, which launchd keys can't expand. postinstall bootstraps it into the
console user's GUI domain so it runs right away. Unsigned packages: on macOS 15 and later,
Control-click › Open no longer bypasses Gatekeeper. Either use System Settings › Privacy &
Security › Open Anyway, or `xattr -d com.apple.quarantine <pkg>`. The installed binary
carries the linker's ad hoc signature (required on Apple silicon), and files installed by a
package get no quarantine flag, so the binary runs. Signing turns on when
`MACOS_INSTALLER_IDENTITY` and the P12 secrets are set.

## R6. arm64 snap

`snap/snapcraft.yaml` already lists `build-on: arm64`. The job now runs on `ubuntu-latest`
and `ubuntu-24.04-arm` (free for public repositories), and snapcraft builds the entry that
matches the host. That is simpler than Launchpad remote build, which needs a Launchpad
account and a public repository to clone. Both snaps are installed and tested on their
runner before publishing.

## R7. Why the old release chain never ran

`uses: ./.github/workflows/x.yml` needs `on: workflow_call` in x.yml. None of them had it,
so `release.yml` failed at startup. A release that `GITHUB_TOKEN` creates doesn't fire
`release: published` either. Fix: every per-platform workflow gets `workflow_call` with a
`version` input and loses its `release` trigger. `release.yml` drafts the release, the
builds upload into the draft (`gh release upload --clobber`, so re-runs are safe), and the
draft is published before Homebrew and winget, which download public assets.
