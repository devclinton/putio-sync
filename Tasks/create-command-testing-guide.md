# Create Command - Human Testing Guide

This guide provides comprehensive test cases for the create command implementation in putio-sync.

## Prerequisites

1. **Build the CLI**:
   ```bash
   cd putio-sync
   cargo build --release
   ```

2. **Authenticate**:
   ```bash
   ./target/release/putio-sync login --token YOUR_TOKEN
   ```

3. **Prepare Test Files**:
   - Have a valid magnet link ready (for legal content)
   - Have a valid .torrent file ready (for legal content)
   - Create a test directory with some .torrent files

## Test Cases

### Test 1: Add Single Magnet Link

**Purpose**: Verify adding a transfer from a magnet link

```bash
./target/release/putio-sync create "magnet:?xt=urn:btih:HASH&dn=Legal+Content"
```

**Expected Behavior**:
- ✅ Command succeeds without error
- ✅ Displays: "✓ Transfer added: [name] (ID: [id])"
- ✅ Shows transfer status (e.g., "WAITING", "DOWNLOADING")
- ✅ Shows transfer size (may be "Unknown" initially)
- ✅ Transfer appears in Put.io web interface

**Example Output**:
```
✓ Transfer added: Legal Content (ID: 12345)
  Status: WAITING
  Size: Unknown
```

---

### Test 2: Upload Single .torrent File

**Purpose**: Verify uploading a .torrent file

```bash
./target/release/putio-sync create /path/to/file.torrent
```

**Expected Behavior**:
- ✅ Command succeeds without error
- ✅ File is uploaded to Put.io
- ✅ Displays transfer details (name, ID, status, size)
- ✅ Transfer appears in Put.io web interface

---

### Test 3: Add Multiple Sources

**Purpose**: Verify adding multiple sources in one command

```bash
./target/release/putio-sync create \
  "magnet:?xt=urn:btih:HASH1" \
  /path/to/file1.torrent \
  /path/to/file2.torrent \
  "magnet:?xt=urn:btih:HASH2"
```

**Expected Behavior**:
- ✅ All sources are processed sequentially
- ✅ Each transfer is added successfully
- ✅ Displays success message for each transfer
- ✅ All transfers appear in Put.io web interface

---

### Test 4: Add Transfer to Specific Folder

**Purpose**: Verify --parent-id option

First, create a folder in Put.io web interface and note its ID, then:

```bash
./target/release/putio-sync create \
  --parent-id 67890 \
  "magnet:?xt=urn:btih:HASH"
```

**Expected Behavior**:
- ✅ Transfer is added successfully
- ✅ Displays: "Parent folder: 67890"
- ✅ Transfer appears in the specified folder in Put.io web interface

---

### Test 5: Process Watch Directory (One-time)

**Purpose**: Verify --watch option without daemon mode

Setup: Create a directory with some .torrent files:
```bash
mkdir ~/test-torrents
cp /path/to/file1.torrent ~/test-torrents/
cp /path/to/file2.torrent ~/test-torrents/
```

Run:
```bash
./target/release/putio-sync create --watch ~/test-torrents
```

**Expected Behavior**:
- ✅ Scans the directory
- ✅ Displays: "Found X .torrent file(s) in [path]"
- ✅ Uploads all .torrent files
- ✅ Displays success message for each file
- ✅ Command exits after processing (does not continue watching)

---

### Test 6: Process Multiple Watch Directories

**Purpose**: Verify multiple --watch options

```bash
./target/release/putio-sync create \
  --watch ~/test-torrents1 \
  --watch ~/test-torrents2
```

**Expected Behavior**:
- ✅ Scans both directories
- ✅ Displays count for each directory
- ✅ Uploads all .torrent files from both directories
- ✅ Command exits after processing

---

### Test 7: Watch Directory with Parent Folder

**Purpose**: Verify combining --watch and --parent-id

```bash
./target/release/putio-sync create \
  --watch ~/test-torrents \
  --parent-id 67890
```

