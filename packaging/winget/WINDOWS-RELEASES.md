# Windows Binary Releases

This document describes the Windows binary build and release process for putio-sync.

## Overview

The `.github/workflows/release-binaries.yml` workflow automatically builds Windows binaries for both x64 and ARM64 architectures when a release is published.

## Supported Architectures

### x86_64 (x64)
- **Target**: `x86_64-pc-windows-msvc`
- **Minimum OS**: Windows 10/11 (64-bit)
- **Compatibility**: Intel and AMD processors

### ARM64
- **Target**: `aarch64-pc-windows-msvc`
- **Minimum OS**: Windows 10/11 (ARM64)
- **Compatibility**: ARM-based Windows devices (Surface Pro X, etc.)

## Build Process

### Automatic Builds

When a GitHub release is published:

1. Workflow triggers automatically
2. Builds binaries for both architectures in parallel
3. Creates ZIP archives containing:
   - `putio-sync.exe` - The compiled binary
   - `README.md` - Project documentation
   - `LICENSE` - MIT license file
4. Calculates SHA256 checksums for each archive
5. Uploads archives and checksums to the release

### Manual Builds

You can trigger builds manually:

1. Go to https://github.com/devclinton/putio-sync/actions/workflows/release-binaries.yml
2. Click "Run workflow"
3. Enter the version tag (e.g., `v0.1.1`)
4. Click "Run workflow"
5. Artifacts will be available for download (not attached to a release)

## Archive Format

### File Naming

Archives follow this pattern:
```
putio-sync-{version}-{target}.zip
putio-sync-{version}-{target}.zip.sha256
```

Examples:
- `putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip`
- `putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip.sha256`
- `putio-sync-v0.1.1-aarch64-pc-windows-msvc.zip`
- `putio-sync-v0.1.1-aarch64-pc-windows-msvc.zip.sha256`

### Archive Contents

```
putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip
├── putio-sync.exe     # Main executable
├── README.md          # Documentation
└── LICENSE            # MIT license
```

## Installation Instructions

### For Users

#### Method 1: Using Winget (once listed)

putio-sync isn't in winget-pkgs yet. Once it is:

```powershell
winget install devclinton.putio-sync
```

#### Method 2: Manual Installation

1. Download the appropriate ZIP for your architecture:
   - Most users: `putio-sync-v{VERSION}-x86_64-pc-windows-msvc.zip`
   - ARM devices: `putio-sync-v{VERSION}-aarch64-pc-windows-msvc.zip`

2. Extract the ZIP archive

3. (Optional) Add the directory to your PATH:
   ```powershell
   # Add to user PATH
   $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
   [Environment]::SetEnvironmentVariable("Path", "$userPath;C:\path\to\putio-sync", "User")
   ```

4. Run from anywhere:
   ```powershell
   putio-sync --version
   ```

### Verifying Downloads

Verify the integrity of downloaded files:

```powershell
# Download both the ZIP and .sha256 file
# Calculate hash of downloaded ZIP
$actualHash = (Get-FileHash -Algorithm SHA256 putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip).Hash

# Read expected hash
$expectedHash = Get-Content putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip.sha256

# Compare
if ($actualHash -eq $expectedHash) {
    Write-Host "✅ Checksum verified!" -ForegroundColor Green
} else {
    Write-Host "❌ Checksum mismatch!" -ForegroundColor Red
}
```

## Development

### Building Locally

#### Prerequisites

1. Install Rust: https://rustup.rs/
2. Add Windows targets:
   ```powershell
   rustup target add x86_64-pc-windows-msvc
   rustup target add aarch64-pc-windows-msvc
   ```

#### Build Commands

Build for your current architecture:
```powershell
cargo build --release --package putio-sync-cli
```

Cross-compile for other architectures:
```powershell
# Build for x64
cargo build --release --target x86_64-pc-windows-msvc --package putio-sync-cli

# Build for ARM64
cargo build --release --target aarch64-pc-windows-msvc --package putio-sync-cli
```

#### Create Archive Locally

```powershell
# Create archive directory
New-Item -ItemType Directory -Path "archive" -Force

# Copy binary
Copy-Item "target/x86_64-pc-windows-msvc/release/putio-sync.exe" -Destination "archive/"

# Copy docs
Copy-Item "README.md", "LICENSE" -Destination "archive/"

# Create ZIP
Compress-Archive -Path "archive/*" -DestinationPath "putio-sync-windows-x64.zip"

# Calculate SHA256
(Get-FileHash -Algorithm SHA256 "putio-sync-windows-x64.zip").Hash
```

## Troubleshooting

### Build Errors

#### Missing Target

```
error: can't find crate for `std`
```

**Solution**: Add the target
```powershell
rustup target add aarch64-pc-windows-msvc
```

#### Linker Errors

```
error: linking with `link.exe` failed
```

**Solution**: Install Visual Studio Build Tools
- Download from: https://visualstudio.microsoft.com/downloads/
- Select "Desktop development with C++"

### Runtime Errors

#### "VCRUNTIME140.dll not found"

**Solution**: Install Microsoft Visual C++ Redistributable
- Download from: https://aka.ms/vs/17/release/vc_redist.x64.exe

#### "This app can't run on your PC"

**Causes**:
- Downloaded ARM64 binary on x64 system (or vice versa)
- Downloaded for wrong architecture

**Solution**: Download the correct binary for your architecture

### SmartScreen Warnings

New executables may trigger Windows SmartScreen warnings initially.

**For Users**:
- This is normal for new publishers
- Verify the download source is github.com/devclinton/putio-sync
- Check SHA256 matches the published checksum
- Click "More info" → "Run anyway" if you trust the source

**For Developers**:
- Consider code signing for production releases
- Code signing certificates available from: Sectigo, DigiCert, etc.
- Reduces SmartScreen warnings significantly

## Code Signing (Future Enhancement)

To reduce SmartScreen warnings and increase trust:

### 1. Obtain Certificate

- Purchase from: Sectigo, DigiCert, etc.
- Cost: ~$200-500/year
- Requires company or individual verification

### 2. Sign Binaries

```powershell
# Using signtool (from Windows SDK)
signtool sign /f certificate.pfx /p password /t http://timestamp.digicert.com putio-sync.exe
```

### 3. Update Workflow

Add signing step to `.github/workflows/release-binaries.yml`:
```yaml
- name: Sign binary
  run: |
    signtool sign /f ${{ secrets.SIGNING_CERT }} /p ${{ secrets.CERT_PASSWORD }} target/release/putio-sync.exe
```

## Performance Notes

### Binary Size

Typical sizes (release builds with optimizations):
- x64: ~5-10 MB (depends on features)
- ARM64: ~5-10 MB (similar to x64)

Sizes can be reduced with:
```toml
# Cargo.toml
[profile.release]
strip = true          # Remove debug symbols
lto = true           # Link-time optimization
opt-level = "z"      # Optimize for size
codegen-units = 1    # Better optimization
```

### Startup Time

- Cold start: ~100-200ms
- With file I/O: ~300-500ms
- Network operations: Depends on connection

## Related Documentation

- [README.md](README.md) - Winget manifest documentation
- [AUTOMATION.md](AUTOMATION.md) - Winget submission automation
- [Windows SDK](https://developer.microsoft.com/en-us/windows/downloads/windows-sdk/)
