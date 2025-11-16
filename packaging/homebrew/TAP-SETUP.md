# Homebrew Tap Repository Setup Guide

This guide walks through creating and configuring the Homebrew tap repository for putio-sync.

## Overview

A Homebrew tap is a GitHub repository that contains formulae (package definitions). The tap repository should be named `homebrew-putio-sync` to follow Homebrew naming conventions.

Repository URL: `https://github.com/clintoncollins/homebrew-putio-sync`

## Step 1: Create the GitHub Repository

1. Go to https://github.com/new
2. Create a new repository:
   - **Name**: `homebrew-putio-sync` (must start with `homebrew-`)
   - **Description**: "Homebrew tap for putio-sync CLI tool"
   - **Visibility**: Public (required for Homebrew taps)
   - **Initialize**: Do not add README, .gitignore, or license (we'll add these)

## Step 2: Initialize the Tap Repository

```bash
# Clone the new repository
git clone https://github.com/clintoncollins/homebrew-putio-sync.git
cd homebrew-putio-sync

# Copy the tap repository files from this project
cp -r /path/to/putio-sync/packaging/homebrew/tap-repository/* .

# Initialize git and push
git add .
git commit -m "Initial tap setup with putio-sync formula"
git push origin main
```

## Step 3: Test the Tap Locally

```bash
# Add the tap locally
brew tap clintoncollins/putio-sync

# Verify the tap was added
brew tap | grep putio-sync

# Try installing (requires a valid release tarball)
brew install putio-sync

# Test the installation
putio-sync --version

# Remove the tap (for testing)
brew untap clintoncollins/putio-sync
```

## Step 4: Configure GitHub Actions

The tap repository includes a GitHub Actions workflow (`.github/workflows/tests.yml`) that:
- Validates formula syntax
- Tests installation on macOS and Linux
- Runs on pull requests and pushes

Ensure GitHub Actions are enabled in the repository settings.

## Step 5: Update Formula for Releases

When a new version is released, the formula needs to be updated:

1. Update the `version` field
2. Update the `url` to point to the new release tag
3. Update the `sha256` checksum

This can be done manually or automated via GitHub Actions (see Phase 1.3).

### Manual Formula Update

```bash
# Generate SHA256 for new release
curl -L https://github.com/clintoncollins/putio-sync/archive/refs/tags/v0.2.0.tar.gz | shasum -a 256

# Update Formula/putio-sync.rb with:
# - New version
# - New URL
# - New SHA256

git add Formula/putio-sync.rb
git commit -m "Update formula to version 0.2.0"
git push
```

## User Installation Instructions

Once the tap is set up, users can install putio-sync with:

```bash
# Add the tap
brew tap clintoncollins/putio-sync

# Install putio-sync
brew install putio-sync
```

Or in one command:

```bash
brew install clintoncollins/putio-sync/putio-sync
```

## Tap Repository Structure

```
homebrew-putio-sync/
├── .github/
│   └── workflows/
│       └── tests.yml          # Formula testing workflow
├── Formula/
│   └── putio-sync.rb          # The formula file
├── README.md                   # Installation and usage instructions
└── LICENSE                     # MIT license
```

## Troubleshooting

### Formula Audit Errors

Run the audit locally to check for issues:

```bash
brew audit --strict Formula/putio-sync.rb
```

### Installation Failures

Check the formula logs:

```bash
brew install --verbose --debug putio-sync
```

### Tap Not Found

Ensure the repository is public and named correctly (`homebrew-putio-sync`).

## References

- [Homebrew Tap Documentation](https://docs.brew.sh/Taps)
- [Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Acceptable Formulae Requirements](https://docs.brew.sh/Acceptable-Formulae)
