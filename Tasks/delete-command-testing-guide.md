# Delete Command - Human Testing Guide

This guide provides comprehensive test cases for the delete command implementation in putio-sync.

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

3. **Upload Test Files**:
   Before testing delete functionality, upload some test files to Put.io:
   - Create several test files with different extensions (.txt, .mkv, .mp4, etc.)
   - Create a test folder with some files inside
   - Create files with different dates if testing date-based deletion

## Test Cases

### Test 1: Delete Specific File by ID

**Purpose**: Verify deletion of a single file by ID

```bash
# First, list files to get an ID
./target/release/putio-sync list

# Delete a specific file (replace 12345 with actual ID)
./target/release/putio-sync delete 12345
```

**Expected Behavior**:
- ✅ Display file information (ID, name, size, type)
- ✅ Show confirmation prompt
- ✅ After confirmation (y), delete the file
- ✅ Display success message with space freed
- ✅ File should no longer appear in list

**Verification**:
```bash
./target/release/putio-sync list
```

---

### Test 2: Delete Multiple Files by ID

**Purpose**: Verify deletion of multiple files at once

```bash
# Delete multiple files (replace with actual IDs)
./target/release/putio-sync delete 12345 67890 11111
```

**Expected Behavior**:
- ✅ Display all files to be deleted
- ✅ Show total size that will be freed
- ✅ Show confirmation prompt
- ✅ Delete all files after confirmation
- ✅ Display summary of deletions

---

### Test 3: Delete with Confirmation Prompt

**Purpose**: Verify confirmation prompt works correctly

```bash
# Delete a file and answer 'n' to cancel
./target/release/putio-sync delete 12345
# Type 'n' when prompted

# Delete a file and answer 'y' to confirm
./target/release/putio-sync delete 12345
# Type 'y' when prompted
```

**Expected Behavior**:
- ✅ When answering 'n': Display "Deletion cancelled", file not deleted
- ✅ When answering 'y': File is deleted successfully

---

### Test 4: Delete with --yes Flag (Skip Confirmation)

**Purpose**: Verify --yes flag skips confirmation prompt

```bash
./target/release/putio-sync delete 12345 --yes
```

**Expected Behavior**:
- ✅ No confirmation prompt shown
- ✅ File deleted immediately
- ✅ Success message displayed

---

### Test 5: Delete with Glob Filter

**Purpose**: Verify filtering files by glob pattern

```bash
# Delete all .txt files
./target/release/putio-sync delete --all --filter "*.txt"

# Delete all .mkv files
./target/release/putio-sync delete --all --filter "*.mkv"

# Delete files starting with "test"
./target/release/putio-sync delete --all --filter "test*"
```

**Expected Behavior**:
- ✅ Only files matching the pattern are shown
- ✅ Other files are not affected
- ✅ Confirmation prompt shows only matching files
- ✅ Only matching files are deleted

**Verification**:
```bash
./target/release/putio-sync list
```

---

### Test 6: Dry Run Mode

**Purpose**: Verify --dry-run shows what would be deleted without deleting

```bash
# Dry run for specific file
./target/release/putio-sync delete 12345 --dry-run

# Dry run for all files
./target/release/putio-sync delete --all --dry-run

# Dry run with filter
./target/release/putio-sync delete --all --filter "*.txt" --dry-run
```

**Expected Behavior**:
- ✅ Display files that would be deleted
- ✅ Show "Dry run mode - no files were deleted" message
- ✅ No confirmation prompt
- ✅ No actual deletion occurs
- ✅ Files still present after dry run

**Verification**:
```bash
./target/release/putio-sync list
```

---

### Test 7: Delete All Files

**Purpose**: Verify --all flag deletes all files (with confirmation)

```bash
./target/release/putio-sync delete --all
```

**Expected Behavior**:
- ✅ Display all files in the account
- ✅ Show total count and size
- ✅ Show confirmation prompt
- ✅ Delete all files after confirmation
- ✅ Display summary

**⚠️ WARNING**: This will delete ALL files in your account! Use with caution or test with a dedicated test account.

---

### Test 8: Delete with Date Filter (--before)

**Purpose**: Verify deletion of files created before a specific date

