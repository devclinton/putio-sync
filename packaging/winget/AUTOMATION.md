# Winget Package Automation

This document describes the automated workflow for submitting putio-sync to the Windows Package Manager (Winget) repository.

## Overview

The `.github/workflows/winget.yml` workflow automatically creates a pull request to the [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) repository whenever a new release is published.

## How It Works

### Automatic Submission on Release

When a new GitHub release is published:

1. The workflow is triggered automatically
2. It uses the [winget-releaser](https://github.com/vedantmgoyal2009/winget-releaser) action
3. The action:
   - Detects Windows ZIP releases from the release assets
   - Downloads the installers and calculates SHA256 checksums
   - Creates/updates the Winget manifest files
   - Forks the microsoft/winget-pkgs repository (if needed)
   - Creates a pull request with the updated manifests
4. Microsoft's validation workflow runs automatically
5. Maintainers review and merge the PR

### Manual Trigger

You can also trigger the workflow manually:

1. Go to https://github.com/clintoncollins/putio-sync/actions/workflows/winget.yml
2. Click "Run workflow"
3. Enter the version tag (e.g., `v0.1.2`)
4. Click "Run workflow"

## Setup Requirements

### 1. Windows Binary Releases

The workflow requires Windows binary releases in ZIP format. Ensure your release workflow (Phase 2.3) creates:

- `putio-sync-v{VERSION}-x86_64-pc-windows-msvc.zip`
- `putio-sync-v{VERSION}-aarch64-pc-windows-msvc.zip`

These are matched by the regex: `putio-sync-.*-windows-.*\.zip$`

### 2. GitHub Token

The workflow uses `GITHUB_TOKEN` which is automatically provided by GitHub Actions. No additional setup is required.

### 3. First-Time Setup

For the first release to Winget:

1. The workflow will create a fork of microsoft/winget-pkgs
2. Submit the PR manually or wait for the automated PR
3. Address any review feedback from Microsoft maintainers
4. Once merged, future updates will be automatic

## Workflow Details

### winget-releaser Action

The workflow uses [vedantmgoyal2009/winget-releaser@v2](https://github.com/vedantmgoyal2009/winget-releaser) which:

- **Automatically detects installers** from release assets
- **Calculates SHA256 checksums** for validation
- **Creates proper manifest structure** following Winget schema
- **Manages versioning** and updates existing packages
- **Creates pull requests** with proper formatting
- **Handles validation** errors automatically

### Configuration

```yaml
identifier: putio.putio-sync          # Package identifier
version: ${{ steps.get_version.outputs.version }}  # From release tag
installers-regex: 'putio-sync-.*-windows-.*\.zip$'  # Match Windows ZIPs
token: ${{ secrets.GITHUB_TOKEN }}    # GitHub authentication
fork-user: github-actions[bot]        # Fork under bot account
max-versions-to-keep: 5               # Keep 5 recent versions
```

## Submission Process

### 1. Release Published

When you publish a GitHub release, the workflow starts automatically.

### 2. PR Creation

The workflow creates a PR to microsoft/winget-pkgs with:
- Updated manifest files in `manifests/p/putio/putio-sync/{version}/`
- Proper commit messages and PR description
- All required files (version, installer, locale manifests)

### 3. Validation

Microsoft's automated validation checks:
- Manifest schema compliance
- URL accessibility
- SHA256 checksum accuracy
- SmartScreen reputation (may require time for new packages)
- Installer type and compatibility

### 4. Review

Microsoft maintainers review:
- Package metadata accuracy
- License compliance
- Installer behavior
- Security concerns

### 5. Merge

Once approved and validated:
- PR is merged to microsoft/winget-pkgs
- Package becomes available via `winget install putio.putio-sync`
- Updates typically appear within a few hours

## Monitoring Submissions

### Check PR Status

After the workflow runs:

1. Go to https://github.com/microsoft/winget-pkgs/pulls
2. Search for "putio.putio-sync" or your username
3. Monitor the PR for:
   - Validation results
   - Review comments
   - Merge status

### Workflow Logs

View the workflow execution:

1. Go to https://github.com/clintoncollins/putio-sync/actions/workflows/winget.yml
2. Click on the latest run
3. Check the logs for:
   - Version detection
   - PR creation status
   - Any errors or warnings

## Troubleshooting

### Workflow Fails: "No installers found"

**Cause**: The release doesn't have Windows ZIP files matching the regex.

**Fix**:
1. Ensure Phase 2.3 (Windows Binary Releases) is complete
2. Check release assets include files like:
   - `putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip`
   - `putio-sync-v0.1.1-aarch64-pc-windows-msvc.zip`
3. Verify the regex pattern matches your file names

### Validation Fails: "SmartScreen warning"

**Cause**: New packages may trigger SmartScreen warnings initially.

**Fix**:
- This is normal for new publishers
- Warnings decrease as the package gains reputation
- Consider code signing for faster trust building
- Users can bypass with `--ignore-security-hash` (not recommended)

### Validation Fails: "Incorrect SHA256"

**Cause**: Checksum mismatch between manifest and actual file.

**Fix**:
- winget-releaser calculates this automatically
- If manual update needed, recalculate:
  ```powershell
  Get-FileHash -Algorithm SHA256 installer.zip
  ```

### PR Not Created

**Cause**: Various reasons including rate limits or permissions.

**Fix**:
1. Check workflow logs for specific error
2. Verify GITHUB_TOKEN has proper permissions
3. Ensure no rate limit errors
4. Try manual trigger after a delay

### Review Feedback

Common review requests:
- **Better description**: Update locale manifest with more details
- **License verification**: Ensure license URL is correct
- **Installer improvements**: Consider MSI instead of ZIP (future enhancement)
- **Metadata accuracy**: Fix typos or improve tags

**Response**:
1. Update local manifest files in `packaging/winget/manifests/`
2. Commit changes to main repository
3. Re-run the workflow or update PR manually

## Manual Submission

If automated submission fails, submit manually:

### Steps

1. Fork https://github.com/microsoft/winget-pkgs

2. Create version directory:
   ```bash
   mkdir -p manifests/p/putio/putio-sync/0.1.1/
   ```

3. Copy manifest files:
   ```bash
   cp packaging/winget/manifests/*.yaml manifests/p/putio/putio-sync/0.1.1/
   ```

4. Update checksums:
   ```powershell
   # Download installers
   Invoke-WebRequest -Uri "https://github.com/clintoncollins/putio-sync/releases/download/v0.1.1/putio-sync-v0.1.1-x86_64-pc-windows-msvc.zip" -OutFile "x64.zip"

   # Calculate SHA256
   Get-FileHash -Algorithm SHA256 x64.zip

   # Update putio.putio-sync.installer.yaml with the checksum
   ```

5. Create pull request to microsoft/winget-pkgs

6. Address validation and review feedback

## Best Practices

### Release Timing

- Publish releases during Microsoft's business hours (PST) for faster review
- Allow 1-3 days for initial review of new packages
- Updates to existing packages are usually faster

### Version Numbering

- Use semantic versioning (X.Y.Z)
- Ensure Cargo.toml version matches release tag
- Update all manifest files with the same version

### Testing Before Release

1. Test Windows binaries locally
2. Verify ZIP structure contains putio-sync.exe
3. Test installation from local manifest:
   ```powershell
   winget install --manifest packaging/winget/manifests/
   ```

### Communication

- Respond promptly to review feedback
- Be polite and professional with maintainers
- Provide additional context if requested

## Related Documentation

- [README.md](README.md) - Manifest structure and local validation
- [Winget Documentation](https://learn.microsoft.com/en-us/windows/package-manager/)
- [winget-pkgs Contributing Guide](https://github.com/microsoft/winget-pkgs/blob/master/CONTRIBUTING.md)
- [winget-releaser Documentation](https://github.com/vedantmgoyal2009/winget-releaser)
