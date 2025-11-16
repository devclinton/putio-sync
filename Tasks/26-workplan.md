# putio-sync Workplan - Phase 3: CLI Refactoring & Web Server

## Overview
This workplan covers a major refactoring and the implementation of a web server for `putio-sync`:
- CLI command restructuring (remove login, rename get-token, reorganize transfers)
- Unified daemon mode supporting both sync and watch simultaneously
- Watch directory configuration in config file
- New `putio-sync-api` crate with REST API endpoints
- New `putio-sync-web` crate with HTTP server
- Thread model for daemon mode with server
- Test-driven development approach

---

## 1. CLI Command Restructuring

### 1.1 Remove Old Login Command
**Feature**: Remove the existing login command in favor of simplified get-token

**Tasks**:
- [ ] Remove `putio-sync-cli/src/commands/login.rs`
- [ ] Remove login command from CLI enum in `cli.rs`
- [ ] Remove login tests from integration tests
- [ ] Update documentation to remove login references

**Commit**: "refactor(cli): remove old login command"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Removed
- Old login command (replaced by simplified get-token workflow)
```

---

### 1.2 Rename get-token to login
**Feature**: Simplify authentication by renaming get-token to login

**Tasks**:
- [ ] Rename `execute_get_token` to `execute_login` in `commands/get_token.rs`
- [ ] Update CLI enum to use `Login` instead of `GetToken`
- [ ] Update all command references in main.rs
- [ ] Keep backward compatibility by accepting both `--save` and making it default
- [ ] Update help text and documentation
- [ ] Update tests to reflect new command name
- [ ] Update README examples

**Commit**: "refactor(cli): rename get-token to login for simpler auth"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Changed
- Renamed `get-token` command to `login` for simpler authentication workflow
- Token is now saved to config by default
```

---

### 1.3 Restructure Transfers Command
**Feature**: Move create command under transfers as a subcommand

**Current Structure**:
```
putio-sync create [SOURCES]...
putio-sync transfers list
putio-sync transfers cancel
putio-sync transfers retry
```

**New Structure**:
```
putio-sync transfers list
putio-sync transfers add [SOURCES]...
putio-sync transfers cancel
putio-sync transfers retry
```

**Tasks**:
- [ ] Update `TransfersCommands` enum in `cli.rs`
- [ ] Add `Add` variant to `TransfersCommands` with same args as old create
- [ ] Move create command logic to `commands/transfers.rs` as `execute_add`
- [ ] Update main.rs to route `transfers add` to execute_add
- [ ] Remove standalone create command from CLI
- [ ] Update all documentation and help text
- [ ] Update README examples
- [ ] Maintain backward compatibility via hidden alias if needed

**Commit**: "refactor(cli): move create command under transfers"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Changed
- Moved `create` command under `transfers` as `transfers add`
- More logical command structure for transfer management
```

---

## 2. Watch Configuration

### 2.1 Add Watch Config to Config File
**Feature**: Configure watch directories in config.toml instead of CLI only

**Config Schema**:
```toml
[watch]
enabled = false
paths = [
    "/path/to/torrents",
    "/another/watch/directory"
]
refresh_cycle = 60  # seconds
parent_folder_id = 0
```

**Tasks**:
- [ ] Update `WatchConfig` struct in `putio-sync-core/src/config.rs`
- [ ] Add `enabled` field to enable/disable watching
- [ ] Add `paths` field as Vec<PathBuf> for watch directories
- [ ] Add validation for watch paths (must exist if enabled)
- [ ] Add defaults (enabled=false, paths=[], refresh_cycle=60)
- [ ] Update config example file
- [ ] Update config serialization/deserialization
- [ ] Write unit tests for config validation

**Commit**: "feat(core): add watch directory configuration to config file"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- Watch directory configuration in config.toml
- Support for multiple watch paths
- Enable/disable toggle for watch functionality
```

---

### 2.2 Update Transfers Add Command for Watch Config
**Feature**: Use watch config from file if CLI args not provided