**Expected Behavior**:
- ✅ Scans directory and uploads files
- ✅ All transfers are placed in the specified parent folder
- ✅ Displays parent folder ID for each transfer

---

### Test 8: Combine Sources and Watch

**Purpose**: Verify using both positional sources and --watch

```bash
./target/release/putio-sync create \
  "magnet:?xt=urn:btih:HASH" \
  --watch ~/test-torrents
```

**Expected Behavior**:
- ✅ First adds the magnet link
- ✅ Then processes the watch directory
- ✅ All transfers are added successfully

---

### Test 9: Daemon Mode (Not Yet Implemented)

**Purpose**: Verify daemon mode returns helpful error

```bash
./target/release/putio-sync create \
  --watch ~/test-torrents \
  --daemon
```

**Expected Behavior**:
- ✅ Command fails with error message
- ✅ Error: "Daemon mode not yet implemented. Will be added in Phase 7.4 (Watch Mode Implementation)."
- ✅ Suggests user should use without --daemon flag

---

### Test 10: No Sources or Watch Directories

**Purpose**: Verify validation of required arguments

```bash
./target/release/putio-sync create
```

**Expected Behavior**:
- ✅ Command fails with error
- ✅ Error message: "No sources or watch directories specified"
- ✅ Suggests providing torrent files, magnet links, or watch directories

---

### Test 11: Invalid Magnet Link Format

**Purpose**: Verify magnet link validation

```bash
./target/release/putio-sync create "http://example.com/file.torrent"
```

**Expected Behavior**:
- ✅ Command fails with error
- ✅ Error: "Invalid source: '...'. Must be a magnet link (magnet:?...) or .torrent file"

---

### Test 12: Non-existent .torrent File

**Purpose**: Verify file existence check

```bash
./target/release/putio-sync create /path/to/nonexistent.torrent
```

**Expected Behavior**:
- ✅ Command fails with error
- ✅ Error: "File not found: /path/to/nonexistent.torrent"

---

### Test 13: Invalid File Extension

**Purpose**: Verify .torrent extension validation

```bash
touch /tmp/fake.txt
./target/release/putio-sync create /tmp/fake.txt
```

**Expected Behavior**:
- ✅ Command fails with error
- ✅ Error: "Invalid source: '...'. Must be a magnet link (magnet:?...) or .torrent file"

---

### Test 14: Watch Non-existent Directory

**Purpose**: Verify watch directory existence check

```bash
./target/release/putio-sync create --watch /nonexistent/directory
```

