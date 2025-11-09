# putio-sync Workplan - Phase 1: Login & List Commands

## Overview
This workplan covers the initial implementation of the `putio-sync` project focusing on:
- Project structure setup (core and cli crates)
- Login command with config management
- List command with filtering and subfolder support
- GitHub workflows for CI/CD
- Test-driven development approach

---

## 1. Project Structure & Initial Setup

### 1.1 Create Cargo Workspace
**Feature**: Initialize Rust workspace with core and CLI crates

**Tasks**:
- [ ] Create root `Cargo.toml` workspace file
- [ ] Create `putio-sync-core/` directory with `Cargo.toml`
- [ ] Create `putio-sync-cli/` directory with `Cargo.toml`
- [ ] Add initial dependencies to both crates
- [ ] Create basic README.md for the project
- [ ] Initialize `.gitignore` for Rust projects

**Commit**: "feat: initialize cargo workspace with core and cli crates"

**Changelog Entries**:
- `putio-sync-core/CHANGELOG.md`: "### Added\n- Initial crate structure"
- `putio-sync-cli/CHANGELOG.md`: "### Added\n- Initial crate structure"

**Dependencies for core**:
```toml
[dependencies]
reqwest = { version = "0.12", features = ["json", "stream"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
thiserror = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"
sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "sqlite"] }
toml = "0.8"
dirs = "5.0"
```

**Dependencies for CLI**:
```toml
[dependencies]
putio-sync-core = { path = "../putio-sync-core" }
clap = { version = "4.5", features = ["derive", "env"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

---

### 1.2 Setup Logging Infrastructure
**Feature**: Structured logging with tracing

**Tasks**:
- [ ] Create `putio-sync-core/src/logging.rs` module
- [ ] Configure tracing subscriber with multiple levels (trace, debug, info, warn, error)
- [ ] Add environment variable support for log levels
- [ ] Write unit tests for logging configuration

**Commit**: "feat(core): add structured logging infrastructure"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Structured logging using `tracing` crate
- Environment variable support for log level configuration (RUST_LOG)
```

---

### 1.3 Error Handling Framework
**Feature**: Robust error handling with custom error types

**Tasks**:
- [ ] Create `putio-sync-core/src/error.rs` module
- [ ] Define `CoreError` enum using `thiserror`
- [ ] Include variants for: ApiError, ConfigError, DatabaseError, IoError, AuthError
- [ ] Implement Display and Error traits
- [ ] Add `Result<T>` type alias as `CoreResult<T>`
- [ ] Write tests for error conversions

**Commit**: "feat(core): implement comprehensive error handling"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Custom error types using `thiserror`
- CoreError enum with variants for different error categories
- CoreResult type alias for ergonomic error handling
```

---

## 2. Configuration Management

### 2.1 Config Structure & File Handling
**Feature**: Configuration file management with TOML format

**Config File Location**:
- Default: `~/.putiosync/config.toml`
- Customizable via build-time constant
- Can be overridden via CLI flag: `--config <path>`
- Can be overridden via environment variable: `PUTIO_SYNC_CONFIG`

**Config Structure** (modifiable):
```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub auth: Option<AuthConfig>,
    pub sync: SyncConfig,
    pub download: DownloadConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    pub local_path: PathBuf,
    pub remote_parent_id: Option<i64>,  // Default to root (0)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadConfig {
    pub max_concurrent: usize,  // Default: 3
    pub chunk_size_mb: Option<usize>,  // Default: None (disabled)
    pub resume_enabled: bool,  // Default: true
}
```

**Tasks**:
- [ ] Create `putio-sync-core/src/config.rs` module
- [ ] Implement `Config` struct with serde
- [ ] Implement `Config::load(path)` to read from file
- [ ] Implement `Config::save(&self, path)` to write to file
- [ ] Implement `Config::default_path()` to get default config location
- [ ] Create config directory if it doesn't exist
- [ ] Handle missing config file gracefully
- [ ] Write unit tests for config loading/saving
- [ ] Write integration tests for config file operations

**Commit**: "feat(core): implement configuration management with TOML"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Configuration management with TOML format
- Default config location at ~/.putiosync/config.toml
- Support for custom config paths
- Auto-creation of config directory
```

---

### 2.2 Config Validation & Defaults
**Feature**: Config validation and sensible defaults

**Tasks**:
- [ ] Implement `Config::validate()` method
- [ ] Add validation for required fields
- [ ] Implement `Default` trait for config structs
- [ ] Add config builder pattern for programmatic creation
- [ ] Write tests for validation logic

