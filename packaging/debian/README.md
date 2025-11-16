# Debian Package for putio-sync

This directory contains the Debian package structure for building `.deb` packages of putio-sync.

## Package Structure

```
packaging/debian/
├── DEBIAN/
│   ├── control              # Package metadata
│   ├── copyright            # Copyright and license information
│   ├── postinst             # Post-installation script
│   └── prerm                # Pre-removal script
├── usr/
│   ├── bin/
│   │   └── putio-sync       # Binary (copied during build)
│   └── share/
│       ├── doc/
│       │   └── putio-sync/
│       │       └── changelog.Debian  # Debian changelog
│       └── man/
│           └── man1/
│               └── putio-sync.1      # Man page
└── README.md                # This file
```

## Building the Package

### Prerequisites

On Debian/Ubuntu:
```bash
sudo apt-get install build-essential dpkg-dev
```

### Build Script

Use the provided build script (to be created in Section 3.2):
```bash
bash packaging/debian/build-deb.sh 0.1.1 amd64
```

### Manual Build

1. Build the binary:
   ```bash
   cargo build --release --target x86_64-unknown-linux-gnu
   ```

2. Copy binary to package structure:
   ```bash
   cp target/x86_64-unknown-linux-gnu/release/putio-sync \
      packaging/debian/usr/bin/
   ```

3. Compress man page and changelog:
   ```bash
   gzip -9 -n -c packaging/debian/usr/share/man/man1/putio-sync.1 \
      > packaging/debian/usr/share/man/man1/putio-sync.1.gz

   gzip -9 -n -c packaging/debian/usr/share/doc/putio-sync/changelog.Debian \
      > packaging/debian/usr/share/doc/putio-sync/changelog.Debian.gz
   ```

4. Set permissions:
   ```bash
   chmod 755 packaging/debian/usr/bin/putio-sync
   chmod 755 packaging/debian/DEBIAN/postinst
   chmod 755 packaging/debian/DEBIAN/prerm
   chmod 644 packaging/debian/usr/share/man/man1/putio-sync.1.gz
   ```

5. Build the package:
   ```bash
   dpkg-deb --build packaging/debian putio-sync_0.1.1_amd64.deb
   ```

## Installation

### From .deb file

```bash
sudo dpkg -i putio-sync_0.1.1_amd64.deb
```

### Resolve dependencies (if needed)

```bash
sudo apt-get install -f
```

## Testing the Package

### Verify package contents

```bash
dpkg-deb --contents putio-sync_0.1.1_amd64.deb
```

### Verify package information

```bash
dpkg-deb --info putio-sync_0.1.1_amd64.deb
```

### Test installation

```bash
# Install
sudo dpkg -i putio-sync_0.1.1_amd64.deb

# Verify
which putio-sync
putio-sync --version
man putio-sync

# Uninstall
sudo dpkg -r putio-sync
```

## Package Linting

Verify package quality with lintian:

```bash
# Install lintian
sudo apt-get install lintian

# Check package
lintian putio-sync_0.1.1_amd64.deb

# Verbose check
lintian -v putio-sync_0.1.1_amd64.deb
```

## Architecture Support

The package can be built for multiple architectures:

- **amd64** (x86_64): Most common desktop/server
- **arm64** (aarch64): ARM-based systems (Raspberry Pi 4+, AWS Graviton)
- **armhf** (armv7): Older ARM systems (Raspberry Pi 3)

## Multi-Architecture Builds

To build for ARM64 on x86_64:

```bash
# Install cross-compilation tools
sudo apt-get install gcc-aarch64-linux-gnu

# Add Rust target
rustup target add aarch64-unknown-linux-gnu

# Configure cargo for cross-compilation
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Build
cargo build --release --target aarch64-unknown-linux-gnu

# Build package
bash packaging/debian/build-deb.sh 0.1.1 arm64
```

## Debian Repository

For distribution via a Debian repository, packages should be:

1. Signed with GPG key
2. Included in a Packages index
3. Served via HTTP/HTTPS

Example repository structure:
```
deb-repo/
├── pool/
│   └── main/
│       └── p/
│           └── putio-sync/
│               ├── putio-sync_0.1.1_amd64.deb
│               └── putio-sync_0.1.1_arm64.deb
└── dists/
    └── stable/
        └── main/
            └── binary-amd64/
                └── Packages.gz
```

## Ubuntu PPA

For Ubuntu distribution via Personal Package Archive (PPA), see Section 3.3 of the workplan.

## Troubleshooting

### Package build fails

- Verify all directories exist
- Check file permissions
- Ensure binary is built for correct architecture

### Installation fails

- Check dependencies
- Verify package architecture matches system
- Check for conflicts with existing installations

### lintian warnings

Common warnings and fixes:
- **no-copyright-file**: Ensure copyright file exists in DEBIAN/
- **binary-without-manpage**: Ensure man page is compressed and in place
- **maintainer-script-empty**: Remove empty maintainer scripts

## Related Files

- Build script: `packaging/debian/build-deb.sh` (Section 3.2)
- GitHub workflow: `.github/workflows/debian.yml` (Section 3.4)

## References

- [Debian New Maintainers' Guide](https://www.debian.org/doc/manuals/maint-guide/)
- [Debian Policy Manual](https://www.debian.org/doc/debian-policy/)
- [dpkg-deb Manual](https://man7.org/linux/man-pages/man1/dpkg-deb.1.html)
