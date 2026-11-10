# Windows Package Manager (Winget) Manifests

This directory contains the Winget package manifest for installing `putio-sync` on Windows via the Windows Package Manager.

## Installation (for users)

Once the package is published to the winget-pkgs repository, users can install putio-sync with:

```powershell
winget install devclinton.putio-sync
```

Or search for it:

```powershell
winget search putio-sync
```

## Manifest Structure

Winget uses a multi-file manifest format (schema version 1.6.0):

### Files

1. **devclinton.putio-sync.yaml** - Version manifest
   - Defines the package identifier and version
   - Links to the other manifest files

2. **devclinton.putio-sync.installer.yaml** - Installer manifest
   - Defines installer type, architecture, and download URLs
   - Includes SHA256 checksums for verification
   - Supports both x64 and ARM64 Windows

3. **devclinton.putio-sync.locale.en-US.yaml** - Default locale manifest
   - Package metadata (name, description, publisher)
   - License information
   - Tags and release notes

## Package Identifier

The package uses the identifier: `devclinton.putio-sync`

Format: `{Publisher}.{PackageName}`

## Supported Architectures

- **x64** (x86_64-pc-windows-msvc)
- **ARM64** (aarch64-pc-windows-msvc)

## Local Validation

You can validate the manifests locally using wingetcreate or the Winget CLI:

### Using Winget CLI (Windows only)

```powershell
# Install Winget (if not already installed)
# Available from Microsoft Store or GitHub releases

# Validate manifests
winget validate packaging/winget/manifests/

# Test installation from local manifest
winget install --manifest packaging/winget/manifests/
```

### Using wingetcreate

```powershell
# Install wingetcreate
winget install wingetcreate

# Update manifest for new version
wingetcreate update devclinton.putio-sync --version 0.1.2 --urls https://github.com/devclinton/putio-sync/releases/download/v0.1.2/putio-sync-v0.1.2-x86_64-pc-windows-msvc.zip
```

## Manual Manifest Updates

When releasing a new version, update the following fields in all three manifest files:

1. **Version** (all files):
   ```yaml
   PackageVersion: 0.1.2
   ```

2. **Installer URLs and checksums** (installer.yaml):
   ```yaml
   InstallerUrl: https://github.com/devclinton/putio-sync/releases/download/v0.1.2/putio-sync-v0.1.2-x86_64-pc-windows-msvc.zip
   InstallerSha256: NEW_CHECKSUM_HERE
   ```

3. **Release date** (installer.yaml):
   ```yaml
   ReleaseDate: 2025-11-15
   ```

4. **Release notes** (locale.en-US.yaml):
   ```yaml
   ReleaseNotes: |-
     Version 0.1.2
     - Feature or fix description
   ReleaseNotesUrl: https://github.com/devclinton/putio-sync/releases/tag/v0.1.2
   ```

### Generating SHA256 Checksums

On Windows (PowerShell):
```powershell
Get-FileHash -Algorithm SHA256 putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip
```

On macOS/Linux:
```bash
curl -L https://github.com/devclinton/putio-sync/releases/download/v0.1.1/putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip | sha256sum
```

## Submission to winget-pkgs

The manifests in this directory will be automatically submitted to the official [winget-pkgs](https://github.com/microsoft/winget-pkgs) repository via the GitHub Actions workflow (`.github/workflows/winget.yml`).

### Submission Process

1. Release is published on GitHub
2. Workflow generates Windows binaries
3. Workflow updates manifest checksums
4. Workflow creates a PR to microsoft/winget-pkgs
5. Microsoft team reviews and merges the PR
6. Package becomes available via `winget install`

### Manual Submission

If needed, you can manually submit to winget-pkgs:

1. Fork https://github.com/microsoft/winget-pkgs
2. Create a new directory: `manifests/p/putio/putio-sync/0.1.1/`
3. Copy all three manifest files to that directory
4. Create a pull request
5. Address any validation errors or review feedback

## Troubleshooting

### Validation Errors

Run validation to check for errors:
```powershell
winget validate packaging/winget/manifests/
```

Common issues:
- Incorrect YAML formatting (use spaces, not tabs)
- Invalid SHA256 checksums
- Unreachable installer URLs
- Schema version mismatch

### Installer Type

The manifest uses `InstallerType: zip` with `NestedInstallerType: portable` because:
- Windows releases are distributed as ZIP archives
- The executable is portable (no installation required)
- Winget extracts and manages the binary automatically

## References

- [Winget CLI Documentation](https://learn.microsoft.com/en-us/windows/package-manager/winget/)
- [Manifest Schema](https://github.com/microsoft/winget-pkgs/tree/master/doc/manifest)
- [Submission Guidelines](https://github.com/microsoft/winget-pkgs/blob/master/CONTRIBUTING.md)
- [wingetcreate Tool](https://github.com/microsoft/winget-create)