**Commit**: "feat(core): add config validation and defaults"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Configuration validation
- Default values for all config options
- Builder pattern for config creation
```

---

## 3. Put.io API Client

### 3.1 Base API Client Structure
**Feature**: HTTP client wrapper for Put.io API

**API Base URL**: `https://api.put.io/v2`

**Authentication**: OAuth2 token via header: `Authorization: Bearer <token>`

**Tasks**:
- [ ] Create `putio-sync-core/src/api/mod.rs` module
- [ ] Create `putio-sync-core/src/api/client.rs`
- [ ] Implement `ApiClient` struct with reqwest
- [ ] Add `new(token: String)` constructor
- [ ] Add `base_url` field (configurable, default to production)
- [ ] Implement request wrapper with auth header injection
- [ ] Add retry logic with exponential backoff
- [ ] Add request/response logging
- [ ] Write unit tests with mock server

**Commit**: "feat(core): implement base Put.io API client"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Put.io API client with reqwest
- OAuth2 token authentication
- Request retry with exponential backoff
- Request/response logging
```

---

### 3.2 API Models
**Feature**: Data models for Put.io API responses

**Models to Implement**:
- File/Folder structure
- Transfer information
- Account information
- API error responses

**Tasks**:
- [ ] Create `putio-sync-core/src/api/models.rs`
- [ ] Implement `FileEntry` struct (id, name, size, file_type, parent_id, etc.)
- [ ] Implement `Transfer` struct (based on API spec)
- [ ] Implement `AccountInfo` struct
- [ ] Implement `ApiResponse<T>` wrapper
- [ ] Implement `ApiError` struct
- [ ] Add serialization/deserialization with serde
- [ ] Write tests for JSON parsing

**Commit**: "feat(core): add Put.io API data models"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Data models for Put.io API entities
- FileEntry, Transfer, AccountInfo structs
- JSON serialization/deserialization
```

---

### 3.3 File & Folder API Endpoints
**Feature**: Implement file/folder listing endpoints

**Endpoints to Implement**:
- `GET /files/list` - List files in a folder
- `GET /files/{id}` - Get file details
- `GET /files/{id}/download` - Get download URL

**Tasks**:
- [ ] Create `putio-sync-core/src/api/files.rs`
- [ ] Implement `ApiClient::list_files(parent_id: i64)` method
- [ ] Implement `ApiClient::get_file(id: i64)` method
- [ ] Implement `ApiClient::get_download_url(id: i64)` method
- [ ] Add query parameter support for filtering
- [ ] Add pagination support if needed
- [ ] Write unit tests with mock responses
- [ ] Write integration tests (optional, requires real token)

**Commit**: "feat(core): implement file and folder API endpoints"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- File listing API endpoint
- File details API endpoint
- Download URL generation
- Support for folder navigation
```

---

### 3.4 Account Verification Endpoint
**Feature**: Verify OAuth token and get account info

**Endpoint**: `GET /account/info`

**Tasks**:
- [ ] Create `putio-sync-core/src/api/account.rs`
- [ ] Implement `ApiClient::verify_token()` method
- [ ] Implement `ApiClient::get_account_info()` method
- [ ] Return account details (username, disk usage, etc.)
- [ ] Write unit tests
- [ ] Write integration tests (optional)

**Commit**: "feat(core): add account verification endpoint"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Account information API endpoint
- Token verification capability
```

---

## 4. CLI Foundation

### 4.1 CLI Structure with Clap
**Feature**: Command-line argument parsing

**CLI Interface Design** (modifiable):

```
putio-sync <COMMAND> [OPTIONS]

Commands:
  login    Authenticate with Put.io and save credentials
  list     List files and folders from Put.io
  help     Print this message or the help of the given subcommand(s)

Options:
  -c, --config <PATH>    Path to config file [default: ~/.putiosync/config.toml]
  -v, --verbose...       Increase logging verbosity (-v, -vv, -vvv)
  -q, --quiet            Suppress all output except errors
  -h, --help             Print help
  -V, --version          Print version
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/main.rs`
- [ ] Create `putio-sync-cli/src/cli.rs` with clap definitions
- [ ] Define `Cli` struct with global options
- [ ] Define `Commands` enum
- [ ] Implement `--config` flag for custom config path
- [ ] Implement `--verbose` flag for log level control
- [ ] Implement `--quiet` flag
- [ ] Add version info from Cargo.toml
- [ ] Write help text for each command

