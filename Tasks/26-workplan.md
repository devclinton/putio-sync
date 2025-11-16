# putio-sync Workplan - Phase 3: CLI Refactoring & Web Server

## Progress Tracker

### Phase 1: CLI Command Restructuring ✅ PARTIALLY COMPLETED
- [x] 1.1 Remove Old Login Command - COMPLETED (commit: fa90119)
- [x] 1.2 Rename get-token to login - COMPLETED (commit: fa90119)
- [ ] 1.3 Create Files Command Structure (in progress)
- [ ] 1.4 Restructure Transfers Command

**Next**: Continue with Phase 1.3 - Create Files Command Structure

---

## Completed Work

### ✅ 1.1 & 1.2: Login Command Refactoring (COMPLETED)
**Commit**: fa90119
**Date**: 2025-11-15

**Changes Made**:
- Removed old login command with username/password authentication
- Renamed `get-token` command to `login`
- Changed behavior: token now saves by default (use `--no-save` to prevent)
- Updated all command references and documentation
- Removed login integration tests

**Breaking Changes**:
- Old `login` command removed
- `get-token` renamed to `login`  
- Token saves by default now

**Files Modified**:
- putio-sync-cli/src/commands/get_token.rs → login.rs (renamed)
- putio-sync-cli/src/commands/mod.rs
- putio-sync-cli/src/cli.rs
- putio-sync-cli/src/main.rs
- putio-sync-cli/tests/login_integration.rs (deleted)

---

## In Progress

### 🚧 1.3: Create Files Command Structure (IN PROGRESS)

**Goal**: Restructure commands to group file operations

**Current Structure**:
```bash
putio-sync list
putio-sync delete
```

**New Structure**:
```bash
putio-sync files list
putio-sync files delete
```

**Tasks**:
- [ ] Create FilesCommands enum in cli.rs
- [ ] Move List and Delete into FilesCommands
- [ ] Update Commands enum
- [ ] Update main.rs routing
- [ ] Update tests
- [ ] Test and commit

---

## Remaining Work

### 📋 1.4: Restructure Transfers Command

**Current**:
```bash
putio-sync create [SOURCES]...
putio-sync transfers list
putio-sync transfers cancel  
putio-sync transfers retry
```

**Target**:
```bash
putio-sync transfers list
putio-sync transfers add [SOURCES]...
putio-sync transfers cancel
putio-sync transfers retry
```

### 📋 Phase 2: Watch Configuration
- Add watch directories to config.toml
- Support multiple watch paths
- CLI/config merge logic

### 📋 Phase 3: API Crate
- Create putio-sync-api crate
- Implement REST endpoints
- Add security middleware

### 📋 Phase 4: Web Server
- Create putio-sync-web crate
- Integrate API routes
- Add daemon mode support

### 📋 Phase 5: Testing & Documentation
- Integration tests
- Update README
- Migration guide

---

## Session Summary

**Current Status**: Phase 1.3 in progress
**Last Commit**: fa90119 (Login command refactoring)
**Next Step**: Complete files command structure
