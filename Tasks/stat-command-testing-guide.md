# Stat Command - Human Testing Guide

This guide provides comprehensive test cases for the stat command implementation in putio-sync.

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

3. **Download Some Files**:
   The stat command displays statistics from the download database, so you need to download some files first:
   ```bash
   # Download a few files to populate the database
   ./target/release/putio-sync download --all
   ```

## Test Cases

### Test 1: Basic Statistics Display

**Purpose**: Verify basic statistics are displayed correctly

```bash
./target/release/putio-sync stat
```

**Expected Behavior**:
- ✅ Display "Download Statistics" header with separator
- ✅ Show Total Downloads count with thousands separator
- ✅ Show Total Size in human-readable format (TB, GB, MB, KB, B)
- ✅ Show Total Time in human-readable format (hours, minutes, seconds)
- ✅ Show Average Speed in human-readable format (GB/s, MB/s, KB/s, B/s)
- ✅ Proper alignment and formatting

**Example Output**:
```
Download Statistics
═════════════════════════════════════════════════════════════════
Total Downloads:          1,234
Total Size:            4.56 TB
Total Time:            12h 34m 56s
Average Speed:         102.3 MB/s
═════════════════════════════════════════════════════════════════
```

---

### Test 2: Statistics by Extension

**Purpose**: Verify extension breakdown display

```bash
./target/release/putio-sync stat --by-extension
```

**Expected Behavior**:
- ✅ Display basic statistics
- ✅ Show "By Extension:" section
- ✅ Table with columns: Extension, Files, Total Size, Percentage
- ✅ Extensions sorted by total size (descending)
- ✅ Percentages calculated correctly (sum to ~100%)
- ✅ Human-readable file counts and sizes
- ✅ Proper alignment

**Example Output**:
```
Download Statistics
═════════════════════════════════════════════════════════════════
Total Downloads:          1,234
Total Size:            4.56 TB
Total Time:            12h 34m 56s
Average Speed:         102.3 MB/s

By Extension:
─────────────────────────────────────────────────────────────────
Extension        Files     Total Size    Percentage
─────────────────────────────────────────────────────────────────
.mkv              450        3.20 TB         70.2%
.mp4              300      890.00 GB         19.5%
.pdf              484      470.00 GB         10.3%
─────────────────────────────────────────────────────────────────
═════════════════════════════════════════════════════════════════
```

---

### Test 3: Date Range Filtering (--since)

**Purpose**: Verify filtering statistics since a date

```bash
# Show stats since January 1, 2025
./target/release/putio-sync stat --since "2025-01-01"

# Show stats since January 2025
./target/release/putio-sync stat --since "2025-01"
```

**Expected Behavior**:
- ✅ Display date range in header
- ✅ Only include downloads since the specified date
- ✅ Stats reflect filtered data
- ✅ Extension breakdown (if requested) also filtered

---

### Test 4: Date Range Filtering (--until)

**Purpose**: Verify filtering statistics until a date

```bash
# Show stats until December 31, 2024
./target/release/putio-sync stat --until "2024-12-31"
```

**Expected Behavior**:
- ✅ Display date range in header
- ✅ Only include downloads until the specified date
- ✅ Stats reflect filtered data

---

### Test 5: Combined Date Range

**Purpose**: Verify filtering with both --since and --until

```bash
./target/release/putio-sync stat --since "2024-01-01" --until "2024-12-31"
```

**Expected Behavior**:
- ✅ Display both dates in header: "Since 2024-01-01 Until 2024-12-31"
- ✅ Only include downloads in the specified range
- ✅ Stats reflect filtered data

---

### Test 6: JSON Output Format

**Purpose**: Verify JSON output format

```bash
./target/release/putio-sync stat --output json
```

**Expected Behavior**:
- ✅ Output valid JSON
- ✅ Include fields: total_downloads, total_bytes, total_seconds, average_speed
- ✅ Pretty-printed format
- ✅ No table formatting

**Example Output**:
```json
{
  "total_downloads": 1234,
  "total_bytes": 5000000000000,
  "total_seconds": 45296,
  "average_speed": 110380000.5
}
```

---

### Test 7: JSON Output with Extension Breakdown

**Purpose**: Verify JSON output includes extension data

```bash
./target/release/putio-sync stat --by-extension --output json
```

**Expected Behavior**:
- ✅ Output valid JSON
- ✅ Include "by_extension" array
- ✅ Each extension has: extension, file_count, total_bytes, percentage
- ✅ Percentages as floating point numbers

