# Flatpak Package for putio-sync

This directory contains configuration and scripts for building and distributing putio-sync as a Flatpak package.

## Installation (for users)

Once published to Flathub, users can install putio-sync with:

```bash
flatpak install flathub com.putio.PutioSync
```

Run the application:

```bash
flatpak run com.putio.PutioSync --version
```

## Flatpak Configuration

### Manifest Location

The Flatpak manifest is located at the project root:
- `com.putio.PutioSync.yaml`

### Runtime Details

- **App ID**: `com.putio.PutioSync`
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
flatpak-builder --force-clean --user --install build-dir com.putio.PutioSync.yaml
```

Options:
- `--force-clean`: Remove previous build directory
- `--user`: Install for current user only
- `--install`: Install after building

### Testing

Run the installed Flatpak:

```bash
flatpak run com.putio.PutioSync --version
flatpak run com.putio.PutioSync login
flatpak run com.putio.PutioSync files list
```

Create a desktop alias for convenience:

```bash
alias putio-sync='flatpak run com.putio.PutioSync'
```

### Accessing Flatpak Data

Configuration and data are stored in:

```bash
# Config location
~/.var/app/com.putio.PutioSync/config/putio-sync/config.toml

# Data directory
~/.var/app/com.putio.PutioSync/data/putio-sync/

# View all app data
ls -la ~/.var/app/com.putio.PutioSync/
```

### Debugging

Run with verbose output:

```bash
flatpak run --verbose com.putio.PutioSync
```

Access the app's filesystem:

```bash
flatpak run --command=sh com.putio.PutioSync
```

Override permissions for testing:

```bash
flatpak run --filesystem=host com.putio.PutioSync
```

## Flathub Submission

### Prerequisites

1. Create Flathub account on GitHub
2. Application meets [Flathub requirements](https://github.com/flathub/flathub/wiki/App-Requirements)
3. Repository is public

### Submission Process

1. Fork the Flathub repository:
   ```bash
   # Go to https://github.com/flathub/flathub
   # Click "Fork"
   ```

2. Create a new repository for your app:
   ```bash
   # Repository name: com.putio.PutioSync
   # Initialize with the manifest file
   ```

3. Add required files to the repository:
   ```
   com.putio.PutioSync/
   ├── com.putio.PutioSync.yaml     # Flatpak manifest
   ├── cargo-sources.json            # Generated cargo dependencies
   ├── com.putio.PutioSync.appdata.xml  # AppStream metadata
   └── com.putio.PutioSync.png       # Application icon (128x128)
   ```

4. Create AppStream metadata file:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<component type="console-application">
  <id>com.putio.PutioSync</id>
  <name>putio-sync</name>
  <summary>CLI tool for syncing files from Put.io</summary>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>MIT</project_license>
  <description>
    <p>
      putio-sync is a command-line tool for downloading and managing
      files from Put.io cloud storage service.
    </p>
  </description>
  <url type="homepage">https://github.com/clintoncollins/putio-sync</url>
  <url type="bugtracker">https://github.com/clintoncollins/putio-sync/issues</url>
  <developer_name>Clinton Collins</developer_name>
  <releases>
    <release version="0.1.1" date="2025-11-14"/>
  </releases>
  <content_rating type="oars-1.1"/>
</component>
```

5. Submit pull request to Flathub

6. Address review feedback

7. Once merged, app appears on Flathub within 24 hours

## Automated Builds

The GitHub Actions workflow (`.github/workflows/flatpak.yml`) automates:

1. Cargo sources generation
2. Flatpak building
3. Bundle creation
4. Upload to GitHub releases

## Manifest Structure Explained

```yaml
app-id: com.putio.PutioSync
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

2. Update version in manifest if needed

3. Commit and push to Flathub repository

4. Flathub builds automatically

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

Flathub builds for all architectures automatically.

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
flatpak override --user --filesystem=home com.putio.PutioSync
```

## References

- [Flatpak Documentation](https://docs.flatpak.org/)
- [Flathub](https://flathub.org/)
- [Flatpak Builder Tools](https://github.com/flatpak/flatpak-builder-tools)
- [App Requirements](https://github.com/flathub/flathub/wiki/App-Requirements)
- [Rust SDK Extension](https://github.com/flathub/org.freedesktop.Sdk.Extension.rust-stable)
