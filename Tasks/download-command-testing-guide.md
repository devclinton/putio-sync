# Download Command Testing Guide

This guide provides comprehensive test cases for manually testing the download command functionality.

## Prerequisites

1. Build the project:
```bash
cargo build --release
```

2. Set up authentication:
```bash
./target/release/putio-sync login --interactive
```

3. Ensure you have some test files on your Put.io account

## Test Categories

### 1. Basic Download Tests

#### Test 1.1: Download all files
```bash
./target/release/putio-sync download --all
```
**Expected**: Downloads all files from root folder with default settings

#### Test 1.2: Download specific file by ID
```bash
# Replace 12345 with an actual file ID from your account
./target/release/putio-sync download 12345
```
**Expected**: Downloads only the specified file

#### Test 1.3: Download multiple specific files
```bash
# Replace with actual file IDs from your account
./target/release/putio-sync download 12345 67890 111213
```
**Expected**: Downloads all specified files

### 2. Filter Tests

#### Test 2.1: Download with glob pattern filter
```bash
./target/release/putio-sync download --all --filter "*.mkv"
```
**Expected**: Downloads only .mkv files

#### Test 2.2: Download with multiple extension filter
```bash
./target/release/putio-sync download --all --filter "*.{mp4,mkv,avi}"
```
**Expected**: Downloads only video files with those extensions

#### Test 2.3: Download with name pattern
```bash
./target/release/putio-sync download --all --filter "*movie*"
```
**Expected**: Downloads only files with "movie" in the name

### 3. Directory and Worker Tests

#### Test 3.1: Download to custom directory
```bash
./target/release/putio-sync download --all --dir /tmp/putio-test
```
**Expected**: Creates /tmp/putio-test if it doesn't exist and downloads files there

#### Test 3.2: Download with multiple workers
```bash
./target/release/putio-sync download --all --workers 4
```
**Expected**: Uses 4 concurrent workers for downloading

#### Test 3.3: Download with single worker
```bash
./target/release/putio-sync download --all --workers 1
```
**Expected**: Downloads files sequentially

### 4. Chunking and Buffering Tests

#### Test 4.1: Download with chunking enabled
```bash
./target/release/putio-sync download --all --chunk-size 256MB
```
**Expected**: Large files are split into 256MB chunks

#### Test 4.2: Download with custom buffer size
```bash
./target/release/putio-sync download --all --buffer-size 32MB
```
**Expected**: Uses 32MB buffer for streaming

#### Test 4.3: Download with both chunking and buffering
```bash
./target/release/putio-sync download --all --chunk-size 128MB --buffer-size 16MB
```
**Expected**: Uses both settings appropriately

### 5. Resume and Skip Tests

#### Test 5.1: Test resume functionality
```bash
# Start a download
./target/release/putio-sync download --all

# Press Ctrl+C after partial download

# Resume the download
./target/release/putio-sync download --all
```
**Expected**: Resumes from where it left off

#### Test 5.2: Disable resume
```bash
./target/release/putio-sync download --all --no-resume
```
**Expected**: Restarts downloads from beginning

#### Test 5.3: Skip existing files
```bash
# Download once
./target/release/putio-sync download --all

# Download again with skip-existing
./target/release/putio-sync download --all --skip-existing
```
**Expected**: Skips files that were already downloaded

### 6. Sorting Tests

#### Test 6.1: Sort by date (oldest first)
```bash
./target/release/putio-sync download --all --sort date --reverse
```
**Expected**: Downloads oldest files first

#### Test 6.2: Sort by name
```bash
./target/release/putio-sync download --all --sort name
```
**Expected**: Downloads files in alphabetical order

#### Test 6.3: Sort by size (largest first)
```bash
./target/release/putio-sync download --all --sort size --reverse
```
**Expected**: Downloads largest files first

### 7. Progress Bar Tests

#### Test 7.1: Per-file progress (default)
```bash
./target/release/putio-sync download --all
```
**Expected**: Shows individual file progress bars and overall progress

#### Test 7.2: Overall progress only
```bash
./target/release/putio-sync download --all --progress overall
```
**Expected**: Shows only overall progress bar

#### Test 7.3: Individual file progress only
```bash
./target/release/putio-sync download --all --progress individual
```
**Expected**: Shows only individual file progress bars

#### Test 7.4: No progress bars
```bash
./target/release/putio-sync download --all --progress none
```
**Expected**: No progress bars, only log messages

#### Test 7.5: Rotating progress
```bash
./target/release/putio-sync download --all --progress rotating
```
**Expected**: Rotates between file and overall progress

### 8. Delete After Download Tests

#### Test 8.1: Delete after download
```bash
./target/release/putio-sync download 12345 --delete-after
```
**Expected**: File is deleted from Put.io after successful download

⚠️ **Warning**: This will actually delete files from your account!

### 9. Daemon Mode Tests