**Commit**: "feat(cli): implement CLI structure with clap"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Command-line argument parsing with clap
- Global options: config path, verbosity, quiet mode
- Version and help commands
```

---

### 4.2 CLI Logging Integration
**Feature**: Connect CLI verbosity flags to logging system

**Tasks**:
- [ ] Create `putio-sync-cli/src/logging.rs`
- [ ] Map `-v` flags to log levels (warn -> info -> debug -> trace)
- [ ] Implement `--quiet` mode (errors only)
- [ ] Initialize tracing subscriber in main()
- [ ] Add colored output support (optional)
- [ ] Write tests for verbosity mapping

**Commit**: "feat(cli): integrate logging with verbosity flags"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Verbosity control via -v flags
- Quiet mode for minimal output
- Colored log output support
```

---

## 5. Login Command

### 5.1 Login Command Interface
**Feature**: Authenticate with Put.io and save token

**Command Signature** (modifiable):
```
putio-sync login [OPTIONS]

Options:
  -t, --token <TOKEN>          Put.io OAuth token
  --interactive                Prompt for token interactively
  --verify                     Verify token before saving [default: true]
  --no-verify                  Skip token verification
  -h, --help                   Print help
```

**Authentication Flow Options** (choose one or implement multiple):

**Option A: Manual Token Entry**
- User provides token via `--token` flag or interactive prompt
- Token is verified against Put.io API
- Token is saved to config file

**Option B: OAuth Flow** (future enhancement)
- Display URL for user to authorize
- Start local HTTP server to receive callback
- Exchange code for token
- Save token to config

**Recommended for Phase 1**: Option A (manual token)

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/mod.rs`
- [ ] Create `putio-sync-cli/src/commands/login.rs`
- [ ] Define `LoginArgs` struct with clap
- [ ] Implement `--token` flag
- [ ] Implement `--interactive` mode with stdin prompt
- [ ] Implement token verification step
- [ ] Implement `--no-verify` flag to skip verification
- [ ] Load existing config or create new one
- [ ] Update config with token
- [ ] Save config to file
- [ ] Print success message with account info
- [ ] Handle errors gracefully (invalid token, network errors, etc.)
- [ ] Write unit tests for login logic
- [ ] Write integration tests

**Commit**: "feat(cli): implement login command"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Login command for OAuth token management
- Interactive and non-interactive modes
- Token verification against Put.io API
- Automatic config file creation and updates
```

---

### 5.2 Token Security Considerations
**Feature**: Secure token handling

**Security Measures** (modifiable):
- [ ] Set config file permissions to 0600 (owner read/write only)
- [ ] Warn if token is provided via command line (visible in shell history)
- [ ] Recommend interactive mode for better security
- [ ] Never log the full token (only first 8 chars)
- [ ] Consider keyring integration (future enhancement)

**Tasks**:
- [ ] Implement file permission setting after config save
- [ ] Add warning message for CLI token usage
- [ ] Add token masking in logs
- [ ] Write security tests

**Commit**: "feat(cli): add security measures for token handling"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Secure file permissions for config file (0600)
- Token masking in logs
- Security warnings for CLI token usage
```

---

## 6. List Command

### 6.1 List Command Interface
**Feature**: List files and folders from Put.io

**Command Signature** (modifiable):
```
putio-sync list [PATH] [OPTIONS]

Arguments:
  [PATH]    Remote folder path or ID to list [default: root]

Options:
  -i, --id <ID>                List folder by ID instead of path
  -r, --recursive              List files recursively
  -l, --long                   Long format with details (size, date, etc.)
  -f, --filter <PATTERN>       Filter files by name pattern (glob or regex)
  -t, --type <TYPE>            Filter by file type [possible values: file, folder, video, audio, image, archive]
  --min-size <SIZE>            Filter by minimum file size (e.g., 100MB, 1GB)
  --max-size <SIZE>            Filter by maximum file size
  --sort <FIELD>               Sort by field [possible values: name, size, date]
  --reverse                    Reverse sort order
  -o, --output <FORMAT>        Output format [default: table] [possible values: table, json, csv, tree]
  -h, --help                   Print help
```

**Output Format Options** (modifiable):

**Option A: Table Format** (default)
```
ID          Name                Size        Modified             Type
─────────────────────────────────────────────────────────────────────
12345678    Movies/             -           2025-11-06 10:30     folder
12345679    example.mkv         4.2 GB      2025-11-05 14:22     video
12345680    document.pdf        2.5 MB      2025-11-04 09:15     file
```

**Option B: Tree Format**
```
.
├── Movies/
│   ├── Action/
│   │   └── example.mkv (4.2 GB)
│   └── Comedy/
└── Documents/
    └── document.pdf (2.5 MB)
