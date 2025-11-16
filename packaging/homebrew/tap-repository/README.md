# Homebrew Tap for putio-sync

This is the official Homebrew tap for [putio-sync](https://github.com/devclinton/putio-sync), a CLI tool for syncing files from Put.io to local storage.

## Installation

### Install via Homebrew

```bash
brew tap devclinton/putio-sync
brew install putio-sync
```

Or install in one command:

```bash
brew install devclinton/putio-sync/putio-sync
```

### Upgrade

```bash
brew update
brew upgrade putio-sync
```

### Uninstall

```bash
brew uninstall putio-sync
brew untap devclinton/putio-sync
```

## About putio-sync

putio-sync is a command-line tool for downloading and managing files from Put.io cloud storage service.

### Features

- Download files and folders from Put.io
- Concurrent downloads with configurable workers
- Resume support for interrupted downloads
- Daemon mode for continuous syncing
- Filter files by name, type, size, and date
- Delete files after download
- HTTP API server for remote control
- Watch mode for automatic torrent uploads
- Comprehensive download history and statistics

### Quick Start

After installation:

```bash
# Login with OAuth token
putio-sync login

# List files
putio-sync files list

# Download files
putio-sync download

# Start daemon mode
putio-sync server --daemon

# Get help
putio-sync --help
```

## Documentation

- [Main Repository](https://github.com/devclinton/putio-sync)
- [Documentation](https://github.com/devclinton/putio-sync#readme)
- [Issues](https://github.com/devclinton/putio-sync/issues)

## Formula Information

The formula for putio-sync is automatically updated when new versions are released.

### Formula Location

The formula is located at `Formula/putio-sync.rb` in this repository.

### Requirements

- macOS or Linux
- Rust toolchain (installed automatically as a build dependency)

## Contributing

If you encounter any issues with the Homebrew formula, please:

1. Check existing issues: https://github.com/devclinton/putio-sync/issues
2. Open a new issue with details about your environment and the error

## License

This tap repository is licensed under the MIT License. The putio-sync software itself is also MIT licensed.
