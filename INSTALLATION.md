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
- [Docker](#docker)
- [Home servers (Unraid, Umbrel, CasaOS, TrueNAS, Runtipi)](#home-servers)
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

### Windows Package Manager (Recommended)

**Requirements**: Windows 10 1809+ or Windows 11

```powershell
# Install
winget install putio.putio-sync

# Verify installation
putio-sync --version
```

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

```powershell
winget upgrade putio.putio-sync
```

### Uninstall

```powershell
winget uninstall putio.putio-sync
```

---

## Linux

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

putio-sync runs as a container, so home-server app stores install the same image:

- **Unraid**: search for putio-sync in Community Applications (once listed), or add
  [`packaging/unraid/putio-sync.xml`](packaging/unraid/putio-sync.xml) as a template URL.
- **Umbrel** and **CasaOS / ZimaOS**: install from the app store once listed. The
  packages are in [`packaging/umbrel/`](packaging/umbrel/) and [`packaging/casaos/`](packaging/casaos/).
- **TrueNAS SCALE**, **Runtipi**, **Portainer**, **Dockge**: add a custom app from
  [`packaging/docker-compose/compose.yaml`](packaging/docker-compose/compose.yaml).

On first start the container log shows a one-time setup code. Open `http://<host>:15432/`,
enter it (or the web UI token you set), and choose `/downloads` as the download folder.

---

## Verification

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

### Winget not found

Install Windows Package Manager from Microsoft Store or:
```powershell
# Check Windows version (need 1809+)
winver

# Install App Installer from Microsoft Store
# Or download from: https://aka.ms/getwinget
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