```

**Option C: JSON Format**
```json
{
  "files": [
    {
      "id": 12345678,
      "name": "Movies",
      "size": 0,
      "file_type": "folder",
      "parent_id": 0,
      "created_at": "2025-11-06T10:30:00Z"
    }
  ]
}
```

**Option D: CSV Format**
```csv
id,name,size,type,modified
12345678,Movies,,folder,2025-11-06T10:30:00Z
12345679,example.mkv,4498473984,video,2025-11-05T14:22:00Z
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/commands/list.rs`
- [ ] Define `ListArgs` struct with clap
- [ ] Implement folder listing by ID
- [ ] Implement folder listing by path (resolve path to ID)
- [ ] Implement `--recursive` flag for deep listing
- [ ] Implement `--filter` with glob pattern matching
- [ ] Implement `--type` filtering
- [ ] Implement size filtering (`--min-size`, `--max-size`)
- [ ] Parse human-readable sizes (KB, MB, GB, TB)
- [ ] Implement sorting by name, size, date
- [ ] Implement `--reverse` sort order
- [ ] Implement table output format with proper column alignment
- [ ] Implement JSON output format
- [ ] Implement CSV output format
- [ ] Implement tree output format (optional)
- [ ] Handle empty folders gracefully
- [ ] Handle API errors (folder not found, auth errors, etc.)
- [ ] Write unit tests for filtering logic
- [ ] Write unit tests for output formatters
- [ ] Write integration tests

**Commit**: "feat(cli): implement list command with filtering and formatting"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- List command for browsing Put.io files and folders
- Support for listing by ID or path
- Recursive listing option
- File filtering by name, type, and size
- Multiple output formats: table, json, csv, tree
- Sorting options
```

---

### 6.2 Path Resolution
**Feature**: Resolve folder paths to IDs

**Example**: `putio-sync list /Movies/Action` should resolve to the ID of the "Action" folder

**Tasks**:
- [ ] Create `putio-sync-core/src/api/path.rs`
- [ ] Implement `ApiClient::resolve_path(path: &str)` method
- [ ] Split path into components
- [ ] Traverse folders starting from root
- [ ] Return folder ID
- [ ] Handle path not found errors
- [ ] Cache path resolutions (optional)
- [ ] Write unit tests
- [ ] Write integration tests

**Commit**: "feat(core): implement path resolution for folder navigation"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Path resolution to convert folder paths to IDs
- Folder traversal logic
```

---

### 6.3 Filtering & Sorting Logic
**Feature**: Client-side filtering and sorting

**Tasks**:
- [ ] Create `putio-sync-core/src/filter.rs`
- [ ] Implement `FileFilter` struct with builder pattern
- [ ] Implement glob pattern matching for name filters
- [ ] Implement file type detection and filtering
- [ ] Implement size comparison logic
- [ ] Implement sorting functions (by name, size, date)
- [ ] Write comprehensive unit tests for all filters

**Commit**: "feat(core): implement file filtering and sorting"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- File filtering by name, type, and size
- Sorting by name, size, and date
- Glob pattern matching for name filters
```

---

### 6.4 Output Formatters
**Feature**: Multiple output format support

**Tasks**:
- [ ] Create `putio-sync-cli/src/output/mod.rs`
- [ ] Create `putio-sync-cli/src/output/table.rs`
- [ ] Create `putio-sync-cli/src/output/json.rs`
- [ ] Create `putio-sync-cli/src/output/csv.rs`
- [ ] Create `putio-sync-cli/src/output/tree.rs` (optional)
- [ ] Implement table formatter with column alignment
- [ ] Implement JSON formatter with pretty printing
- [ ] Implement CSV formatter
- [ ] Implement tree formatter with box-drawing characters
- [ ] Add human-readable size formatting (KB, MB, GB, TB)
- [ ] Add date/time formatting
- [ ] Write unit tests for each formatter