```bash
# Delete files created before January 2025
./target/release/putio-sync delete --all --before "2025-01"

# Delete files created before a specific date
./target/release/putio-sync delete --all --before "2025-01-15"

# Different date formats
./target/release/putio-sync delete --all --before "2025/01/15"
./target/release/putio-sync delete --all --before "01/15/2025"
```

**Expected Behavior**:
- ✅ Only files created before the specified date are shown
- ✅ Files created on or after the date are not affected
- ✅ Confirmation shows only matching files
- ✅ Only matching files are deleted

**Verification**:
```bash
./target/release/putio-sync list --sort date
```

---

### Test 9: Recursive Folder Deletion

**Purpose**: Verify recursive deletion of folders and their contents

```bash
# Delete a folder recursively
./target/release/putio-sync delete 12345 --recursive
```

**Expected Behavior**:
- ✅ Display folder and estimate total size
- ✅ Show confirmation prompt
- ✅ Delete folder and all contents
- ✅ Display success message

**Verification**:
```bash
./target/release/putio-sync list
```

---

### Test 10: Combined Filters

**Purpose**: Verify combining multiple filters

```bash
# Delete old .txt files
./target/release/putio-sync delete --all --filter "*.txt" --before "2025-01"

# Dry run with combined filters
./target/release/putio-sync delete --all --filter "*.mkv" --before "2024-12" --dry-run
```

**Expected Behavior**:
- ✅ Both filters are applied correctly
- ✅ Only files matching all criteria are shown
- ✅ Confirmation shows only matching files

---

### Test 11: Error Handling - No Files Specified

**Purpose**: Verify error when no files specified and --all not used

```bash
./target/release/putio-sync delete
```

**Expected Behavior**:
- ✅ Error message: "Either specify file IDs or use --all flag"
- ✅ Exit with error code

---

### Test 12: Error Handling - Invalid File ID

**Purpose**: Verify error handling for non-existent file

```bash
./target/release/putio-sync delete 999999999
```

**Expected Behavior**:
- ✅ Error message indicating file not found
- ✅ No files deleted
- ✅ Exit with error code

---

### Test 13: Error Handling - Invalid Date Format

**Purpose**: Verify error handling for invalid date

```bash
./target/release/putio-sync delete --all --before "invalid-date"
```

**Expected Behavior**:
- ✅ Error message: "Invalid date format"
- ✅ Show expected date formats
- ✅ Exit with error code

---

### Test 14: Delete with No Matching Files

**Purpose**: Verify behavior when filter matches no files

```bash
./target/release/putio-sync delete --all --filter "*.nonexistent"
```

**Expected Behavior**:
- ✅ Message: "No files to delete"
- ✅ Exit successfully (not an error)
- ✅ No confirmation prompt

---

### Test 15: Progress Display

**Purpose**: Verify progress display during deletion

```bash
# Delete multiple files
./target/release/putio-sync delete 12345 67890 11111 --yes
```

**Expected Behavior**:
- ✅ Show progress for each file ("Deleting <filename> ... ✓")
- ✅ Display final summary with:
  - Files deleted count
  - Space freed
- ✅ Show checkmarks (✓) for successful deletions

---

### Test 16: Partial Failure Handling

**Purpose**: Verify handling when some deletions fail

**Setup**:
1. Delete a file
2. Try to delete it again along with valid files

```bash
./target/release/putio-sync delete 12345 67890 --yes
# Then run again with one of the same IDs
./target/release/putio-sync delete 12345 88888 --yes
```

**Expected Behavior**:
- ✅ Show error for failed deletions (✗)
- ✅ Continue deleting other files
- ✅ Display summary showing:
  - Number of successful deletions
  - Number of failed deletions
  - List of errors

---

### Test 17: Empty Filter Result

**Purpose**: Verify behavior when --all is used but account is empty

**Setup**: Delete all files first

```bash
./target/release/putio-sync delete --all --yes
# Then run delete again
./target/release/putio-sync delete --all
```

**Expected Behavior**:
- ✅ Message: "No files to delete"
- ✅ Exit successfully
- ✅ No error or confirmation prompt

---

### Test 18: Delete File vs Folder Without --recursive

**Purpose**: Verify folder deletion requires --recursive flag

