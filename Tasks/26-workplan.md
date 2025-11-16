# putio-sync Workplan - Phase 3: CLI Refactoring & Web Server

## Progress Tracker

### Phase 1: CLI Command Restructuring ✅ 75% COMPLETED
- [x] 1.1 Remove Old Login Command - COMPLETED (commit: fa90119)
- [x] 1.2 Rename get-token to login - COMPLETED (commit: fa90119)
- [x] 1.3 Create Files Command Structure - COMPLETED (commit: 5dfe713)
- [ ] 1.4 Restructure Transfers Command - NEXT

**Current**: Phase 1.4 - Move create under transfers

---

## Completed Work

### ✅ 1.1 & 1.2: Login Command Refactoring (COMPLETED)
**Commit**: fa90119

**Changes**:
- Removed old login command
- Renamed get-token to login
- Token saves by default (--no-save to skip)

### ✅ 1.3: Files Command Structure (COMPLETED)
**Commit**: 5dfe713

**Changes**:
- Created FilesCommands enum with List and Delete
- New structure: `putio-sync files list` and `putio-sync files delete`
- Old commands hidden but still functional for backward compatibility
- Updated main.rs routing

**New Commands**:
```bash
putio-sync files list [OPTIONS]   # List files on Put.io
putio-sync files delete [OPTIONS]  # Delete files from Put.io
```

---

## Next Steps

### 📋 1.4: Restructure Transfers Command (NEXT)

**Goal**: Move create under transfers as add subcommand

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
putio-sync transfers add [SOURCES]...  # New location for create
putio-sync transfers cancel
putio-sync transfers retry
```

**Tasks**:
- [ ] Add `Add` variant to TransfersCommands
- [ ] Copy create command logic
- [ ] Update main.rs routing
- [ ] Hide old create command
- [ ] Test and commit

---

## Phase 2-6: Future Work

- Phase 2: Watch Configuration
- Phase 3: API Crate (putio-sync-api)
- Phase 4: Web Server (putio-sync-web)
- Phase 5: Daemon Integration
- Phase 6: Testing & Documentation

---

## Session Summary

**Completed**:
- Phase 1.1: Remove old login ✅
- Phase 1.2: Rename get-token to login ✅
- Phase 1.3: Create files command structure ✅

**Next**: Phase 1.4 - Move create under transfers

**Commits**:
- fa90119: Login command refactoring
- 7d25277: Workplan tracking
- 5dfe713: Files command structure