**Commit**: "feat(cli): add output formatters for list command"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Table output formatter with column alignment
- JSON output formatter
- CSV output formatter
- Tree output formatter
- Human-readable size and date formatting
```

---

## 7. Testing Strategy

### 7.1 Unit Tests
**Feature**: Comprehensive unit test coverage

**Test Coverage Goals**:
- Core: 80%+ coverage
- CLI: 70%+ coverage

**Tasks**:
- [ ] Write unit tests for all config operations
- [ ] Write unit tests for API client methods (with mocked HTTP)
- [ ] Write unit tests for error handling
- [ ] Write unit tests for filtering logic
- [ ] Write unit tests for output formatters
- [ ] Write unit tests for path resolution
- [ ] Use `mockito` or similar for HTTP mocking
- [ ] Use `assert_fs` for filesystem testing

**Commit**: "test: add comprehensive unit tests"

---

### 7.2 Integration Tests
**Feature**: End-to-end testing

**Tasks**:
- [ ] Create `putio-sync-cli/tests/` directory
- [ ] Write integration test for login flow
- [ ] Write integration test for list command
- [ ] Use temporary config files
- [ ] Mock API responses where possible
- [ ] Add tests for error scenarios
- [ ] Document how to run integration tests

**Commit**: "test: add integration tests for login and list commands"

---

## 8. GitHub Workflows

### 8.1 CI Workflow
**Feature**: Continuous Integration on GitHub Actions

**Workflow File**: `.github/workflows/ci.yml`

**Workflow Steps** (modifiable):
- Trigger on: push, pull request to main
- Run on: Ubuntu, macOS, Windows
- Rust versions: stable, beta (optional)
- Steps:
  1. Checkout code
  2. Setup Rust toolchain
  3. Cache cargo dependencies
  4. Run `cargo fmt --check`
  5. Run `cargo clippy -- -D warnings`
  6. Run `cargo test --all-features`
  7. Run `cargo build --release`

**Tasks**:
- [ ] Create `.github/workflows/ci.yml`
- [ ] Configure matrix for multiple OS (Linux, macOS, Windows)
- [ ] Add Rust toolchain setup
- [ ] Add dependency caching
- [ ] Add formatting check (rustfmt)
- [ ] Add linting (clippy)
- [ ] Add test execution
- [ ] Add release build
- [ ] Configure to fail on warnings

**Commit**: "ci: add GitHub Actions CI workflow"

---

### 8.2 Release Workflow
**Feature**: Automated binary building for releases

**Workflow File**: `.github/workflows/release.yml`

**Workflow Steps** (modifiable):
- Trigger on: tag push (v*.*.*)
- Build for: Linux (x86_64, arm64), macOS (x86_64, arm64), Windows (x86_64)
- Steps:
  1. Checkout code
  2. Setup Rust toolchain
  3. Build release binary for each target
  4. Strip binaries (Linux/macOS)
  5. Create tar.gz/zip archives
  6. Generate checksums
  7. Create GitHub release
  8. Upload artifacts

**Tasks**:
- [ ] Create `.github/workflows/release.yml`
- [ ] Configure cross-compilation targets
- [ ] Add binary stripping step
- [ ] Add archiving step
- [ ] Add checksum generation
- [ ] Configure GitHub release creation
- [ ] Test release workflow

**Commit**: "ci: add release workflow for multi-platform binaries"

---

### 8.3 Code Coverage Workflow
**Feature**: Track test coverage over time

**Tasks**:
- [ ] Create `.github/workflows/coverage.yml`
- [ ] Use `tarpaulin` or `llvm-cov` for coverage
- [ ] Upload to Codecov or Coveralls
- [ ] Add coverage badge to README
- [ ] Configure minimum coverage threshold

**Commit**: "ci: add code coverage tracking"

---

## 9. Documentation

### 9.1 Code Documentation
**Feature**: Inline documentation for all public APIs

**Tasks**:
- [ ] Add module-level documentation
- [ ] Add function/struct documentation with examples
- [ ] Add usage examples in doc comments
- [ ] Generate docs with `cargo doc`
- [ ] Configure docs.rs metadata in Cargo.toml

**Commit**: "docs: add comprehensive inline documentation"

---

### 9.2 User Documentation
**Feature**: User-facing documentation

**Tasks**:
- [ ] Update README.md with project overview
- [ ] Add installation instructions
- [ ] Add usage examples for login command
- [ ] Add usage examples for list command
- [ ] Add troubleshooting section
- [ ] Document how to get Put.io OAuth token
- [ ] Add configuration file documentation

**Commit**: "docs: add user documentation for login and list commands"

---

### 9.3 Changelog Management
**Feature**: Maintain changelogs for both crates

**Tasks**:
- [ ] Create `putio-sync-core/CHANGELOG.md`
- [ ] Create `putio-sync-cli/CHANGELOG.md`
- [ ] Follow Keep a Changelog format
- [ ] Update changelog with each feature commit
- [ ] Document breaking changes, new features, bug fixes

**Commit**: "docs: initialize changelogs for core and cli crates"

---

## 10. Implementation Order

### Phase 1: Foundation (Commits 1-5)
1. ✓ Initialize cargo workspace
2. ✓ Setup logging infrastructure
3. ✓ Implement error handling
4. ✓ Implement configuration management
5. ✓ Add config validation

### Phase 2: API Client (Commits 6-9)
6. ✓ Implement base API client
7. ✓ Add API data models
8. ✓ Implement file/folder endpoints
9. ✓ Implement account verification

### Phase 3: CLI Foundation (Commits 10-11)
10. ✓ Implement CLI structure with clap
11. ✓ Integrate CLI logging

### Phase 4: Login Command (Commits 12-13)
12. ✓ Implement login command
13. ✓ Add token security measures

### Phase 5: List Command (Commits 14-17)
14. ✓ Implement basic list command
15. ✓ Implement path resolution
16. ✓ Add filtering and sorting
17. ✓ Add output formatters

### Phase 6: Testing (Commits 18-19)
18. ✓ Add comprehensive unit tests
19. ✓ Add integration tests

### Phase 7: CI/CD (Commits 20-22)
20. ✓ Add CI workflow
21. ✓ Add release workflow
22. ✓ Add coverage tracking

### Phase 8: Documentation (Commits 23-25)
23. ✓ Add code documentation
24. ✓ Add user documentation
25. ✓ Initialize changelogs

---

## 11. Command Interface Summary

### Login Command Options
```
putio-sync login [OPTIONS]