**Expected Behavior**:
- ✅ Displays warning: "Watch directory does not exist: /nonexistent/directory"
- ✅ Continues processing (doesn't fail)
- ✅ Command completes successfully

---

### Test 15: Watch Directory with No .torrent Files

**Purpose**: Verify behavior with empty watch directory

```bash
mkdir ~/empty-dir
./target/release/putio-sync create --watch ~/empty-dir
```

**Expected Behavior**:
- ✅ Displays: "No .torrent files found in [path]"
- ✅ Command completes successfully
- ✅ No transfers are added

---

### Test 16: Watch File Instead of Directory

**Purpose**: Verify watch path validation

```bash
touch /tmp/notadir.txt
./target/release/putio-sync create --watch /tmp/notadir.txt
```

**Expected Behavior**:
- ✅ Displays warning: "Not a directory: /tmp/notadir.txt"
- ✅ Continues processing (doesn't fail)
- ✅ Command completes successfully

---

### Test 17: No Authentication Configured

**Purpose**: Verify authentication requirement

**Setup**: Remove or rename config file
```bash
mv ~/.putiosync/config.toml ~/.putiosync/config.toml.bak
```

Run:
```bash
./target/release/putio-sync create "magnet:?xt=urn:btih:HASH"
```

**Cleanup**:
```bash
mv ~/.putiosync/config.toml.bak ~/.putiosync/config.toml
```

**Expected Behavior**:
- ✅ Command fails with error
- ✅ Error: "No authentication configured. Please run 'putio-sync login' first."

---

### Test 18: Help Display

**Purpose**: Verify help message is informative

```bash
./target/release/putio-sync create --help
```

**Expected Behavior**:
- ✅ Display command description: "Add torrents to Put.io"
- ✅ List all options:
  - [SOURCES]... - Torrent files or magnet links
  - -w, --watch <PATHS> - Watch directories
  - --parent-id <ID> - Parent folder ID
  - -d, --daemon - Run in daemon mode
  - --interval <SECONDS> - Watch check interval
- ✅ Show default values (interval: 60)
- ✅ Clear descriptions for each option

---

### Test 19: Size Formatting

**Purpose**: Verify human-readable size display

```bash
./target/release/putio-sync create "magnet:?xt=urn:btih:HASH"
```

**Expected Behavior**:
- ✅ Size displays as "Unknown" for new transfers
- ✅ After transfer metadata is fetched, size shows in human-readable format
- ✅ Format examples: "512 B", "1.50 KB", "150.25 MB", "1.50 GB", "4.56 TB"

---

### Test 20: Large .torrent File

**Purpose**: Verify handling of large torrent files

**Setup**: Find or create a large .torrent file (e.g., Linux distro)

```bash
./target/release/putio-sync create /path/to/large.torrent
```

**Expected Behavior**:
- ✅ File uploads successfully
- ✅ No memory issues or timeouts
- ✅ Transfer is added successfully

---

### Test 21: Special Characters in Filename

**Purpose**: Verify handling of special characters

**Setup**: Create a .torrent file with special characters in the name

```bash
cp /path/to/test.torrent "/tmp/file with spaces & chars.torrent"
./target/release/putio-sync create "/tmp/file with spaces & chars.torrent"
```

**Expected Behavior**:
- ✅ File uploads successfully
- ✅ Transfer is created
- ✅ Filename is preserved correctly

---

### Test 22: Verify Transfers in Put.io Web Interface

**Purpose**: End-to-end verification

After running any create command:

1. Log in to Put.io web interface
2. Navigate to Transfers section

**Expected Behavior**:
- ✅ All added transfers appear in the list
- ✅ Transfer names match what was displayed in CLI
- ✅ Transfer IDs match what was displayed in CLI
- ✅ Transfers are in the correct parent folder (if specified)
- ✅ Transfers are downloading or queued

---

### Test 23: API Authentication Error

**Purpose**: Verify handling of invalid credentials

**Setup**: Temporarily modify config with invalid token

```bash
./target/release/putio-sync create "magnet:?xt=urn:btih:HASH"
```

**Expected Behavior**:
- ✅ Command fails with API error
- ✅ Error message indicates authentication failure
- ✅ Clear instructions to check credentials

---

### Test 24: Network Error Handling

**Purpose**: Verify behavior without network connection

**Setup**: Disconnect from internet, then:

```bash
./target/release/putio-sync create "magnet:?xt=urn:btih:HASH"
```

**Expected Behavior**:
- ✅ Command fails with network error
- ✅ Error message indicates connection failure
- ✅ No partial state or corruption

---

### Test 25: Interval Parameter (Daemon Mode)

**Purpose**: Verify interval parameter is accepted

```bash
./target/release/putio-sync create \
  --watch ~/test-torrents \
  --daemon \
  --interval 120
```

**Expected Behavior**:
- ✅ Command fails (daemon not implemented)
- ✅ Error mentions the interval value was received (120s)

---

## Test Scenarios Summary

| # | Test Case | Status | Notes |
|---|-----------|--------|-------|
| 1 | Add single magnet link | ⬜ | |
| 2 | Upload single .torrent file | ⬜ | |
| 3 | Add multiple sources | ⬜ | |
| 4 | Add to specific folder | ⬜ | |
| 5 | Process watch directory once | ⬜ | |
| 6 | Process multiple watch directories | ⬜ | |
| 7 | Watch directory with parent folder | ⬜ | |
| 8 | Combine sources and watch | ⬜ | |
| 9 | Daemon mode (not implemented) | ⬜ | |
| 10 | No sources or watch | ⬜ | |
| 11 | Invalid magnet link | ⬜ | |
| 12 | Non-existent file | ⬜ | |
| 13 | Invalid file extension | ⬜ | |
| 14 | Non-existent watch directory | ⬜ | |
| 15 | Empty watch directory | ⬜ | |
| 16 | Watch file instead of directory | ⬜ | |
| 17 | No authentication | ⬜ | |
| 18 | Help display | ⬜ | |
| 19 | Size formatting | ⬜ | |
| 20 | Large .torrent file | ⬜ | |
| 21 | Special characters in filename | ⬜ | |
| 22 | Verify in web interface | ⬜ | |
| 23 | API authentication error | ⬜ | |
| 24 | Network error handling | ⬜ | |
| 25 | Interval parameter | ⬜ | |

## Quick Test Script

Save this as `test-create.sh` for rapid testing:

```bash
#!/bin/bash

# Quick Create Command Test Script
# USAGE: ./test-create.sh

set -e

CLI="./target/release/putio-sync"

echo "=== Create Command Test Suite ==="
echo ""

# Test 1: Help
echo "Test 1: Display help"
$CLI create --help
echo ""

# Test 2: No sources (should fail)
echo "Test 2: No sources or watch directories"
$CLI create && echo "ERROR: Should have failed" || echo "✓ Correctly failed"
echo ""

# Test 3: Invalid source
echo "Test 3: Invalid source"
$CLI create "http://example.com/file.torrent" && echo "ERROR: Should have failed" || echo "✓ Correctly failed"
echo ""

# Test 4: Non-existent file
echo "Test 4: Non-existent file"
$CLI create /nonexistent/file.torrent && echo "ERROR: Should have failed" || echo "✓ Correctly failed"
echo ""

echo "=== Manual Tests Required ==="
echo "Please test the following manually:"
echo "  - Add magnet link (requires valid magnet link)"
echo "  - Upload .torrent file (requires valid .torrent file)"
echo "  - Watch directory (requires directory with .torrent files)"
echo "  - Parent folder ID (requires folder ID from Put.io)"
echo "  - Verify transfers in Put.io web interface"
echo ""

echo "All automated tests completed!"
```

## Validation Checklist

After completing tests, verify:

- [ ] Magnet links can be added successfully
- [ ] .torrent files can be uploaded successfully
- [ ] Multiple sources can be processed in one command
- [ ] --parent-id option works correctly
- [ ] --watch option scans directories and uploads files
- [ ] Multiple watch directories can be specified
- [ ] Daemon mode returns helpful error message
- [ ] Input validation catches all error cases
- [ ] Error messages are clear and helpful
- [ ] Help text is complete and accurate
- [ ] Transfers appear in Put.io web interface
- [ ] Transfer details are displayed correctly

## Cleanup After Testing

```bash
# Remove test transfers from Put.io web interface
# Delete test directories
rm -rf ~/test-torrents ~/test-torrents1 ~/test-torrents2 ~/empty-dir
```

## Reporting Issues

When reporting issues, please include:

1. **Command executed**: Full command line
2. **Expected behavior**: What should happen
3. **Actual behavior**: What actually happened
4. **Error messages**: Full error output
5. **Environment**:
   - OS and version
   - Rust version (`rustc --version`)
   - CLI version (`putio-sync --version`)
6. **Transfer details**: If applicable, transfer ID from Put.io

## Success Criteria

All tests should:
- ✅ Execute without panics or crashes
- ✅ Provide clear success or error messages
- ✅ Add transfers that appear in Put.io web interface
- ✅ Handle edge cases gracefully
- ✅ Validate inputs properly
- ✅ Display helpful error messages

## Notes

- Magnet links must be valid and for legal content
- .torrent files must be valid and for legal content
- Transfers may take time to fetch metadata, so size might show as "Unknown" initially
- Watch mode scans directories but does not recursively scan subdirectories
- Daemon mode (continuous watching) is not yet implemented (Phase 7.4)
- The CLI uses Put.io's transfer API, so transfers are queued and downloaded by Put.io's servers
- Transfer status can be: WAITING, DOWNLOADING, SEEDING, COMPLETED, ERROR, etc.