**Tasks**:
- [ ] Update `transfers add` command to check config for watch paths
- [ ] Merge CLI watch args with config watch paths
- [ ] CLI args take precedence over config
- [ ] Update help text to mention config file option
- [ ] Write tests for config/CLI merge logic

**Commit**: "feat(cli): use watch config from file in transfers add"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Support for reading watch directories from config file
- CLI arguments override config file settings
```

---

## 3. Unified Daemon Mode

### 3.1 Download Daemon Mode Enhancement
**Feature**: Enhance download daemon to work alongside watch daemon

**Current**: Download daemon only syncs files
**New**: Download daemon coordinates with watch daemon

**Tasks**:
- [ ] Create `putio-sync-cli/src/daemon/mod.rs` module
- [ ] Create `putio-sync-cli/src/daemon/download.rs` for download loop
- [ ] Create `putio-sync-cli/src/daemon/watch.rs` for watch loop
- [ ] Extract download daemon logic from download command
- [ ] Make download daemon cancellable via tokio::select
- [ ] Add proper shutdown signaling
- [ ] Write unit tests for daemon components

**Commit**: "refactor(cli): extract daemon mode into separate module"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Changed
- Refactored daemon mode into dedicated module
- Improved daemon shutdown handling
```

---

### 3.2 Unified Daemon Command
**Feature**: Run both download and watch daemons simultaneously

**Command Signature**:
```
putio-sync daemon [OPTIONS]

Options:
  --download                Enable download daemon (sync files)
  --watch                   Enable watch daemon (monitor torrent directories)
  --server                  Enable HTTP server
  --port <PORT>             Server port [default: 15432]
  --download-interval <SEC> Download check interval [default: 300]
  --watch-interval <SEC>    Watch check interval [default: 60]
  -h, --help                Print help
```

**Tasks**:
- [ ] Create new `Daemon` command in CLI enum
- [ ] Create `putio-sync-cli/src/commands/daemon.rs`
- [ ] Implement daemon command with boolean flags
- [ ] Spawn download daemon thread if `--download` enabled
- [ ] Spawn watch daemon thread if `--watch` enabled
- [ ] Spawn server thread if `--server` enabled
- [ ] Coordinate graceful shutdown across all threads
- [ ] Add signal handling (SIGTERM, SIGINT)
- [ ] Write integration tests for daemon mode

**Commit**: "feat(cli): add unified daemon command for download/watch/server"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Unified `daemon` command supporting download, watch, and server modes
- Concurrent execution of multiple daemon modes
- Graceful shutdown coordination across daemon threads
```

---

## 4. API Crate (putio-sync-api)

### 4.1 Create API Crate
**Feature**: New crate for REST API types and handlers

**Crate Structure**:
```
putio-sync-api/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── models/          # API request/response models
│   ├── handlers/        # Route handlers
│   ├── routes.rs        # Route definitions
│   └── error.rs         # API error types
```

**Tasks**:
- [ ] Create new crate: `cargo new --lib putio-sync-api`
- [ ] Add to workspace in root Cargo.toml
- [ ] Add dependencies: axum, serde, serde_json, tokio
- [ ] Add putio-sync-core as dependency
- [ ] Create initial lib.rs with module structure
- [ ] Add API error types with proper HTTP status codes
- [ ] Write basic tests

**Commit**: "feat(api): create putio-sync-api crate"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- Initial API crate structure
- API error types and handlers
- Foundation for REST endpoints
```

---

### 4.2 API Models
**Feature**: Request and response models for API endpoints

