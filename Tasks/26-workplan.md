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

### Phase 2.1: Add Watch Config to Config File

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
- [ ] Update WatchConfig struct in putio-sync-core/src/config.rs
- [ ] Add enabled field (bool)
- [ ] Add paths field (Vec<PathBuf>)
- [ ] Add validation for watch paths
- [ ] Update config example file
- [ ] Update config tests
- [ ] Test and commit

**Commit Target**: "feat(core): add watch directory configuration"

---

## Future Phases

### Phase 3: API Crate (putio-sync-api)
- Create new crate for REST API
- Implement endpoints for files, transfers, queue
- Add security middleware

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

---

## Session Summary

**Phase 1: CLI Command Restructuring - COMPLETE ✅**

All CLI commands have been successfully restructured with:
- Cleaner command hierarchy
- Better organization (files, transfers as parent commands)
- Full backward compatibility
- Hidden deprecated commands

**Next**: Phase 2 - Watch Configuration
