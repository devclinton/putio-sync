# Installation Guide

Complete installation instructions for putio-sync across all supported platforms.

## Table of Contents

- [macOS](#macos)
- [Windows](#windows)
- [Linux](#linux)
  - [Snap](#snap-all-distributions)
  - [Debian/Ubuntu](#debianubuntu)
  - [Arch Linux](#arch-linux)
  - [Fedora/RHEL](#fedorarhel)
- [From Source](#from-source)
- [CLI-only build (lite)](#cli-only-build-lite)
- [Docker](#docker)
- [Home servers (Unraid, Umbrel, CasaOS, TrueNAS, Runtipi)](#home-servers)
- [Updating](#updating)
- [Verification](#verification)
- [Troubleshooting](#troubleshooting)

---

## macOS

### Homebrew (Recommended)

```bash
# Add the tap
brew tap devclinton/putio-sync

# Install
brew install putio-sync

# Verify installation
putio-sync --version
```

### Installer package

Download `putio-sync-X.Y.Z-macos-arm64.pkg` from the
[latest release](https://github.com/devclinton/putio-sync/releases/latest) (Apple silicon).
It installs `/usr/local/bin/putio-sync`, a **Put.io Sync Web UI** link in Applications, and,
unless you untick **Start at login** under Customize, a launchd agent that runs the daemon
now and at every login. Its log is `~/Library/Logs/putio-sync.log`; with no config yet the
daemon starts in setup mode, and the setup code is in that log.

The package isn't signed with an Apple Developer ID, so Gatekeeper blocks it the first time.
Either open it, then go to **System Settings › Privacy & Security** and click **Open
Anyway**, or clear the download flag first:

```bash
xattr -d com.apple.quarantine ~/Downloads/putio-sync-*-macos-arm64.pkg
```

To remove it: `sudo /usr/local/share/putio-sync/uninstall.sh` (your config in
`~/.putiosync` stays). A plain tarball, `putio-sync-X.Y.Z-macos-arm64.tar.gz`, is there too.

### Update

```bash
brew update
brew upgrade putio-sync
```

### Uninstall

```bash
brew uninstall putio-sync
brew untap devclinton/putio-sync
```

---

## Windows

putio-sync isn't in the Windows Package Manager (winget) yet; use the installer or the zip.

### Installer (Recommended)

Download `putio-sync-X.Y.Z-windows-x64-setup.exe` (or `-arm64-setup.exe`) from the
[latest release](https://github.com/devclinton/putio-sync/releases/latest) and run it. It
asks how putio-sync should run:

- **In the background at startup** (default): a `putio-sync` scheduled task runs the daemon
  as SYSTEM, before anyone signs in. Its config and log are in `C:\ProgramData\putio-sync`.
- **When you sign in**: a Startup shortcut runs the daemon in a minimized window, with your
  own config in `%USERPROFILE%\.putiosync`.
- **Manually**: run `putio-sync daemon` yourself.

The Start menu gets **Put.io Sync Web UI** (`http://localhost:15432/`). The installer isn't
code-signed, so SmartScreen may ask first: click **More info › Run anyway**. Uninstall from
**Settings › Apps**.

### Manual Installation

1. Download the latest release:
   - For x64: `putio-sync-vX.X.X-x86_64-pc-windows-msvc.zip`
   - For ARM64: `putio-sync-vX.X.X-aarch64-pc-windows-msvc.zip`

2. Extract the ZIP file

3. (Optional) Add to PATH:
   ```powershell
   # Add to user PATH
   $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
   [Environment]::SetEnvironmentVariable("Path", "$userPath;C:\path\to\putio-sync", "User")
   ```

4. Run from anywhere:
   ```powershell
   putio-sync --version
   ```

### Update

Run the newer installer over the old one, or replace `putio-sync.exe` from the newer zip.

### Uninstall

Installer: **Settings › Apps › Put.io Sync › Uninstall**. Zip: delete the folder and remove
it from PATH.

---

## Linux

<a id="snap"></a>

### Snap (All Distributions)

**Works on**: Ubuntu, Debian, Fedora, Arch, openSUSE, Manjaro, and more.

```bash
# Install
sudo snap install putio-sync

# Run
putio-sync --version

# Or use the snap command
snap run putio-sync --version
```

#### Permissions

Connect removable media (if needed):
```bash
sudo snap connect putio-sync:removable-media
```

#### Update

```bash
sudo snap refresh putio-sync
```

#### Uninstall

```bash
sudo snap remove putio-sync
```

---

### Standalone binary (any distribution)

`putio-sync-X.Y.Z-linux-x86_64.tar.gz` and `-linux-aarch64.tar.gz` hold a single static
binary with the web UI built in. It needs no libraries, so it runs on any distribution,
Alpine and NAS firmware included. Subtitle transcription also needs `ffmpeg` on `PATH`
(or `transcription.ffmpeg_path`); nothing else does.

```bash
curl -LO https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync-X.Y.Z-linux-x86_64.tar.gz
tar -xzf putio-sync-X.Y.Z-linux-x86_64.tar.gz
sudo install putio-sync-X.Y.Z-linux-x86_64/putio-sync /usr/local/bin/
putio-sync --version
```

---

<a id="debian"></a>

### Debian/Ubuntu

**Supported**: Ubuntu 20.04+, Debian 10+

```bash
# Download the .deb package
wget https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync_VERSION_amd64.deb

# Install
sudo dpkg -i putio-sync_VERSION_amd64.deb

# If dependencies are missing
sudo apt-get install -f

# Verify
putio-sync --version
```

#### ARM64

```bash
wget https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync_VERSION_arm64.deb
sudo dpkg -i putio-sync_VERSION_arm64.deb
```

#### Update

Download and install the new .deb package.

#### Uninstall

```bash
sudo dpkg -r putio-sync
```

---

### Arch Linux

```bash
# From source using cargo
cargo install putio-sync-cli

# Or create a PKGBUILD (community contribution welcome!)
```

---

### Fedora/RHEL

```bash
# Using Snap
sudo snap install putio-sync

# Or from source
cargo install putio-sync-cli
```

---

## From Source

### Requirements

- Rust 1.96 (pinned in `rust-toolchain.toml`)
- `pkg-config`
- `libssl-dev` (Debian/Ubuntu) or `openssl-devel` (Fedora)

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Build and Install

```bash
# Clone the repository
git clone https://github.com/devclinton/putio-sync.git
cd putio-sync

# Build and install
cargo install --path putio-sync-cli

# Or install from crates.io
cargo install putio-sync-cli
```

### Development Build

```bash
# Build in debug mode
cargo build

# Build in release mode
cargo build --release

# Run directly
cargo run -- --help
```

---

## CLI-only build (lite)

The CLI-only builds are the same `putio-sync` command without the daemon, the HTTP API
and web UI, the setup wizard, metrics or the archive commands. They keep `login`,
`files`, `download`, `stat`, `history`, `queue`, `transfers`, `status`, `health`,
`debug` and `doctor`, and use the same config file. Use one to manage Put.io from a
terminal or a script without running a server. There are two:

| Edition | Event hooks | Lua/WASM plugins |
|---------|-------------|------------------|
| `putio-sync-lite` | yes | no |
| `putio-sync-lite-plugins` | yes | yes |

Hooks and plugins run on the events a command emits (a download finishing or failing,
low Put.io storage), and the command waits up to two minutes for them before it exits.
With plugins, `download` also applies their filters and rename and folder rules. The
transfer events come from the daemon, so a lite build never sends them. In `doctor`,
both skip the Tailscale check, and `putio-sync-lite` also skips the plugins check.

Each release has these tarballs and zips next to the full one, for each edition
(`lite` or `lite-plugins`): `putio-sync-lite-X.Y.Z-linux-x86_64.tar.gz`,
`-linux-aarch64.tar.gz`, `-macos-arm64.tar.gz`, and
`putio-sync-lite-vX.Y.Z-x86_64-pc-windows-msvc.zip` (and `aarch64-`). The binary
inside is still named `putio-sync`.

```bash
curl -LO https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync-lite-X.Y.Z-linux-x86_64.tar.gz
tar -xzf putio-sync-lite-X.Y.Z-linux-x86_64.tar.gz
sudo install putio-sync-lite-X.Y.Z-linux-x86_64/putio-sync /usr/local/bin/
putio-sync login
```

From source, pick the features (leave them out for a CLI with neither):

```bash
cargo build --release -p putio-sync-cli --no-default-features --features hooks,plugins
```

---

## Docker

```bash
# Pull the image
docker pull ghcr.io/devclinton/putio-sync:latest

# Run
docker run -it --rm \
  -v $HOME/.config/putio-sync:/config \
  -v $HOME/Downloads:/downloads \
  ghcr.io/devclinton/putio-sync:latest \
  --help

# Daemon mode
docker run -d \
  --name putio-sync \
  -v $HOME/.config/putio-sync:/config \
  -v $HOME/Downloads:/downloads \
  ghcr.io/devclinton/putio-sync:latest \
  server --daemon
```

See the main README for Docker Compose examples.

### Hardened images

The same repository also has images built on
[Docker Hardened Images](https://dhi.io): the static binary on a distroless base with no
shell or package manager. They take the same volumes, ports, user (99:100) and command as
the default image, so switching is a tag change.

| Tag | What's in it |
|-----|--------------|
| `dhi-X.Y.Z`, `dhi` | putio-sync only |
| `dhi-ffmpeg-X.Y.Z`, `dhi-ffmpeg` | plus static `ffmpeg`/`ffprobe`, for subtitle transcription |
| `dev-X.Y.Z`, `dev` | putio-sync on a hardened Alpine with a shell and `apk`, for debugging |
| `dev-ffmpeg-X.Y.Z`, `dev-ffmpeg` | the dev image plus `ffmpeg`/`ffprobe` |

`latest`, `X.Y.Z` and `main` stay the Alpine image with ffmpeg.

### Media server (DLNA)

TVs and players find putio-sync with multicast discovery, which Docker's default bridge
network hides. To use the media server ([README](README.md#media-server)), run the container
on the host network. In Docker Compose:

```yaml
services:
  putio-sync:
    image: ghcr.io/devclinton/putio-sync:latest
    network_mode: host    # needed so players can find the media server
    volumes:
      - ./config:/config
      - /path/to/downloads:/downloads
```

With `docker run`, add `--network host`. Then open port 8200 (TCP) and 1900 (UDP) on the
host firewall for your home network only. Without host networking, Settings › Media server
says "No LAN interface: players can't find it".

---

## Home servers

<a id="unraid"></a>

putio-sync runs as a container, so home-server app stores install the same image:

- **Unraid**: search for putio-sync in Community Applications (once listed), or add
  [`packaging/unraid/putio-sync.xml`](packaging/unraid/putio-sync.xml) as a template URL.
- **Umbrel** and **CasaOS / ZimaOS**: install from the app store once listed. The
  packages are in [`packaging/umbrel/`](packaging/umbrel/) and [`packaging/casaos/`](packaging/casaos/).
- **TrueNAS SCALE**, **Runtipi**, **Portainer**, **Dockge**: add a custom app from
  [`packaging/docker-compose/compose.yaml`](packaging/docker-compose/compose.yaml).

On first start the container log shows a one-time setup code. Open `http://<host>:15432/`,
enter it (or the web UI token you set), and choose `/downloads` as the download folder.

The config file and the state database (`.putiosync.db`) live in `/config`; keep that
volume on local storage. Versions up to 0.2.x kept the database in `/downloads`; the first
start of a newer image moves it to `/config` by itself.

---

## Updating

putio-sync looks for a newer release on GitHub about a minute after it starts, then once a
day. The request sends only a User-Agent with the running version. When a newer release is
out, the **What's new** page shows it with its release notes, a link to the release, and
**Check now** and **Dismiss** buttons. The notice also says how this copy was installed and
how that install gets the new version:

| Installed with | How it updates |
|---|---|
| Binary from GitHub Releases (Linux, macOS, Windows) | **Update now** downloads a small patch from the running version when the release has one, or else the full binary. It checks the download against the signed `SHA256SUMS`, replaces the binary and restarts. If transfers are running, it asks first; they resume after the restart. If the new version doesn't start cleanly within 90 seconds, the old binary and its database are put back. |
| macOS `.pkg`, Windows `-setup.exe` | **Update now** downloads the new installer, checks it, and opens it, or tells you where it is. |
| Homebrew | `brew upgrade putio-sync` |
| Snap | `sudo snap refresh putio-sync` |
| Debian/Ubuntu `.deb` | Download and install the new `.deb`. |
| Docker | `docker pull` for your image and tag, then recreate the container. |
| Unraid, CasaOS, Umbrel | The app store's own update. |

Package managers and containers keep their own files, so putio-sync never replaces itself
there: the notice shows the command or step with a **Copy** button. If it names the wrong
install, set `PUTIO_SYNC_INSTALL`; `GET /status` reports what it detected as
`install_channel`.

**Install updates automatically** (Settings › General › Updates, `[updates] auto_install`,
off by default) is offered only for a binary from GitHub Releases. With it on, a new release
installs once no transfer is running, checking every minute. A version that was rolled back
is never installed again automatically, though **Update now** can still install it.

To stop checking, turn off **Check for updates** in Settings › General › Updates, or set:

```toml
[updates]
check = false
```

**Older versions:** 0.2.x and earlier don't check for updates. 0.3.0 and 0.4.0 show the
notice but can't install it. Update those by hand once, the way you installed them; after
that, a binary from GitHub Releases updates itself.

---

## Verification

Every release has a `SHA256SUMS` file covering its binaries and installers:

```bash
sha256sum --ignore-missing -c SHA256SUMS     # macOS: shasum -a 256 --ignore-missing -c SHA256SUMS
```

After installation, verify putio-sync is working:

```bash
# Check version
putio-sync --version

# Check help
putio-sync --help

# Authenticate
putio-sync login

# List files
putio-sync files list
```

---

## Troubleshooting

### Command not found

**macOS/Linux**:
```bash
# Check if in PATH
which putio-sync

# Add to PATH (bash)
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Add to PATH (zsh)
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

**Windows**:
- Restart terminal/PowerShell after installation
- Check PATH environment variable

### Permission denied (Snap)

```bash
# Connect interfaces
sudo snap connect putio-sync:home
sudo snap connect putio-sync:removable-media
```

### SSL/TLS errors

**Debian/Ubuntu**:
```bash
sudo apt-get install ca-certificates
sudo update-ca-certificates
```

**Fedora/RHEL**:
```bash
sudo yum install ca-certificates
```

### Library not found (Linux)

```bash
# Debian/Ubuntu
sudo apt-get install libssl3

# Fedora
sudo dnf install openssl-libs
```

### Cargo install fails

```bash
# Update Rust
rustup update

# Install build dependencies
# Debian/Ubuntu
sudo apt-get install build-essential pkg-config libssl-dev

# Fedora
sudo dnf install gcc pkg-config openssl-devel

# Arch
sudo pacman -S base-devel openssl
```

### Homebrew installation fails

```bash
# Update Homebrew
brew update

# Reinstall
brew reinstall putio-sync
```

---

## System Requirements

### Minimum

- **OS**: Windows 10 1809+, macOS 10.15+, Linux (kernel 4.4+)
- **RAM**: 256 MB
- **Disk**: 50 MB for installation
- **Network**: Internet connection

### Recommended

- **RAM**: 1 GB (for large downloads)
- **Disk**: 1 GB (for download cache)

---

## Next Steps

After installation:

1. **Set up**: start the daemon and open `http://<host>:15432/` for the setup
   wizard (enter the setup code it prints), or run `putio-sync setup` in a
   terminal. `putio-sync login` still works for the token alone.
2. **Browse files**: `putio-sync files list`
3. **Download**: `putio-sync download`
4. **View help**: `putio-sync --help`

See the main [README.md](README.md) for usage examples and features.

---

## Support

- **Issues**: https://github.com/devclinton/putio-sync/issues
- **Documentation**: https://github.com/devclinton/putio-sync
- **Discussions**: https://github.com/devclinton/putio-sync/discussions
