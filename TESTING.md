# Testing Guide

This document provides testing procedures for putio-sync package installations and functionality across all supported platforms.

## Table of Contents

- [Package Installation Testing](#package-installation-testing)
  - [macOS (Homebrew)](#macos-homebrew)
  - [Windows (Winget)](#windows-winget)
  - [Linux (Snap)](#linux-snap)
  - [Linux (Flatpak)](#linux-flatpak)
  - [Linux (Debian)](#linux-debian)
- [Functionality Testing](#functionality-testing)
- [Pre-Release Testing Checklist](#pre-release-testing-checklist)

---

## Package Installation Testing

### macOS (Homebrew)

**Test on**:
- macOS 13+ (x64)
- macOS 14+ (ARM64)

**Procedure**:
```bash
# 1. Install from tap
brew tap devclinton/putio-sync
brew install putio-sync

# 2. Verify installation
which putio-sync
putio-sync --version

# 3. Test basic functionality
putio-sync --help
putio-sync login --help

# 4. Test uninstallation
brew uninstall putio-sync
which putio-sync  # Should return nothing

# 5. Reinstall for further testing
brew install putio-sync
```

**Expected Results**:
- ✅ Installation completes without errors
- ✅ Binary is in PATH
- ✅ Version matches release version
- ✅ Help commands display correctly
- ✅ Uninstall removes binary completely

---

### Windows (Winget)

**Test on**:
- Windows 10 21H2+ (x64)
- Windows 11 (x64, ARM64)

**Procedure**:
```powershell
# 1. Install via Winget
winget install putio.putio-sync

# 2. Verify installation
where.exe putio-sync
putio-sync --version

# 3. Test basic functionality
putio-sync --help
putio-sync login --help

# 4. Test uninstallation
winget uninstall putio.putio-sync
where.exe putio-sync  # Should return nothing

# 5. Reinstall for further testing
winget install putio.putio-sync
```

**Expected Results**:
- ✅ Installation completes without SmartScreen warnings (after code signing)
- ✅ Binary is accessible from Command Prompt/PowerShell
- ✅ Version matches release version
- ✅ Help commands display correctly
- ✅ Uninstall removes binary completely

---

### Linux (Snap)

**Test on**:
- Ubuntu 22.04, 24.04 (x64, ARM64)
- Fedora 38+ (x64)
- Arch Linux (x64)

**Procedure**:
```bash
# 1. Install from Snap Store
sudo snap install putio-sync

# 2. Verify installation
which putio-sync
putio-sync --version

# 3. Test permissions
# Try accessing home directory
mkdir -p ~/putio-test
putio-sync --help

# If permission issues:
sudo snap connect putio-sync:home
sudo snap connect putio-sync:removable-media

# 4. Test basic functionality
putio-sync --help
putio-sync login --help

# 5. Test uninstallation
sudo snap remove putio-sync
which putio-sync  # Should return nothing

# 6. Reinstall for further testing
sudo snap install putio-sync
```

**Expected Results**:
- ✅ Installation completes without errors
- ✅ Binary is accessible via `putio-sync`
- ✅ Version matches release version
- ✅ Snap confinement works correctly
- ✅ Interfaces connect properly
- ✅ Uninstall removes snap completely

---

### Linux (Flatpak)

**Test on**:
- Ubuntu 22.04, 24.04 (x64)
- Fedora 38+ (x64)
- Arch Linux (x64)

**Procedure**:
```bash
# 1. Install from Flathub (if published) or bundle
flatpak install flathub com.putio.PutioSync
# OR from bundle:
flatpak install putio-sync-VERSION.flatpak

# 2. Verify installation
flatpak list | grep putio
flatpak run com.putio.PutioSync --version

# 3. Create alias for convenience
alias putio-sync='flatpak run com.putio.PutioSync'

# 4. Test basic functionality
flatpak run com.putio.PutioSync --help
flatpak run com.putio.PutioSync login --help

# 5. Test permissions
flatpak run com.putio.PutioSync files list

# 6. Test uninstallation
flatpak uninstall com.putio.PutioSync
flatpak list | grep putio  # Should return nothing

# 7. Reinstall for further testing
flatpak install flathub com.putio.PutioSync
```

**Expected Results**:
- ✅ Installation completes without errors
- ✅ Application runs via `flatpak run com.putio.PutioSync`
- ✅ Version matches release version
- ✅ Sandboxing works correctly
- ✅ Filesystem access works as expected
- ✅ Uninstall removes Flatpak completely

---

### Linux (Debian)

**Test on**:
- Ubuntu 20.04, 22.04, 24.04 (amd64, arm64)
- Debian 11, 12 (amd64, arm64)

**Procedure**:
```bash
# 1. Download .deb package
wget https://github.com/devclinton/putio-sync/releases/latest/download/putio-sync_VERSION_amd64.deb

# 2. Install package
sudo dpkg -i putio-sync_VERSION_amd64.deb

# 3. Fix dependencies if needed
sudo apt-get install -f

# 4. Verify installation
which putio-sync
putio-sync --version
dpkg -l | grep putio-sync

# 5. Test basic functionality
putio-sync --help
man putio-sync  # Check man page

# 6. Test uninstallation
sudo dpkg -r putio-sync
which putio-sync  # Should return nothing
dpkg -l | grep putio-sync  # Should return nothing

# 7. Reinstall for further testing
sudo dpkg -i putio-sync_VERSION_amd64.deb
```

**Expected Results**:
- ✅ Installation completes without dependency errors
- ✅ Binary is in PATH (`/usr/bin/putio-sync`)
- ✅ Man page is installed and accessible
- ✅ Version matches release version
- ✅ Uninstall removes all files cleanly
- ✅ Postinst and prerm scripts execute without errors

---

## Functionality Testing

After installation on each platform, test core functionality:

### Authentication
```bash
# Test login flow
putio-sync login
# Follow OAuth flow in browser
# Verify token is saved to ~/.putiosync/config.toml
```

**Expected**:
- ✅ Browser opens to Put.io OAuth page
- ✅ Token is saved correctly
- ✅ Config file has correct permissions (0600)

### File Operations
```bash
# List files
putio-sync files list

# List specific folder
putio-sync files list --parent-id 0

# Test filtering
putio-sync files list --filter "*.mkv"
```

**Expected**:
- ✅ Files are listed correctly
- ✅ Filtering works as expected
- ✅ Output is formatted properly

### Download Operations
```bash
# Create test directory
mkdir -p ~/putio-downloads

# Download a small file
putio-sync download FILE_ID --dir ~/putio-downloads

# Verify download
ls -lh ~/putio-downloads
```

**Expected**:
- ✅ File downloads successfully
- ✅ Progress bar displays correctly
- ✅ File is saved to correct location
- ✅ File integrity is maintained

### Transfer Operations
```bash
# Add a test torrent (use a legal test torrent)
putio-sync transfers add "magnet:?xt=urn:btih:..."

# List transfers
putio-sync transfers list
```

**Expected**:
- ✅ Transfer is added successfully
- ✅ Transfers are listed correctly

---

## Pre-Release Testing Checklist

Before creating a release, complete this checklist:

### Version Verification
- [ ] Run `bash scripts/pre-release-check.sh VERSION`
- [ ] All version checks pass
- [ ] No version mismatches across packaging files

### Build Verification
- [ ] `cargo build --release` succeeds
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace` has no warnings
- [ ] `cargo fmt -- --check` passes
- [ ] `cargo audit` shows no vulnerabilities

### Documentation
- [ ] CHANGELOG.md is updated
- [ ] README.md reflects current features
- [ ] INSTALLATION.md has correct version numbers
- [ ] MAINTAINING.md is up to date

### Platform Builds (CI)
- [ ] GitHub Actions workflows pass
- [ ] Windows binaries build (x64, ARM64)
- [ ] Debian packages build (amd64, arm64)
- [ ] Snap builds successfully
- [ ] Flatpak builds successfully
- [ ] Homebrew formula validates

---

## Post-Release Testing Checklist

After creating a release, verify package availability:

### GitHub Release
- [ ] Release is published
- [ ] All artifacts are uploaded:
  - [ ] Windows x64 ZIP
  - [ ] Windows ARM64 ZIP
  - [ ] Debian amd64 .deb
  - [ ] Debian arm64 .deb
  - [ ] Flatpak bundle
  - [ ] SHA256 checksums

### Package Managers
- [ ] Homebrew: Tap repository updated
- [ ] Winget: PR created to winget-pkgs
- [ ] Snap: Published to Snap Store
- [ ] Flatpak: Bundle available for download

### Installation Testing
Test installation from each source:
- [ ] macOS Homebrew (x64, ARM64)
- [ ] Windows Winget (x64)
- [ ] Linux Snap (Ubuntu, Fedora, Arch)
- [ ] Linux Flatpak (Ubuntu, Fedora, Arch)
- [ ] Linux Debian (Ubuntu 22.04, 24.04)

### Smoke Testing
After installation on each platform:
- [ ] `putio-sync --version` returns correct version
- [ ] `putio-sync --help` displays help
- [ ] `putio-sync login` opens browser
- [ ] `putio-sync files list` works with valid auth

---

## Automated Testing

### CI/CD Integration
The pre-release check script is integrated into CI:
- Runs on every pull request
- Runs before releases
- Fails if version mismatches detected
- Fails if tests don't pass

### Future Enhancements
Potential automated tests to add:
- [ ] Package installation tests in CI (using Docker)
- [ ] End-to-end functionality tests
- [ ] Performance benchmarks
- [ ] Cross-platform compatibility tests
- [ ] Security scanning for binaries

---

## Reporting Issues

If you encounter issues during testing:

1. **Check Logs**:
   ```bash
   RUST_LOG=debug putio-sync COMMAND
   ```

2. **Gather System Info**:
   - OS version
   - Installation method
   - putio-sync version
   - Error messages

3. **Open Issue**:
   - Go to: https://github.com/devclinton/putio-sync/issues
   - Use the bug report template
   - Include logs and system info

---

## Resources

- [Installation Guide](INSTALLATION.md)
- [Maintenance Guide](MAINTAINING.md)
- [GitHub Releases](https://github.com/devclinton/putio-sync/releases)
- [Issue Tracker](https://github.com/devclinton/putio-sync/issues)
