# Snap Package for putio-sync

This directory contains the Snap package configuration for putio-sync, enabling installation across multiple Linux distributions.

## Installation (for users)

Once published to the Snap Store, users can install putio-sync with:

```bash
sudo snap install putio-sync
```

For development/testing with local builds:

```bash
sudo snap install --dangerous putio-sync_0.1.1_amd64.snap
```

## Snap Configuration

### Base and Confinement

- **Base**: `core22` (Ubuntu 22.04 LTS)
- **Confinement**: `strict` (sandboxed for security)
- **Grade**: `stable` (production-ready)

### Supported Architectures

- **amd64** (x86_64)
- **arm64** (aarch64)
- **armhf** (ARMv7)

### Plugs (Permissions)

The snap requires the following interfaces:

- **home**: Access to user's home directory
- **network**: Network access for Put.io API
- **network-bind**: Bind to network ports (for server mode)
- **removable-media**: Access to external drives (for downloads)

### Environment

- **PUTIO_SYNC_CONFIG**: Automatically set to `$SNAP_USER_DATA/config.toml`

## Local Development

### Prerequisites

Install snapcraft:

```bash
sudo snap install snapcraft --classic
```

### Building Locally

Build the snap:

```bash
snapcraft
```

This creates: `putio-sync_0.1.1_amd64.snap`

### Testing Locally

Install the locally built snap:

```bash
sudo snap install --dangerous putio-sync_0.1.1_amd64.snap
```

### Running

After installation:

```bash
putio-sync --version
putio-sync login
putio-sync files list
```

### Accessing Snap Data

Configuration and data are stored in:

```bash
# Configuration
~/snap/putio-sync/current/config.toml

# Download database
~/snap/putio-sync/current/.local/share/putio-sync/downloads.db

# View snap data
ls ~/snap/putio-sync/current/
```

### Connecting Interfaces

The snap auto-connects most interfaces. For removable media:

```bash
# Connect removable-media interface (if not auto-connected)
sudo snap connect putio-sync:removable-media

# View connected interfaces
snap connections putio-sync
```

## Cleaning Build Artifacts

Clean snapcraft build artifacts:

```bash
snapcraft clean
```

Remove all build artifacts:

```bash
snapcraft clean --destructive-mode
```

## Publishing to Snap Store

### Prerequisites

1. Create Snap Store account: https://snapcraft.io/account
2. Register the snap name:
   ```bash
   snapcraft register putio-sync
   ```

### Manual Upload

Build and upload:

```bash
# Build the snap
snapcraft

# Login to Snap Store
snapcraft login

# Upload to edge channel
snapcraft upload putio-sync_0.1.1_amd64.snap --release edge

# Promote to stable
snapcraft promote putio-sync --from-channel edge --to-channel stable
```

### Channels

Snap Store uses risk levels:

- **stable**: Production releases
- **candidate**: Release candidates
- **beta**: Beta testing
- **edge**: Development builds

## Automated Builds

The GitHub Actions workflow (`.github/workflows/snap.yml`) automatically:

1. Builds the snap on release
2. Uploads to Snap Store
3. Publishes to the stable channel

## Troubleshooting

### Build Errors

#### Missing dependencies

```bash
# Install build dependencies
sudo apt-get install snapd snapcraft
```

#### Rust not found

The snap uses the `rust` plugin which automatically installs Rust. If issues persist:

```bash
snapcraft clean
snapcraft
```

### Runtime Errors

#### Permission denied

Connect the required interface:

```bash
sudo snap connect putio-sync:removable-media
```

#### Config file not found

The snap uses `$SNAP_USER_DATA` for config:

```bash
# Snap config location
~/snap/putio-sync/current/config.toml

# Create config with login
putio-sync login
```

### Confinement Issues

If strict confinement causes issues, you can test with devmode:

```bash
sudo snap install --devmode putio-sync_0.1.1_amd64.snap
```

**Note**: Devmode snaps cannot be published to the Snap Store.

## Size Optimization

The snap is optimized for size:

- Binary stripping enabled
- Only necessary stage packages included
- Typical size: ~10-15 MB

Further optimization:

```yaml
# In snapcraft.yaml, add:
build-attributes:
  - enable-patchelf
```

## Multi-Architecture Builds

### Building for ARM

On x86_64, build for ARM using snapcraft remote-build:

```bash
snapcraft remote-build
```

This builds for all supported architectures in the cloud.

### Manual Cross-Build

For local ARM64 build:

```bash
# Install QEMU for cross-compilation
sudo apt-get install qemu-user-static

# Build for ARM64
snapcraft --target-arch=arm64
```

## Snap Store Metrics

After publishing, view metrics:

- Downloads: https://snapcraft.io/putio-sync/metrics
- Channels: `snapcraft status putio-sync`
- Releases: `snapcraft releases putio-sync`

## Testing Installation

### Test on Multiple Distributions

Snaps work across many distributions:

- Ubuntu (16.04+)
- Debian (9+)
- Fedora
- Arch Linux
- openSUSE
- Manjaro
- Linux Mint

### Test in Container

Test in a clean Ubuntu container:

```bash
docker run -it ubuntu:22.04
apt update && apt install -y snapd
snap install putio-sync
```

## Updating the Snap

When releasing a new version:

1. Update `version` in `snap/snapcraft.yaml`
2. Commit and push changes
3. Create a GitHub release
4. Workflow automatically builds and publishes

Or manually:

```bash
# Update version
sed -i "s/version: .*/version: '0.1.2'/" snap/snapcraft.yaml

# Build and upload
snapcraft
snapcraft upload putio-sync_0.1.2_amd64.snap --release stable
```

## Related Documentation

- [Snapcraft Documentation](https://snapcraft.io/docs)
- [Snap Confinement](https://snapcraft.io/docs/snap-confinement)
- [Interface Management](https://snapcraft.io/docs/interface-management)
- [Publishing](https://snapcraft.io/docs/releasing-your-app)

## References

- Snap Store: https://snapcraft.io/
- Snapcraft Forum: https://forum.snapcraft.io/
- Build Service: https://build.snapcraft.io/