**Models Needed**:
```rust
// Files
pub struct FileListRequest { pub parent_id: Option<i64> }
pub struct FileListResponse { pub files: Vec<FileEntry> }
pub struct DeleteFilesRequest { pub file_ids: Vec<i64> }

// Transfers
pub struct TransferListResponse { pub transfers: Vec<Transfer> }
pub struct AddTransferRequest { pub url: Option<String>, pub parent_id: Option<i64> }
pub struct CancelTransferRequest { pub transfer_ids: Vec<i64> }
pub struct PauseTransferRequest { pub transfer_id: i64 }

// Queue
pub struct QueueListResponse { pub items: Vec<QueueItem> }
pub struct AddToQueueRequest { pub file_ids: Vec<i64>, pub priority: Option<i64> }
pub struct ReorderQueueRequest { pub item_id: i64, pub new_position: i64 }
pub struct BlacklistRequest { pub file_id: i64 }

// Local files
pub struct LocalFileListRequest { pub path: Option<String> }
pub struct LocalFileListResponse { pub files: Vec<LocalFile> }
pub struct LocalDeleteRequest { pub paths: Vec<String> }
```

**Tasks**:
- [ ] Create `putio-sync-api/src/models/files.rs`
- [ ] Create `putio-sync-api/src/models/transfers.rs`
- [ ] Create `putio-sync-api/src/models/queue.rs`
- [ ] Create `putio-sync-api/src/models/local.rs`
- [ ] Add serde derives for all models
- [ ] Add validation where needed
- [ ] Write serialization tests

**Commit**: "feat(api): add API request/response models"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- Complete API models for all endpoints
- Request/response types with serde support
- Input validation for API models
```

---

### 4.3 Remote Files Endpoints
**Feature**: API endpoints for Put.io file operations

**Endpoints**:
- `GET /api/files` - List files from Put.io
- `DELETE /api/files` - Delete files from Put.io

**Tasks**:
- [ ] Create `putio-sync-api/src/handlers/files.rs`
- [ ] Implement `list_files` handler
  - Query Put.io API via core ApiClient
  - Apply filters if provided
  - Return JSON response
- [ ] Implement `delete_files` handler
  - Validate file IDs
  - Call core delete API
  - Return success/error response
- [ ] Add route definitions in `routes.rs`
- [ ] Write handler tests with mock ApiClient

**Commit**: "feat(api): add remote files endpoints"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- GET /api/files endpoint for listing remote files
- DELETE /api/files endpoint for deleting remote files
```

---

### 4.4 Local Files Endpoints
**Feature**: API endpoints for local filesystem operations

**Endpoints**:
- `GET /api/local/files` - List files in download directory
- `DELETE /api/local/files` - Delete local files

**Tasks**:
- [ ] Create `putio-sync-api/src/handlers/local.rs`
- [ ] Implement `list_local_files` handler
  - Read from configured download directory
  - Return file metadata (name, size, modified time)
  - Support subdirectory traversal
- [ ] Implement `delete_local_files` handler
  - Validate paths are within download directory (security!)
  - Delete files safely
  - Return success/error response
- [ ] Add security checks to prevent path traversal attacks
- [ ] Write handler tests

**Commit**: "feat(api): add local files endpoints"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- GET /api/local/files endpoint for listing local files
- DELETE /api/local/files endpoint for deleting local files
- Path traversal protection for security
```

---

### 4.5 Transfers Endpoints
**Feature**: API endpoints for transfer management

**Endpoints**:
- `GET /api/transfers` - List all transfers
- `POST /api/transfers` - Add new transfer (torrent/magnet)
- `POST /api/transfers/cancel` - Cancel transfers
- `POST /api/transfers/pause` - Pause transfer (if supported by Put.io)

**Tasks**:
- [ ] Create `putio-sync-api/src/handlers/transfers.rs`
- [ ] Implement `list_transfers` handler
- [ ] Implement `add_transfer` handler
  - Support both URL and file upload
  - Validate input
- [ ] Implement `cancel_transfers` handler
- [ ] Implement `pause_transfer` handler (check Put.io API support)
- [ ] Write handler tests

**Commit**: "feat(api): add transfers management endpoints"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- GET /api/transfers endpoint for listing transfers
- POST /api/transfers endpoint for adding torrents
- POST /api/transfers/cancel endpoint
- POST /api/transfers/pause endpoint
```