**Example Output**:
```json
{
  "total_downloads": 1234,
  "total_bytes": 5000000000000,
  "total_seconds": 45296,
  "average_speed": 110380000.5,
  "by_extension": [
    {
      "extension": "mkv",
      "file_count": 450,
      "total_bytes": 3510000000000,
      "percentage": 70.2
    },
    {
      "extension": "mp4",
      "file_count": 300,
      "total_bytes": 975000000000,
      "percentage": 19.5
    }
  ]
}
```

---

### Test 8: Empty Database

**Purpose**: Verify behavior when no downloads exist

**Setup**: Use a fresh database or test with a new target directory

```bash
# Create a new config with different target directory
./target/release/putio-sync stat
```

**Expected Behavior**:
- ✅ Display statistics with all zeros
- ✅ Total Downloads: 0
- ✅ Total Size: 0 B
- ✅ Total Time: 0s
- ✅ Average Speed: N/A
- ✅ No extension breakdown section (or empty if --by-extension used)

---

### Test 9: Help Display

**Purpose**: Verify help message is informative

```bash
./target/release/putio-sync stat --help
```

**Expected Behavior**:
- ✅ Display command description
- ✅ List all options:
  - --by-extension
  - --since <DATE>
  - --until <DATE>
  - -o, --output <FORMAT>
- ✅ Show default values (output: table)
- ✅ Clear descriptions for each option

---

### Test 10: Different Date Formats

**Purpose**: Verify various date format parsing

```bash
# YYYY-MM-DD format
./target/release/putio-sync stat --since "2025-01-15"

# YYYY/MM/DD format
./target/release/putio-sync stat --since "2025/01/15"

# MM/DD/YYYY format
./target/release/putio-sync stat --since "01/15/2025"
```

**Expected Behavior**:
- ✅ All formats parse correctly
- ✅ Display consistent date in header
- ✅ Same results for equivalent dates

---

### Test 11: Invalid Date Format

**Purpose**: Verify error handling for invalid dates

```bash
./target/release/putio-sync stat --since "invalid-date"
```

**Expected Behavior**:
- ✅ Error message: "Invalid date format"
- ✅ Show expected formats
- ✅ Exit with error code

---

### Test 12: Size Formatting Accuracy

**Purpose**: Verify human-readable size formatting

**Setup**: Download files of various sizes

```bash
./target/release/putio-sync stat
```

**Expected Behavior**:
- ✅ Bytes (< 1 KB): "512 B"
- ✅ Kilobytes (< 1 MB): "1.50 KB"
- ✅ Megabytes (< 1 GB): "150.25 MB"
- ✅ Gigabytes (< 1 TB): "1.50 GB"
- ✅ Terabytes: "4.56 TB"
- ✅ Two decimal places for KB/MB/GB/TB

---

### Test 13: Duration Formatting

**Purpose**: Verify time duration formatting

**Expected Behavior**:
- ✅ Seconds only (< 1 min): "45s"
- ✅ Minutes and seconds: "1m 30s"
- ✅ Hours, minutes, seconds: "2h 15m 30s"
- ✅ Hours only: "3h"
- ✅ No unnecessary units

---

### Test 14: Speed Formatting

**Purpose**: Verify transfer speed formatting

**Expected Behavior**:
- ✅ Bytes/sec (< 1 KB/s): "512 B/s"
- ✅ Kilobytes/sec (< 1 MB/s): "150.50 KB/s"
- ✅ Megabytes/sec (< 1 GB/s): "50.25 MB/s"
- ✅ Gigabytes/sec: "1.25 GB/s"
- ✅ Zero speed: "N/A"
- ✅ Two decimal places

---

### Test 15: Count Formatting with Thousands Separator

**Purpose**: Verify number formatting

**Expected Behavior**:
- ✅ < 1,000: "999"
- ✅ Thousands: "1,234"
- ✅ Millions: "1,234,567"
- ✅ Proper comma placement

---

### Test 16: Percentage Calculations

**Purpose**: Verify percentage accuracy in extension breakdown

```bash
./target/release/putio-sync stat --by-extension
```

**Expected Behavior**:
- ✅ Sum of all percentages ≈ 100%
- ✅ One decimal place (e.g., "70.2%")
- ✅ Largest extension has highest percentage
- ✅ Percentages match relative sizes

---

### Test 17: Extension Sorting

**Purpose**: Verify extensions are sorted by total size

```bash
./target/release/putio-sync stat --by-extension
```

**Expected Behavior**:
- ✅ First extension has largest total_bytes
- ✅ Last extension has smallest total_bytes
- ✅ Descending order maintained

---

### Test 18: Database Path from Config

**Purpose**: Verify stat command uses correct database path

```bash
# Should read database from target directory specified in config
./target/release/putio-sync stat
```

