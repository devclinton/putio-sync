# putio-sync Workplan - Phase 3: CLI Refactoring & Web Server

## Progress Tracker

### Phase 1: CLI Command Restructuring ✅ 100% COMPLETED
- [x] 1.1 Remove Old Login Command - COMPLETED (commit: fa90119)
- [x] 1.2 Rename get-token to login - COMPLETED (commit: fa90119)
- [x] 1.3 Create Files Command Structure - COMPLETED (commit: 5dfe713)
- [x] 1.4 Restructure Transfers Command - COMPLETED (commit: ed91ddd)

**Status**: Phase 1 COMPLETE! 🎉

---

## Phase 1 Summary - CLI Command Restructuring ✅

### Completed Changes

**1.1 & 1.2: Login Command Refactoring**
- Removed old login command with username/password
- Renamed `get-token` to `login`
- Token saves by default (use `--no-save` to prevent)
- Commit: fa90119

**1.3: Files Command Structure**  
- Created `FilesCommands` enum
- New: `putio-sync files list` and `putio-sync files delete`
- Old commands hidden but functional for backward compatibility
- Commit: 5dfe713

**1.4: Transfers Command Restructuring**
- Added `Add` variant to `TransfersCommands`
- New: `putio-sync transfers add [SOURCES]...`
- Old `create` command hidden but functional
- Commit: ed91ddd

### New Command Structure

```bash
# Authentication
putio-sync login [--no-save] [--no-browser]

# File Management  
putio-sync files list [OPTIONS]
putio-sync files delete [OPTIONS]

# Transfer Management
putio-sync transfers list
putio-sync transfers add [SOURCES]...
putio-sync transfers cancel <IDS>
putio-sync transfers retry <ID>

# Queue Management
putio-sync queue list
putio-sync queue add [FILES]
putio-sync queue pause [FILES]
putio-sync queue resume [FILES]
putio-sync queue remove <FILES>
putio-sync queue clear
putio-sync queue priority <FILE> <PRIORITY>
putio-sync queue stats

# Other Commands (unchanged)
putio-sync download [OPTIONS]
putio-sync stat [OPTIONS]
putio-sync history [OPTIONS]
```

### Backward Compatibility

All old commands still work but are hidden from help:
- `putio-sync list` → use `putio-sync files list`
- `putio-sync delete` → use `putio-sync files delete`
- `putio-sync create` → use `putio-sync transfers add`

---

## Next Phase: Watch Configuration

### Phase 2.1: Add Watch Config to Config File ✅ COMPLETED

**Goal**: Configure watch directories in config.toml

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
- [x] Update WatchConfig struct in putio-sync-core/src/config.rs - COMPLETED
- [x] Add enabled field (bool) - COMPLETED
- [x] Add paths field (Vec<PathBuf>) - COMPLETED (changed from Vec<String>)
- [x] Add parent_folder_id field (Option<i64>) - COMPLETED
- [x] Add validation for watch paths - COMPLETED
- [x] Update config example file - COMPLETED
- [x] Update config tests - COMPLETED
- [x] Test and commit - COMPLETED (commit: 7a6b7b0)

**Status**: Phase 2.1 COMPLETE! ✅

**Changes Made**:
- Added `enabled: bool` field to WatchConfig
- Changed `paths: Vec<String>` to `paths: Vec<PathBuf>` for type safety
- Added `parent_folder_id: Option<i64>` field
- Added validation for watch paths (checks existence and directory type)
- Updated ConfigBuilder with new methods: `watch_enabled()`, `watch_paths()`, `watch_parent_folder_id()`
- Updated all tests and documentation examples
- All 261 tests passing

---

## Future Phases

### Phase 3: API Crate (putio-sync-api) ✅ COMPLETED

**Goal**: Create a REST API crate for putio-sync that can be used by both CLI and web server

**Status**: Phase 3 COMPLETE! 🎉

**Architecture**:
```
putio-sync-api/
├── src/
│   ├── lib.rs           # Public API exports
│   ├── routes/          # HTTP route handlers
│   │   ├── mod.rs
│   │   ├── files.rs     # Put.io file operations
│   │   ├── local.rs     # Local file operations
│   │   ├── transfers.rs # Transfer operations
│   │   └── queue.rs     # Queue operations
│   ├── handlers/        # Business logic
│   │   ├── mod.rs
│   │   ├── files.rs
│   │   ├── local.rs
│   │   ├── transfers.rs
│   │   └── queue.rs
│   ├── models/          # Request/Response models
│   │   ├── mod.rs
│   │   ├── files.rs
│   │   ├── transfers.rs
│   │   └── queue.rs
│   ├── middleware/      # Security and auth middleware
│   │   ├── mod.rs
│   │   └── auth.rs
│   └── error.rs         # API error types
└── Cargo.toml
```