---

### 4.6 Queue Endpoints
**Feature**: API endpoints for download queue management

**Endpoints**:
- `GET /api/queue` - List queue items
- `POST /api/queue` - Add files to queue
- `DELETE /api/queue/:id` - Remove from queue
- `PUT /api/queue/:id/priority` - Reorder queue
- `POST /api/queue/blacklist` - Blacklist file from auto-download

**Tasks**:
- [ ] Create `putio-sync-api/src/handlers/queue.rs`
- [ ] Implement `list_queue` handler
- [ ] Implement `add_to_queue` handler
- [ ] Implement `remove_from_queue` handler
- [ ] Implement `update_priority` handler (reorder)
- [ ] Implement `blacklist_file` handler
- [ ] Add blacklist table to database schema if not exists
- [ ] Write handler tests

**Commit**: "feat(api): add queue management endpoints"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- Complete queue management API
- Queue reordering support
- File blacklisting functionality
```

---

### 4.7 Route Configuration
**Feature**: Centralized route configuration for API

**Tasks**:
- [ ] Create `putio-sync-api/src/routes.rs`
- [ ] Define router function returning axum Router
- [ ] Group routes by resource (/api/files, /api/transfers, etc.)
- [ ] Add middleware for logging requests
- [ ] Add middleware for error handling
- [ ] Add CORS support for web UI
- [ ] Write route integration tests

**Commit**: "feat(api): add route configuration and middleware"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- Centralized route configuration
- Request logging middleware
- Error handling middleware
- CORS support for web frontends
```

---

## 5. Web Server Crate (putio-sync-web)

### 5.1 Create Web Crate
**Feature**: New crate for HTTP server

**Crate Structure**:
```
putio-sync-web/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── server.rs        # Server initialization
│   └── state.rs         # Shared application state
```

**Tasks**:
- [ ] Create new crate: `cargo new --lib putio-sync-web`
- [ ] Add to workspace
- [ ] Add dependencies: axum, tokio, tower, tower-http
- [ ] Add putio-sync-core and putio-sync-api as dependencies
- [ ] Create server initialization code
- [ ] Add graceful shutdown handling
- [ ] Write basic server tests

**Commit**: "feat(web): create putio-sync-web crate with server"

**Changelog Entry** (`putio-sync-web`):
```markdown
### Added
- Initial web server crate
- HTTP server with graceful shutdown
- Foundation for REST API hosting
```

---

### 5.2 Application State
**Feature**: Shared state for API handlers

**State Includes**:
- Config
- Database connection pool
- ApiClient for Put.io
- Active download/watch tasks

**Tasks**:
- [ ] Create `putio-sync-web/src/state.rs`
- [ ] Define `AppState` struct with all shared resources
- [ ] Implement state initialization from config
- [ ] Add state cloning for multi-threaded access
- [ ] Add methods to access resources safely
- [ ] Write state management tests

**Commit**: "feat(web): add shared application state"

**Changelog Entry** (`putio-sync-web`):
```markdown
### Added
- Shared application state for API handlers
- Thread-safe resource access
- State initialization from configuration
```

---

### 5.3 Server Integration
**Feature**: Integrate API routes with web server

**Tasks**:
- [ ] Create `putio-sync-web/src/server.rs`
- [ ] Implement `start_server(config, port)` function
- [ ] Mount API routes from putio-sync-api
- [ ] Configure server with state
- [ ] Bind to configured port (default: 15432)
- [ ] Add health check endpoint: `GET /health`
- [ ] Write server integration tests

**Commit**: "feat(web): integrate API routes with server"

**Changelog Entry** (`putio-sync-web`):
```markdown
### Added
- HTTP server listening on port 15432
- Health check endpoint
- Full API route integration
```

---

## 6. Daemon Thread Model

### 6.1 Thread Architecture
**Feature**: Implement coordinated thread model for daemon mode