**Expected Behavior**:
- ✅ Uses database at: <target_directory>/.putiosync.db
- ✅ Shows stats from that database
- ✅ Error if database doesn't exist

---

### Test 19: Missing Database File

**Purpose**: Verify error handling when database doesn't exist

**Setup**: Delete or move the database file

```bash
./target/release/putio-sync stat
```

**Expected Behavior**:
- ✅ Clear error message: "Failed to open database"
- ✅ Exit with error code
- ✅ Helpful message about running download first

---

### Test 20: Multiple Output Formats with Same Data

**Purpose**: Verify consistency between table and JSON outputs

```bash
# Get table output
./target/release/putio-sync stat --by-extension

# Get JSON output
./target/release/putio-sync stat --by-extension --output json
```

**Expected Behavior**:
- ✅ Same total_downloads in both
- ✅ Same total_bytes in both
- ✅ Same total_seconds in both
- ✅ Same average_speed in both
- ✅ Same extension data in both
- ✅ Only formatting differs

---

## Test Scenarios Summary

| # | Test Case | Status | Notes |
|---|-----------|--------|-------|
| 1 | Basic statistics | ⬜ | |
| 2 | Extension breakdown | ⬜ | |
| 3 | Date filter (--since) | ⬜ | |
| 4 | Date filter (--until) | ⬜ | |
| 5 | Combined date range | ⬜ | |
| 6 | JSON output | ⬜ | |
| 7 | JSON with extensions | ⬜ | |
| 8 | Empty database | ⬜ | |
| 9 | Help display | ⬜ | |
| 10 | Date format variations | ⬜ | |
| 11 | Invalid date | ⬜ | |
| 12 | Size formatting | ⬜ | |
| 13 | Duration formatting | ⬜ | |
| 14 | Speed formatting | ⬜ | |
| 15 | Count formatting | ⬜ | |
| 16 | Percentage calculations | ⬜ | |
| 17 | Extension sorting | ⬜ | |
| 18 | Database path | ⬜ | |
| 19 | Missing database | ⬜ | |
| 20 | Format consistency | ⬜ | |

## Quick Test Script

Save this as `test-stat.sh` for rapid testing:

```bash
#!/bin/bash

# Quick Stat Command Test Script
# USAGE: ./test-stat.sh

set -e

CLI="./target/release/putio-sync"

echo "=== Stat Command Test Suite ==="
echo ""

# Test 1: Help
echo "Test 1: Display help"
$CLI stat --help
echo ""

# Test 2: Basic stats
echo "Test 2: Basic statistics"
$CLI stat
echo ""

# Test 3: By extension
echo "Test 3: Extension breakdown"
$CLI stat --by-extension
echo ""

# Test 4: JSON format
echo "Test 4: JSON output"
$CLI stat --output json
echo ""

# Test 5: JSON with extensions
echo "Test 5: JSON with extensions"
$CLI stat --by-extension --output json
echo ""

echo "=== Manual Tests Required ==="
echo "Please test the following manually:"
echo "  - Date range filtering"
echo "  - Invalid date handling"
echo "  - Empty database behavior"
echo "  - Formatting accuracy for various sizes"
echo ""

echo "All automated tests completed!"
```

## Validation Checklist

After completing tests, verify:

- [ ] All statistics display correctly
- [ ] Extension breakdown shows accurate data
- [ ] Date filtering works as expected
- [ ] JSON output is valid and parseable
- [ ] Formatting is consistent and readable
- [ ] Error handling is graceful
- [ ] Performance is acceptable for large databases
- [ ] Help text is clear and accurate

## Cleanup After Testing

```bash
# Stats are read-only, no cleanup needed
# Database persists for future stat queries
```

## Reporting Issues

When reporting issues, please include:

1. **Command executed**: Full command line
2. **Expected behavior**: What should happen
3. **Actual behavior**: What actually happened
4. **Database state**: Number of downloads, extensions present
5. **Error messages**: Full error output
6. **Environment**:
   - OS and version
   - Rust version (`rustc --version`)
   - CLI version (`putio-sync --version`)

## Success Criteria

All tests should:
- ✅ Execute without panics or crashes
- ✅ Display clear, formatted output
- ✅ Show accurate statistics
- ✅ Handle edge cases gracefully
- ✅ Provide valid JSON when requested
- ✅ Match data between table and JSON formats

## Notes

- Stats are calculated from download history in the database
- Database is created during first download
- Stats accumulate over time
- Extension breakdown shows all downloaded file types
- Date filtering uses download completion timestamp
- Average speed is calculated as total bytes / total seconds
- Zero values display as "N/A" for speeds or "0" for counts/sizes
