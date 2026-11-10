# Flatpak Package for putio-sync

This directory contains configuration and scripts for building and distributing putio-sync as a Flatpak package.

## Status

putio-sync isn't on Flathub, and isn't likely to be: Flathub doesn't accept console
software, and putio-sync is a daemon with a web UI rather than a desktop app. The manifest
builds a local bundle (`make build-flatpak`, or the manual `flatpak.yml` workflow) for anyone who
wants putio-sync sandboxed. The snap, the `.deb` files and the static binaries are the
supported Linux packages.

## Installation (from a bundle)

```bash
flatpak install --user putio-sync.flatpak
```

Run the application:

```bash
flatpak run io.github.devclinton.putio-sync --version
```

## Flatpak Configuration

### Manifest Location

The Flatpak manifest is located at the project root:
- `io.github.devclinton.putio-sync.yaml`

### Runtime Details

- **App ID**: `io.github.devclinton.putio-sync`
- **Runtime**: `org.freedesktop.Platform` version 23.08
- **SDK**: `org.freedesktop.Sdk` with Rust extension
- **Command**: `putio-sync`

### Permissions (finish-args)

- **--share=network**: Network access for Put.io API
- **--filesystem=home**: Access to home directory for downloads
- **--filesystem=xdg-download**: Access to Downloads folder
- **--filesystem=/media**: Access to mounted media
- **--filesystem=/run/media**: Access to removable media

## Local Development

### Prerequisites

Install Flatpak and flatpak-builder:

```bash
# Fedora
sudo dnf install flatpak flatpak-builder

# Ubuntu/Debian
sudo apt-get install flatpak flatpak-builder

# Arch Linux
sudo pacman -S flatpak flatpak-builder
```

Add Flathub repository:

```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

Install required runtime and SDK:

```bash
flatpak install flathub org.freedesktop.Platform//23.08
flatpak install flathub org.freedesktop.Sdk//23.08
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//23.08
```

### Generate Cargo Sources

For offline builds (required for Flathub), generate the cargo sources file:

```bash
bash packaging/flatpak/generate-cargo-sources.sh
```

This creates `cargo-sources.json` with all Cargo dependencies.

After generation, uncomment the cargo-sources line in the manifest:

```yaml
sources:
  - type: dir
    path: .
  - cargo-sources.json  # Uncomment this line
```

### Building Locally

Build the Flatpak:

```bash
# From project root
flatpak-builder --force-clean --user --install build-dir io.github.devclinton.putio-sync.yaml
```

Options:
- `--force-clean`: Remove previous build directory
- `--user`: Install for current user only
- `--install`: Install after building

### Testing

Run the installed Flatpak:

```bash
flatpak run io.github.devclinton.putio-sync --version
flatpak run io.github.devclinton.putio-sync login
flatpak run io.github.devclinton.putio-sync files list
```

Create a desktop alias for convenience:

```bash
alias putio-sync='flatpak run io.github.devclinton.putio-sync'
```

### Accessing Flatpak Data

Configuration and data are stored in:

```bash
# Config location
~/.var/app/io.github.devclinton.putio-sync/config/putio-sync/config.toml

# Data directory
~/.var/app/io.github.devclinton.putio-sync/data/putio-sync/

# View all app data
ls -la ~/.var/app/io.github.devclinton.putio-sync/
```

### Debugging

Run with verbose output:

```bash
flatpak run --verbose io.github.devclinton.putio-sync
```

Access the app's filesystem:

```bash
flatpak run --command=sh io.github.devclinton.putio-sync
```

Override permissions for testing:

```bash
flatpak run --filesystem=host io.github.devclinton.putio-sync
```

## Automated Builds

The GitHub Actions workflow (`.github/workflows/flatpak.yml`) automates:

1. Cargo sources generation
2. Flatpak building
3. Bundle creation
4. Upload to GitHub releases

## Manifest Structure Explained

```yaml
app-id: io.github.devclinton.putio-sync
  # Reverse DNS format application ID

runtime: org.freedesktop.Platform
  # Base runtime providing system libraries

sdk: org.freedesktop.Sdk
  # SDK for building the application

sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
  # Rust toolchain for building

finish-args:
  # Permissions and filesystem access

modules:
  # Build instructions for the application
  - name: putio-sync
    buildsystem: simple
    # Custom build commands using cargo
```

## Updating for New Releases

When releasing a new version:

1. Generate new cargo-sources.json:
   ```bash
   bash packaging/flatpak/generate-cargo-sources.sh
   ```

2. Build the bundle (`make build-flatpak`, or run `flatpak.yml` by hand)

## Size Optimization

Typical Flatpak size: ~15-20 MB (compressed)

Optimization tips:
- Strip binaries (already done in build)
- Minimize stage-packages
- Use shared runtimes

## Multi-Architecture Support

Flatpak supports multiple architectures:
- x86_64
- aarch64 (ARM64)

flatpak-builder builds for the host architecture.

## Testing on Different Distributions

Test the Flatpak on:
- Fedora Silverblue
- Ubuntu
- Debian
- Arch Linux
- openSUSE

## Troubleshooting

### Build fails: "Rust not found"

Ensure Rust SDK extension is installed:
```bash
flatpak install flathub org.freedesktop.Sdk.Extension.rust-stable//23.08
```

### Build fails: "cargo sources not found"

Generate cargo-sources.json:
```bash
bash packaging/flatpak/generate-cargo-sources.sh
```

### Permission denied errors

Check finish-args in the manifest and add necessary permissions.

### App can't access files

Verify filesystem permissions:
```bash
flatpak override --user --filesystem=home io.github.devclinton.putio-sync
```

## References

- [Flatpak Documentation](https://docs.flatpak.org/)
- [Flathub](https://flathub.org/)
- [Flatpak Builder Tools](https://github.com/flatpak/flatpak-builder-tools)
- [App Requirements](https://github.com/flathub/flathub/wiki/App-Requirements)
- [Rust SDK Extension](https://github.com/flathub/org.freedesktop.Sdk.Extension.rust-stable)