**Thread Model**:
```
Main Thread (Coordinator)
├── Download Thread
│   └── HTTP API client thread pool (for file listing)
├── Watch Thread
│   └── HTTP API client thread pool (for adding transfers)
├── Worker Thread Pool
│   └── Download worker threads (N workers)
└── HTTP Server Thread
    ├── Request handler threads (tokio runtime)
    └── API handler thread pool
```

**Tasks**:
- [ ] Create `putio-sync-cli/src/daemon/coordinator.rs`
- [ ] Implement thread spawn logic for each component
- [ ] Implement graceful shutdown signal propagation
- [ ] Add thread health monitoring
- [ ] Add panic recovery for individual threads
- [ ] Coordinate download workers with queue
- [ ] Write tests for thread coordination

**Commit**: "feat(daemon): implement coordinated thread model"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Multi-threaded daemon coordinator
- Health monitoring for daemon threads
- Panic recovery and graceful degradation
- Thread coordination for download/watch/server
```

---

### 6.2 Thread Communication
**Feature**: Inter-thread communication for daemon

**Communication Needs**:
- Download thread → Worker pool (enqueue downloads)
- Watch thread → Transfer API (add torrents)
- Server → All threads (status queries)
- All threads → Main (shutdown signals)

**Tasks**:
- [ ] Add tokio channels for thread communication
- [ ] Implement download queue channel
- [ ] Implement watch event channel
- [ ] Implement status query channel
- [ ] Implement shutdown signal broadcast
- [ ] Add timeout handling for communication
- [ ] Write communication tests

**Commit**: "feat(daemon): add inter-thread communication"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Inter-thread communication channels
- Download queue coordination
- Status query mechanism
- Shutdown signal broadcasting
```

---

## 7. CLI Integration

### 7.1 Update Main Binary
**Feature**: Wire up daemon command in main CLI

**Tasks**:
- [ ] Update `putio-sync-cli/src/cli.rs` with Daemon command
- [ ] Update `putio-sync-cli/src/main.rs` to handle daemon command
- [ ] Add server flag to daemon command
- [ ] Initialize web server if --server flag provided
- [ ] Update help text
- [ ] Write integration tests

