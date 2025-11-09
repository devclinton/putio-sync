# putio-sync

A Rust-based synchronization tool for Put.io that supports CLI and web interfaces.

## Features

- **CLI Interface**: Command-line tools for managing your Put.io files
- **Web Interface**: Web server with REST API and React UI
- **Core Library**: Reusable core library for Put.io API integration

## Architecture

This project is organized as a Cargo workspace with the following crates:

- **putio-sync-core**: Core library containing:
  - Put.io API client
  - Sync planner
  - Local state management (SQLite)
  - Configuration and authentication handling
  - Download logic with chunked and resume support

- **putio-sync-cli**: Command-line interface providing commands:
  - `login`: Authenticate with Put.io
  - `list`: Browse files and folders
  - `sync`: Synchronize files
  - `watch`: Monitor sync status

- **putio-sync-web**: Web server (coming soon) with:
  - REST API for sync management
  - React-based UI
  - Real-time progress updates

## Development

### Prerequisites

- Rust 1.70 or later
- SQLite 3

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running the CLI

```bash
cargo run --bin putio-sync -- --help
```

## Distribution

- Compiled binaries for macOS, Linux, and Windows
- Docker image (coming soon)

## Development Principles

- **Test-Driven Development**: Write tests first
- **Documentation**: Document as we go
- **Structured Logging**: Using tracing from day one
- **Robust Error Handling**: Comprehensive error types
- **Commit Frequency**: Commit after completing each feature

## License

MIT
