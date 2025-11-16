# Homebrew Formula Automation

This document describes the automated workflow for updating the Homebrew formula when new releases are published.

## Overview

The `.github/workflows/homebrew.yml` workflow automatically updates the formula in the `homebrew-putio-sync` tap repository whenever a new release is published to the main repository.

## Setup Requirements

### 1. Create the Homebrew Tap Token

The workflow requires a GitHub Personal Access Token (PAT) with write access to the tap repository.

1. Go to https://github.com/settings/tokens/new
2. Create a new token with the following settings:
   - **Note**: "Homebrew Tap Updates"
   - **Expiration**: No expiration (or set based on your security policy)
   - **Scopes**:
     - `repo` (Full control of private repositories)
     - `workflow` (Update GitHub Action workflows)

3. Copy the generated token (you won't be able to see it again!)

### 2. Add the Token to GitHub Secrets

1. Go to the main repository settings: https://github.com/devclinton/putio-sync/settings/secrets/actions
2. Click "New repository secret"
3. Name: `HOMEBREW_TAP_TOKEN`
4. Value: Paste the token from step 1
5. Click "Add secret"

### 3. Create the Tap Repository

Follow the instructions in `TAP-SETUP.md` to create the `homebrew-putio-sync` repository.

## How It Works

### Automatic Updates on Release

When a new release is published (via GitHub Releases):

1. The workflow is triggered automatically
2. It downloads the release tarball from GitHub
3. Calculates the SHA256 checksum
4. Updates the formula in the tap repository with:
   - New version number
   - New download URL
   - New SHA256 checksum
5. Commits and pushes the changes to the tap repository
6. Verifies the formula syntax

### Manual Updates

You can also trigger the workflow manually:

1. Go to https://github.com/devclinton/putio-sync/actions/workflows/homebrew.yml
2. Click "Run workflow"
3. Enter the version tag (e.g., `v0.1.2`)
4. Click "Run workflow"

## Workflow Details

### Inputs

- **Automatic (on release)**: Version is extracted from the release tag
- **Manual**: Version must be provided as input (format: `v0.1.1`)

### Steps

1. **Get Version**: Extract version from release or manual input
2. **Download & Calculate SHA256**: Download the release tarball and compute its checksum
3. **Checkout Tap Repository**: Clone the homebrew-putio-sync repository
4. **Update Formula**: Modify the formula file with new version, URL, and SHA256
5. **Commit & Push**: Commit changes and push to the tap repository
6. **Verify Formula**: Run brew audit to validate the formula
7. **Create Summary**: Generate a summary with installation instructions

### Formula Updates

The workflow updates three fields in the formula:

```ruby
version "0.1.1"  # Updated to new version
url "https://github.com/devclinton/putio-sync/archive/refs/tags/v0.1.1.tar.gz"  # Updated URL
sha256 "abcd1234..."  # Updated checksum
```

## Testing the Workflow

### Test with a Draft Release

1. Create a draft release in GitHub
2. Publish the draft release
3. Watch the workflow run in the Actions tab
4. Check the tap repository for the updated formula

### Verify the Update

After the workflow completes:

```bash
# Update Homebrew
brew update

# Check the formula version
brew info devclinton/putio-sync/putio-sync

# Install or upgrade
brew upgrade putio-sync
```

## Troubleshooting

### Workflow Fails: "Authentication failed"

- Check that `HOMEBREW_TAP_TOKEN` secret is set correctly
- Verify the token has `repo` and `workflow` scopes
- Ensure the token hasn't expired

### Workflow Fails: "Failed to download tarball"

- Verify the release tarball is actually published
- Check that the release tag format matches (should start with `v`)
- Ensure the release is public, not a draft

### Formula Audit Fails

- Check the formula syntax manually: `brew audit --strict Formula/putio-sync.rb`
- Verify the download URL is accessible
- Ensure the SHA256 matches the actual tarball

### Users Report Formula Issues

If users report issues after an automated update:

1. Check the commit in the tap repository
2. Verify the SHA256 is correct:
   ```bash
   curl -L https://github.com/devclinton/putio-sync/archive/refs/tags/v0.1.1.tar.gz | shasum -a 256
   ```
3. Manually fix the formula if needed
4. Consider reverting the commit and re-running the workflow

## Manual Formula Updates

If you need to update the formula manually (without the workflow):

```bash
# Clone the tap repository
git clone https://github.com/devclinton/homebrew-putio-sync.git
cd homebrew-putio-sync

# Edit the formula
vim Formula/putio-sync.rb

# Update version, url, and sha256

# Commit and push
git add Formula/putio-sync.rb
git commit -m "Update putio-sync to v0.1.2"
git push
```

## Related Documentation

- [TAP-SETUP.md](TAP-SETUP.md) - Setting up the tap repository
- [README.md](README.md) - Formula testing and local installation
- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
