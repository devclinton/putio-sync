# Homebrew Formula for putio-sync

This directory contains the Homebrew formula for installing `putio-sync` on macOS and Linux.

## Installation (for users)

Once the tap is set up, users can install putio-sync with:

```bash
brew tap clintoncollins/putio-sync
brew install putio-sync
```

Or install directly from the repository URL:

```bash
brew install clintoncollins/putio-sync/putio-sync
```

## Local Testing (for maintainers)

### Testing with a local build

For development and testing purposes, you can test the formula locally:

```bash
# Test the formula audit
brew audit --strict --online ./packaging/homebrew/putio-sync.rb

# Install from the local formula (requires a valid tarball URL or tag)
brew install --build-from-source ./packaging/homebrew/putio-sync.rb

# Uninstall after testing
brew uninstall putio-sync
```

### Testing with a local git checkout

For testing during development without a release:

```bash
# Create a temporary formula pointing to HEAD
brew install --HEAD --build-from-source ./packaging/homebrew/putio-sync.rb
```

### Updating the Formula

When releasing a new version:

1. Update the `version` field in the formula
2. Update the `url` to point to the new release tag
3. Update the `sha256` checksum (this is done automatically by the release workflow)

To generate the SHA256 checksum manually:

```bash
curl -L https://github.com/clintoncollins/putio-sync/archive/refs/tags/v0.1.1.tar.gz | shasum -a 256
```

## Tap Repository

The formula will eventually be published to a separate tap repository at:
`https://github.com/clintoncollins/homebrew-putio-sync`

This follows Homebrew's convention of `homebrew-<tap-name>`.

## Automated Updates

The GitHub Actions workflow `.github/workflows/homebrew.yml` will automatically update the formula in the tap repository when a new release is published.