```bash
# Try to delete a folder without --recursive
./target/release/putio-sync delete <folder_id>
```

**Expected Behavior**:
- ✅ If folder contains files: should still delete (based on current implementation)
- ✅ Display folder in deletion list
- ✅ Show confirmation prompt

**Note**: Current implementation treats folders as regular files unless --recursive is specified.

---

### Test 19: Size Display Formatting

**Purpose**: Verify human-readable size formatting

**Setup**: Create files of various sizes

```bash
./target/release/putio-sync delete --all --dry-run
```

**Expected Behavior**:
- ✅ Sizes displayed in human-readable format:
  - Bytes (B)
  - Kilobytes (KB)
  - Megabytes (MB)
  - Gigabytes (GB)
  - Terabytes (TB)
- ✅ Consistent formatting across file list and summary

---

### Test 20: Network Error Handling

**Purpose**: Verify graceful handling of network errors

**Setup**: Disconnect network or use invalid token

```bash
# Use invalid token
./target/release/putio-sync delete 12345 --config /tmp/invalid-config.toml
```

**Expected Behavior**:
- ✅ Clear error message
- ✅ No partial deletions
- ✅ Exit with error code

---

## Test Scenarios Summary

| # | Test Case | Status | Notes |
|---|-----------|--------|-------|
| 1 | Delete specific file | ⬜ | |
| 2 | Delete multiple files | ⬜ | |
| 3 | Confirmation prompt | ⬜ | |
| 4 | Skip confirmation (--yes) | ⬜ | |
| 5 | Glob filter | ⬜ | |
| 6 | Dry run | ⬜ | |
| 7 | Delete all | ⬜ | Use test account! |
| 8 | Date filter | ⬜ | |
| 9 | Recursive folder deletion | ⬜ | |
| 10 | Combined filters | ⬜ | |
| 11 | Error: no files specified | ⬜ | |
| 12 | Error: invalid file ID | ⬜ | |
| 13 | Error: invalid date | ⬜ | |
| 14 | No matching files | ⬜ | |
| 15 | Progress display | ⬜ | |
| 16 | Partial failure | ⬜ | |
| 17 | Empty account | ⬜ | |
| 18 | Folder without --recursive | ⬜ | |
| 19 | Size formatting | ⬜ | |
| 20 | Network error | ⬜ | |

## Quick Test Script

Save this as `test-delete.sh` for rapid testing:

```bash
#!/bin/bash

# Quick Delete Command Test Script
# USAGE: ./test-delete.sh

set -e

CLI="./target/release/putio-sync"

echo "=== Delete Command Test Suite ==="
echo ""

# Test 1: Help
echo "Test 1: Display help"
$CLI delete --help
echo ""

# Test 2: Dry run
echo "Test 2: Dry run mode"
$CLI delete --all --dry-run
echo ""

# Test 3: No files specified
echo "Test 3: Error - no files specified"
$CLI delete && echo "FAILED: Should error" || echo "PASSED: Error as expected"
echo ""

# Test 4: List files for reference
echo "Test 4: List available files"
$CLI list
echo ""

echo "=== Manual Test Required ==="
echo "Please test the following manually:"
echo "  - Delete specific file by ID"
echo "  - Delete with confirmation"
echo "  - Delete with filters"
echo "  - Recursive folder deletion"
echo ""

echo "All automated tests completed!"
```

## Cleanup After Testing

```bash
# If you need to clean up test files
./target/release/putio-sync delete --all --dry-run
# Review the list, then:
./target/release/putio-sync delete --all --yes
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
6. **Put.io account state**: Number of files, any special conditions

## Success Criteria

All tests should:
- ✅ Execute without panics or crashes
- ✅ Display clear, informative messages
- ✅ Handle errors gracefully
- ✅ Delete files as specified
- ✅ Respect dry-run and confirmation flags
- ✅ Provide accurate summaries

## Notes

- Always test with a dedicated test account or test files you don't mind losing
- The `--dry-run` flag is your friend - use it liberally
- Confirmation prompts are there for safety - never disable them in production use
- Date filtering uses the creation date from Put.io metadata
- Glob patterns follow standard glob syntax (*, ?, [])