#### Test 9.1: Basic daemon mode
```bash
./target/release/putio-sync download --all --daemon --interval 60
```
**Expected**: Checks for new files every 60 seconds and downloads them

#### Test 9.2: Daemon mode with filter
```bash
./target/release/putio-sync download --daemon --all --filter "*.mkv" --interval 120
```
**Expected**: Monitors for new .mkv files every 120 seconds

#### Test 9.3: Daemon mode graceful shutdown
```bash
# Start daemon
./target/release/putio-sync download --daemon --all --interval 300

# Press Ctrl+C
```
**Expected**: Daemon shuts down gracefully, completing current download if any

#### Test 9.4: Daemon mode with verbose logging
```bash
./target/release/putio-sync -vv download --daemon --all --interval 60
```
**Expected**: Shows detailed debug logs for each daemon cycle

### 10. Interrupt and Error Handling Tests

#### Test 10.1: Graceful shutdown with Ctrl+C
```bash
# Start download
./target/release/putio-sync download --all

# Press Ctrl+C during download
```
**Expected**: Cleanly stops download, saves progress, shows summary

#### Test 10.2: Invalid file ID
```bash
./target/release/putio-sync download 99999999999
```
**Expected**: Shows error message and exits gracefully

#### Test 10.3: Network interruption
```bash
# Start download
./target/release/putio-sync download --all

# Disable network briefly during download
```
**Expected**: Handles network errors gracefully with appropriate error messages

### 11. Combined Options Tests

#### Test 11.1: Complex download scenario
```bash
./target/release/putio-sync download --all \
  --filter "*.mkv" \
  --sort size --reverse \
  --workers 3 \
  --chunk-size 256MB \
  --skip-existing \
  --progress per-file \
  --dir ~/Downloads/putio-videos
```
**Expected**: Downloads all .mkv files, sorted by size (largest first), using 3 workers with chunking

#### Test 11.2: Daemon with all options
```bash
./target/release/putio-sync download --daemon \
  --all \
  --filter "*.{mp4,mkv}" \
  --interval 300 \
  --workers 2 \
  --skip-existing \
  --delete-after \
  --progress overall
```
**Expected**: Continuous monitoring with automatic download and deletion

### 12. Non-Interactive Terminal Tests

#### Test 12.1: Redirect to file
```bash
./target/release/putio-sync download --all > download.log 2>&1
```
**Expected**: Progress bars are disabled, only log output

#### Test 12.2: Pipe to another command
```bash
./target/release/putio-sync download --all | tee download.log
```
**Expected**: Progress bars are disabled

### 13. Logging Level Tests

#### Test 13.1: Quiet mode
```bash
./target/release/putio-sync -q download --all
```
**Expected**: Only error messages shown

#### Test 13.2: Verbose mode
```bash
./target/release/putio-sync -v download --all
```
**Expected**: INFO level logging

#### Test 13.3: Debug mode
```bash
./target/release/putio-sync -vv download --all
```
**Expected**: DEBUG level logging

#### Test 13.4: Trace mode
```bash
./target/release/putio-sync -vvv download --all
```
**Expected**: TRACE level logging with detailed information

## Quick Test Script

Save this as `test-download.sh`:

```bash
#!/bin/bash

echo "Building project..."
cargo build --release

echo ""
echo "Test 1: Download with filter"
./target/release/putio-sync download --all --filter "*.txt" --progress none

echo ""
echo "Test 2: Download with sorting"
./target/release/putio-sync download --all --sort date --progress overall

echo ""
echo "Test 3: Download with skip-existing"
./target/release/putio-sync download --all --skip-existing --progress none

echo ""
echo "All tests completed!"
```

Make it executable:
```bash
chmod +x test-download.sh
./test-download.sh
```

## Success Criteria

For each test, verify:
- ✅ Command completes without errors
- ✅ Files are downloaded to correct location
- ✅ Progress bars display correctly (when enabled)
- ✅ Summary statistics are accurate
- ✅ Database is updated correctly
- ✅ Logs show appropriate information
- ✅ Ctrl+C handling works gracefully

## Troubleshooting

### Issue: Downloads are slow
- Try increasing workers: `--workers 4`
- Try enabling chunking for large files: `--chunk-size 256MB`

### Issue: Out of disk space
- Use custom directory: `--dir /path/with/space`
- Check summary to see total size before download

### Issue: Progress bars not showing
- Check if terminal is interactive
- Try different progress style: `--progress overall`
- Disable progress: `--progress none`

### Issue: Downloads restart from beginning
- Ensure resume is enabled (default)
- Check database file exists: `.putiosync.db`
- Verify partial download files exist

## Notes

- All downloads are tracked in `.putiosync.db` in the download directory
- Partial downloads have `.downloading` extension
- Database prevents duplicate downloads when using `--skip-existing`
- Daemon mode uses the same database to track downloads across cycles