**Commit**: "feat(cli): integrate daemon command with server support"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Daemon command support in main binary
- Server mode flag for running HTTP API
```

---

### 7.2 Backward Compatibility
**Feature**: Maintain backward compatibility for existing commands

**Tasks**:
- [ ] Add hidden alias for `create` → `transfers add`
- [ ] Add deprecation warnings for old commands
- [ ] Update migration guide in documentation
- [ ] Test all old command patterns still work

**Commit**: "feat(cli): add backward compatibility for command changes"

**Changelog Entry** (`putio-sync-cli`):
```markdown
### Added
- Backward compatibility aliases for renamed commands
- Deprecation warnings for old command syntax
```

---

## 8. Testing & Documentation

### 8.1 Integration Testing
**Feature**: End-to-end tests for new features

**Test Scenarios**:
- [ ] Test daemon mode with download only
- [ ] Test daemon mode with watch only
- [ ] Test daemon mode with server only
- [ ] Test daemon mode with all three enabled
- [ ] Test API endpoints with real server
- [ ] Test graceful shutdown scenarios
- [ ] Test thread recovery from failures
- [ ] Test queue operations via API
- [ ] Test concurrent API requests

**Commit**: "test: add integration tests for daemon and API"

---

### 8.2 Documentation Updates
**Feature**: Comprehensive documentation for new features

**Tasks**:
- [ ] Update README with new command structure
- [ ] Document daemon command usage
- [ ] Document API endpoints with examples
- [ ] Add API documentation (OpenAPI/Swagger)
- [ ] Add migration guide for command changes
- [ ] Add troubleshooting section for daemon mode
- [ ] Add thread model diagram
- [ ] Update example config with watch settings

**Commit**: "docs: update documentation for daemon and API features"

---

### 8.3 Performance Testing
**Feature**: Validate performance of daemon mode

**Tasks**:
- [ ] Test daemon with large file lists (10k+ files)
- [ ] Test server under load (100+ concurrent requests)
- [ ] Profile memory usage of daemon mode
- [ ] Measure thread overhead
- [ ] Optimize if bottlenecks found
- [ ] Document performance characteristics

**Commit**: "perf: optimize daemon and server performance"

---

## 9. Security

### 9.1 API Security
**Feature**: Secure API endpoints

**Tasks**:
- [ ] Add authentication middleware (API key or JWT)
- [ ] Add rate limiting middleware
- [ ] Validate all input parameters
- [ ] Sanitize file paths for local operations
- [ ] Add CORS configuration
- [ ] Add security headers
- [ ] Write security tests (path traversal, injection, etc.)

**Commit**: "feat(api): add security middleware and validation"

**Changelog Entry** (`putio-sync-api`):
```markdown
### Added
- API authentication support
- Rate limiting protection
- Input validation and sanitization
- Security headers and CORS
```

---

### 9.2 File Blacklisting
**Feature**: Blacklist files from auto-download

**Database Schema**:
```sql
CREATE TABLE blacklist (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    pattern TEXT,  -- glob pattern if blacklisting by pattern
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(file_id)
);
```

**Tasks**:
- [ ] Add blacklist table to database schema
- [ ] Create BlacklistRepository in core
- [ ] Implement add/remove/list blacklist methods
- [ ] Check blacklist before adding to download queue
- [ ] Add blacklist CLI commands
- [ ] Add blacklist API endpoint
- [ ] Write tests

**Commit**: "feat: add file blacklisting functionality"

**Changelog Entry** (`putio-sync-core`):
```markdown
### Added
- File blacklist database table
- Blacklist checking in download queue
- Pattern-based blacklisting support
```

---

## 10. Deployment

### 10.1 Docker Updates
**Feature**: Update Docker image for server mode

**Tasks**:
- [ ] Update Dockerfile to support server mode
- [ ] Expose port 15432 for API
- [ ] Add environment variables for server config
- [ ] Update docker-compose examples for server
- [ ] Test Docker deployment

**Commit**: "feat(docker): add server mode support to Docker image"

---

### 10.2 Systemd Service
**Feature**: Systemd service file for daemon mode

**Tasks**:
- [ ] Create putio-sync.service file
- [ ] Support daemon mode with all options
- [ ] Add restart policies
- [ ] Add logging configuration
- [ ] Document installation process

**Commit**: "feat: add systemd service file for daemon mode"

---

## 11. Implementation Order

### Phase 1: CLI Refactoring (Week 1)
1. Remove old login command
2. Rename get-token to login
3. Restructure transfers command
4. Add watch config to config file
5. Test and commit

### Phase 2: API Foundation (Week 2)
1. Create putio-sync-api crate
2. Add API models
3. Implement file endpoints
4. Implement transfer endpoints
5. Test and commit

### Phase 3: Queue & Local (Week 3)
1. Implement queue endpoints
2. Implement local file endpoints
3. Add blacklisting
4. Test and commit

### Phase 4: Web Server (Week 4)
1. Create putio-sync-web crate
2. Add application state
3. Integrate API routes
4. Add security middleware
5. Test and commit

### Phase 5: Daemon Integration (Week 5)
1. Create unified daemon command
2. Implement thread coordinator
3. Add inter-thread communication
4. Integrate server with daemon
5. Test and commit

### Phase 6: Testing & Documentation (Week 6)
1. Integration testing
2. Performance testing
3. Update all documentation
4. Security audit
5. Final commit

---

## 12. Success Criteria

- [ ] All old commands work or have clear deprecation path
- [ ] Daemon mode runs stably for 24+ hours
- [ ] API handles 100+ concurrent requests
- [ ] All tests passing (unit + integration)
- [ ] Documentation complete and accurate
- [ ] Docker deployment works
- [ ] No memory leaks in daemon mode
- [ ] Graceful shutdown works reliably