**API Endpoints**:

*Put.io Files* (`/api/files`)
- `GET /api/files` - List files (with query params for path, filter)
- `GET /api/files/:id` - Get file by ID
- `DELETE /api/files/:id` - Delete file
- `DELETE /api/files/bulk` - Delete multiple files

*Local Files* (`/api/local`)
- `GET /api/local` - List local files
- `GET /api/local/:path` - Get local file info
- `DELETE /api/local/:path` - Delete local file

*Transfers* (`/api/transfers`)
- `GET /api/transfers` - List transfers
- `POST /api/transfers` - Add new transfer (torrent/magnet)
- `DELETE /api/transfers/:id` - Cancel transfer
- `POST /api/transfers/:id/retry` - Retry transfer
- `POST /api/transfers/:id/pause` - Pause transfer

*Queue* (`/api/queue`)
- `GET /api/queue` - List queue items
- `POST /api/queue` - Add to queue
- `DELETE /api/queue/:id` - Remove from queue
- `POST /api/queue/reorder` - Reorder queue items
- `POST /api/queue/:id/priority` - Change priority
- `POST /api/queue/:id/blacklist` - Blacklist file

**Tasks**:
- [x] Create putio-sync-api crate structure - COMPLETED
- [x] Add to workspace - COMPLETED
- [x] Define common error types - COMPLETED
- [x] Create request/response models - COMPLETED
- [x] Implement files handlers and routes - COMPLETED
- [x] Implement local files handlers and routes - COMPLETED
- [x] Implement transfers handlers and routes - COMPLETED
- [x] Implement queue handlers and routes - COMPLETED
- [x] Add authentication middleware - COMPLETED (placeholder)
- [x] Add CORS middleware - COMPLETED
- [x] Add logging middleware - COMPLETED (via TraceLayer)
- [x] Write unit tests for handlers - COMPLETED (38 tests)
- [x] Write integration tests for routes - COMPLETED
- [x] Test and commit - COMPLETED (commit: 9c70fc3)

**Commit**: 9c70fc3

**Test Results**: 38 unit tests passing, 3 doc tests (2 passed, 1 ignored)

**Dependencies**:
- axum (web framework)
- tower (middleware)
- serde (serialization)
- putio-sync-core (shared logic)

### Phase 4: Web Server (putio-sync-web)
- Create web server crate
- Integrate API routes
- Add shared application state

### Phase 5: Unified Daemon Mode
- Create daemon command
- Support --download, --watch, --server flags
- Thread coordination

### Phase 6: Testing & Documentation
- Integration tests
- Update README
- Migration guide

---

## Commits Log

| Commit | Phase | Description |
|--------|-------|-------------|
| fa90119 | 1.1-1.2 | Login command refactoring |
| 7d25277 | - | Workplan created |
| 5dfe713 | 1.3 | Files command structure |
| 68f975c | - | Workplan update (1.3 done) |
| ed91ddd | 1.4 | Transfers restructure |
| 4f42099 | 1 | CHANGELOG documentation for Phase 1 |
| 7a6b7b0 | 2.1 | Watch directory configuration |
| db26a73 | - | Workplan update (Phase 2.1 done) |
| d2360ba | - | Workplan Phase 3 detailed plan |
| 9c70fc3 | 3 | REST API crate implementation |

---

## Session Summary

**Phase 1: CLI Command Restructuring - COMPLETE ✅**

All CLI commands have been successfully restructured with:
- Cleaner command hierarchy
- Better organization (files, transfers as parent commands)
- Full backward compatibility
- Hidden deprecated commands

**Phase 2.1: Watch Configuration - COMPLETE ✅**

Watch configuration has been enhanced:
- Added `enabled` field for easy on/off toggle
- Changed `paths` to Vec<PathBuf> for type safety
- Added `parent_folder_id` for upload destination
- Added path validation (existence and directory checks)
- Updated ConfigBuilder with new helper methods
- All 261 tests passing

**Phase 3: API Crate - COMPLETE ✅**

REST API crate fully implemented:
- Complete HTTP API for files, transfers, queue, and local files
- Axum web framework with proper state management
- Comprehensive error handling and response types
- Authentication middleware framework
- CORS and tracing middleware
- 38 unit tests (100% passing)
- 20 source files created
- 1,938 lines of code added

**Next**: Phase 4 - Web Server (putio-sync-web) or Phase 5 - Unified Daemon Mode