Options:
  -t, --token <TOKEN>    Put.io OAuth token
  --interactive          Prompt for token interactively [default if no token provided]
  --verify               Verify token before saving [default: true]
  --no-verify            Skip token verification
  -h, --help             Print help
```

### List Command Options
```
putio-sync list [PATH] [OPTIONS]

Arguments:
  [PATH]                 Remote folder path or ID [default: /]

Options:
  -i, --id <ID>          List folder by ID
  -r, --recursive        List recursively
  -l, --long             Long format with details
  -f, --filter <PATTERN> Filter by name pattern
  -t, --type <TYPE>      Filter by type [file, folder, video, audio, image, archive]
  --min-size <SIZE>      Minimum file size (e.g., 100MB)
  --max-size <SIZE>      Maximum file size
  --sort <FIELD>         Sort by [name, size, date]
  --reverse              Reverse sort order
  -o, --output <FORMAT>  Output format [table, json, csv, tree] [default: table]
  -h, --help             Print help
```

### Global Options
```
putio-sync [OPTIONS] <COMMAND>

Options:
  -c, --config <PATH>    Config file path [default: ~/.putiosync/config.toml]
  -v, --verbose...       Increase verbosity (-v, -vv, -vvv)
  -q, --quiet            Suppress non-error output
  -h, --help             Print help
  -V, --version          Print version
```

---

## 12. Success Criteria

### Login Command
- [ ] User can authenticate with Put.io token
- [ ] Token is saved to config file securely
- [ ] Token is verified against API
- [ ] Config file has correct permissions (0600)
- [ ] User sees success message with account info
- [ ] Errors are handled gracefully

### List Command
- [ ] User can list files in root folder
- [ ] User can list files in specific folder by ID
- [ ] User can list files in specific folder by path
- [ ] User can filter files by name, type, size
- [ ] User can sort files by name, size, date
- [ ] User can view output in multiple formats
- [ ] Recursive listing works correctly
- [ ] Errors are handled gracefully

### Overall Project
- [ ] All tests pass on CI
- [ ] Code passes clippy with no warnings
- [ ] Code is formatted with rustfmt
- [ ] Documentation is complete
- [ ] Binaries build successfully for all platforms
- [ ] Changelogs are up to date

---

## 13. Future Enhancements (Post Phase 1)

- OAuth flow for login command
- Keyring integration for token storage
- Shell completion scripts
- Interactive mode for list command (TUI)
- File preview support
- Bookmark/favorite folders
- Batch operations
- Configuration profiles (multiple accounts)

---

## Notes

- All commits should follow conventional commit format
- Each feature should be independently testable
- Break down large features into smaller commits if needed
- Update changelogs with every feature commit
- Run tests before each commit
- Keep PRs focused on single features when possible
