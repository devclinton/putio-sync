# History Command - Human Testing Guide

This guide provides comprehensive test cases for the history command implementation in putio-sync.

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

3. **Create History Data**:
   The history command displays download and delete history from the database, so you need some activity first:
   ```bash
   # Download some files to populate the database
   ./target/release/putio-sync download --all --filter "*.mkv"

   # Delete some files to add delete history
   ./target/release/putio-sync delete --filter "*.txt" --yes
   ```

## Test Cases

### Test 1: Basic History Display

**Purpose**: Verify basic history display with default settings

```bash
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Display "Download & Delete History" header with separator
- ✅ Show table with columns: Timestamp, Action, Filename, Size, Speed
- ✅ Default limit of 50 entries
- ✅ Most recent entries first (newest at top)
- ✅ Pagination info at bottom showing "Showing 1 - X of Y entries"
- ✅ Human-readable sizes, speeds
- ✅ Filenames truncated to 40 characters if too long
- ✅ Proper alignment and formatting

**Example Output**:
```
Download & Delete History
═══════════════════════════════════════════════════════════════════════════════════════════════
Filters:

Timestamp            Action       Filename                                 Size         Speed
───────────────────────────────────────────────────────────────────────────────────────────────
2025-11-15 14:30:45  downloaded   Big.Buck.Bunny.1080p.mkv                1.50 GB      25.50 MB/s
2025-11-15 14:25:12  deleted      old-readme.txt                          5.00 KB      -
2025-11-15 14:20:08  downloaded   sample-video.mp4                        890.00 MB    18.20 MB/s
───────────────────────────────────────────────────────────────────────────────────────────────

Showing 1 - 3 of 3 entries

═══════════════════════════════════════════════════════════════════════════════════════════════
```

---

### Test 2: Filter by Action (Downloaded)

**Purpose**: Verify filtering to show only downloads

```bash
./target/release/putio-sync history --action downloaded
```

**Expected Behavior**:
- ✅ Display "Filters: Action: downloaded" line
- ✅ Only show entries with action "downloaded"
- ✅ No "deleted" entries visible
- ✅ Speed column shows values (not dashes)
- ✅ Entry count reflects filtered results

---

### Test 3: Filter by Action (Deleted)

**Purpose**: Verify filtering to show only deletions

```bash
./target/release/putio-sync history --action deleted
```

**Expected Behavior**:
- ✅ Display "Filters: Action: deleted" line
- ✅ Only show entries with action "deleted"
- ✅ No "downloaded" entries visible
- ✅ Speed column shows "-" (deletions have no speed)
- ✅ Entry count reflects filtered results

---

### Test 4: Invalid Action

**Purpose**: Verify error handling for invalid action

```bash
./target/release/putio-sync history --action invalid
```

**Expected Behavior**:
- ✅ Error message: "Invalid action 'invalid'"
- ✅ Message shows expected values: "Expected 'downloaded' or 'deleted'"
- ✅ Exit with error code

---

### Test 5: Date Range Filtering (--since)

**Purpose**: Verify filtering history since a date

```bash
# Show history since January 1, 2025
./target/release/putio-sync history --since "2025-01-01"

# Show history since a recent date
./target/release/putio-sync history --since "2025-11-14"
```

**Expected Behavior**:
- ✅ Display "Filters: Since: 2025-01-01" line
- ✅ Only include entries from the specified date forward
- ✅ Entry count reflects filtered results
- ✅ Timestamps all >= specified date

---

### Test 6: Date Range Filtering (--until)

**Purpose**: Verify filtering history until a date

```bash
# Show history until November 14, 2025
./target/release/putio-sync history --until "2025-11-14"
```

**Expected Behavior**:
- ✅ Display "Filters: Until: 2025-11-14" line
- ✅ Only include entries up to the specified date
- ✅ Entry count reflects filtered results
- ✅ Timestamps all <= specified date

---

### Test 7: Combined Date Range

**Purpose**: Verify filtering with both --since and --until

```bash
./target/release/putio-sync history --since "2025-01-01" --until "2025-11-14"
```

**Expected Behavior**:
- ✅ Display "Filters: Since: 2025-01-01, Until: 2025-11-14" line
- ✅ Only include entries in the specified range
- ✅ Entry count reflects filtered results
- ✅ Timestamps all within the range

---

### Test 8: Combined Action and Date Filters

**Purpose**: Verify multiple filters work together

```bash
./target/release/putio-sync history --action downloaded --since "2025-11-01"
```

**Expected Behavior**:
- ✅ Display "Filters: Action: downloaded, Since: 2025-11-01" line
- ✅ Only show downloads since November 1, 2025
- ✅ Entry count reflects filtered results
- ✅ Both filters applied correctly

---

### Test 9: Pagination with --limit

**Purpose**: Verify limiting number of results

```bash
# Show only 10 entries
./target/release/putio-sync history --limit 10

# Show only 5 entries
./target/release/putio-sync history -n 5
```

**Expected Behavior**:
- ✅ Show exactly N entries (or fewer if total < N)
- ✅ Pagination info shows "Showing 1 - N of X entries"
- ✅ Message suggests using --offset to see more if there are more entries

---

### Test 10: Pagination with --offset

**Purpose**: Verify skipping entries with offset

```bash
# Skip first 10 entries
./target/release/putio-sync history --offset 10

# Skip first 50 entries, show next 10
./target/release/putio-sync history --offset 50 --limit 10
```

**Expected Behavior**:
- ✅ Skip the specified number of entries
- ✅ Pagination info shows "Showing (offset+1) - (offset+limit) of X entries"
- ✅ Entries start from the correct position
- ✅ No suggestion to use --offset if at end of results

---

### Test 11: JSON Output Format

**Purpose**: Verify JSON output format

```bash
./target/release/putio-sync history --output json
```

**Expected Behavior**:
- ✅ Output valid JSON array
- ✅ Each entry has fields: timestamp, action, filename, size, speed, duration
- ✅ Speed and duration are null for deleted entries
- ✅ Pretty-printed format
- ✅ No table formatting

**Example Output**:
```json
[
  {
    "timestamp": "2025-11-15 14:30:45",
    "action": "downloaded",
    "filename": "Big.Buck.Bunny.1080p.mkv",
    "size": "1.50 GB",
    "speed": "25.50 MB/s",
    "duration": "1m 2s"
  },
  {
    "timestamp": "2025-11-15 14:25:12",
    "action": "deleted",
    "filename": "old-readme.txt",
    "size": "5.00 KB"
  }
]
```

---

### Test 12: CSV Output Format

**Purpose**: Verify CSV output format

```bash
./target/release/putio-sync history --output csv
```

**Expected Behavior**:
- ✅ CSV header: "Timestamp,Action,Filename,Size,Speed,Duration"
- ✅ Each entry as CSV row
- ✅ Proper CSV escaping for filenames with commas
- ✅ Proper CSV escaping for filenames with quotes
- ✅ Empty fields for speed/duration on deleted entries
- ✅ No table formatting

**Example Output**:
```csv
Timestamp,Action,Filename,Size,Speed,Duration
2025-11-15 14:30:45,downloaded,Big.Buck.Bunny.1080p.mkv,1.50 GB,25.50 MB/s,1m 2s
2025-11-15 14:25:12,deleted,old-readme.txt,5.00 KB,,
"2025-11-15 14:20:08",downloaded,"file,with,commas.mp4",890.00 MB,18.20 MB/s,50s
```

---

### Test 13: JSON Output with Filters

**Purpose**: Verify JSON output respects filters

```bash
./target/release/putio-sync history --action downloaded --output json --limit 5
```

**Expected Behavior**:
- ✅ Output valid JSON
- ✅ Only include filtered entries
- ✅ Maximum 5 entries in array
- ✅ All entries have action "downloaded"

---

### Test 14: CSV Output with Filters

**Purpose**: Verify CSV output respects filters

```bash
./target/release/putio-sync history --since "2025-11-01" --output csv
```

**Expected Behavior**:
- ✅ CSV header present
- ✅ Only include entries since November 1, 2025
- ✅ All timestamps >= 2025-11-01

---

### Test 15: Empty History

**Purpose**: Verify behavior when no history exists

**Setup**: Use a fresh database or test with a new target directory

```bash
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Display header and table structure
- ✅ Message: "No history entries found."
- ✅ Suggestion: "Try downloading some files first!"
- ✅ Pagination shows "Showing 0 - 0 of 0 entries"

---

### Test 16: Empty Results from Filter

**Purpose**: Verify behavior when filters match nothing

```bash
./target/release/putio-sync history --since "2099-01-01"
```

**Expected Behavior**:
- ✅ Display header with filter info
- ✅ Message: "No history entries found."
- ✅ No suggestion about downloading (filters are shown)
- ✅ Pagination shows "Showing 0 - 0 of 0 entries"

---

### Test 17: Different Date Formats

**Purpose**: Verify various date format parsing

```bash
# YYYY-MM-DD format
./target/release/putio-sync history --since "2025-01-15"

# YYYY/MM/DD format
./target/release/putio-sync history --since "2025/01/15"

# MM/DD/YYYY format
./target/release/putio-sync history --since "01/15/2025"

# DD-MMM-YYYY format
./target/release/putio-sync history --since "15-Jan-2025"
```

**Expected Behavior**:
- ✅ All formats parse correctly
- ✅ Same results for equivalent dates
- ✅ No parsing errors

---

### Test 18: Invalid Date Format

**Purpose**: Verify error handling for invalid dates

```bash
./target/release/putio-sync history --since "invalid-date"
```

**Expected Behavior**:
- ✅ Error message: "Invalid date format: 'invalid-date'"
- ✅ Message shows expected format: "YYYY-MM-DD"
- ✅ Exit with error code

---

### Test 19: Help Display

**Purpose**: Verify help message is informative

```bash
./target/release/putio-sync history --help
```

**Expected Behavior**:
- ✅ Display command description: "View download and delete history"
- ✅ List all options:
  - --action <ACTION>
  - --since <DATE>
  - --until <DATE>
  - -n, --limit <N>
  - --offset <N>
  - -o, --output <FORMAT>
- ✅ Show default values (limit: 50, offset: 0, output: table)
- ✅ Clear descriptions for each option
- ✅ Examples of valid action values (downloaded, deleted)
- ✅ Examples of valid output formats (table, json, csv)

---

### Test 20: Size Formatting Accuracy

**Purpose**: Verify human-readable size formatting

**Setup**: Download files of various sizes

```bash
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Bytes (< 1 KB): "512 B"
- ✅ Kilobytes (< 1 MB): "1.50 KB"
- ✅ Megabytes (< 1 GB): "150.25 MB"
- ✅ Gigabytes (< 1 TB): "1.50 GB"
- ✅ Terabytes: "4.56 TB"
- ✅ Two decimal places for KB/MB/GB/TB

---

### Test 21: Speed Formatting

**Purpose**: Verify transfer speed formatting

**Expected Behavior**:
- ✅ Bytes/sec (< 1 KB/s): "512 B/s"
- ✅ Kilobytes/sec (< 1 MB/s): "150.50 KB/s"
- ✅ Megabytes/sec (< 1 GB/s): "50.25 MB/s"
- ✅ Gigabytes/sec: "1.25 GB/s"
- ✅ Two decimal places
- ✅ Dash "-" for deleted entries (no speed)

---

### Test 22: Duration Formatting

**Purpose**: Verify time duration formatting

**Expected Behavior**:
- ✅ Seconds only (< 1 min): "45s"
- ✅ Minutes and seconds: "1m 30s"
- ✅ Hours, minutes, seconds: "2h 15m 30s"
- ✅ Hours only: "3h"
- ✅ No unnecessary units

---

### Test 23: Filename Truncation

**Purpose**: Verify long filenames are truncated properly

**Setup**: Download a file with a very long name (> 40 characters)

```bash
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Filenames truncated to 40 characters
- ✅ Ellipsis "..." added at end if truncated
- ✅ Truncated filename is 40 characters including "..."
- ✅ Short filenames not truncated

---

### Test 24: Count Formatting with Thousands Separator

**Purpose**: Verify number formatting in pagination info

**Setup**: Have > 1000 history entries

```bash
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Pagination shows "Showing 1 - 50 of 1,234 entries"
- ✅ Proper comma placement in thousands
- ✅ < 1,000: "999"
- ✅ Thousands: "1,234"
- ✅ Millions: "1,234,567"

---

### Test 25: Database Path from Config

**Purpose**: Verify history command uses correct database path

```bash
# Should read database from target directory specified in config
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Uses database at: <target_directory>/.putiosync.db
- ✅ Shows history from that database
- ✅ Error if database doesn't exist

---

### Test 26: Missing Database File

**Purpose**: Verify error handling when database doesn't exist

**Setup**: Delete or move the database file

```bash
./target/release/putio-sync history
```

**Expected Behavior**:
- ✅ Error message: "Failed to open database"
- ✅ Helpful message: "Have you downloaded any files yet?"
- ✅ Exit with error code

---

### Test 27: CSV Escaping for Commas

**Purpose**: Verify CSV escaping for filenames with commas

**Setup**: Download or delete a file with commas in the name

```bash
./target/release/putio-sync history --output csv
```

**Expected Behavior**:
- ✅ Filename enclosed in double quotes: "file,with,commas.mp4"
- ✅ CSV remains parseable
- ✅ Commas inside quotes don't break format

---

### Test 28: CSV Escaping for Quotes

**Purpose**: Verify CSV escaping for filenames with quotes

**Setup**: Download or delete a file with quotes in the name

```bash
./target/release/putio-sync history --output csv
```

**Expected Behavior**:
- ✅ Quotes escaped as double-quotes: "file""with""quotes.mp4"
- ✅ Field enclosed in double quotes
- ✅ CSV remains parseable

---

### Test 29: CSV Escaping for Newlines

**Purpose**: Verify CSV escaping for filenames with newlines

**Setup**: Download or delete a file with newlines in the name (unlikely but possible)

```bash
./target/release/putio-sync history --output csv
```

**Expected Behavior**:
- ✅ Field enclosed in double quotes
- ✅ Newlines preserved inside quotes
- ✅ CSV remains parseable

---

### Test 30: Multiple Output Formats with Same Data

**Purpose**: Verify consistency between table, JSON, and CSV outputs

```bash
# Get table output
./target/release/putio-sync history --limit 10

# Get JSON output
./target/release/putio-sync history --limit 10 --output json

# Get CSV output
./target/release/putio-sync history --limit 10 --output csv
```

**Expected Behavior**:
- ✅ Same entries in all three formats
- ✅ Same timestamps, actions, filenames
- ✅ Same sizes (human-readable in table/CSV, same in JSON)
- ✅ Same speeds and durations
- ✅ Only formatting differs
- ✅ Same number of entries

---

## Test Scenarios Summary

| # | Test Case | Status | Notes |
|---|-----------|--------|-------|
| 1 | Basic history display | ⬜ | |
| 2 | Filter by action (downloaded) | ⬜ | |
| 3 | Filter by action (deleted) | ⬜ | |
| 4 | Invalid action | ⬜ | |
| 5 | Date filter (--since) | ⬜ | |
| 6 | Date filter (--until) | ⬜ | |
| 7 | Combined date range | ⬜ | |
| 8 | Combined action and date filters | ⬜ | |
| 9 | Pagination (--limit) | ⬜ | |
| 10 | Pagination (--offset) | ⬜ | |
| 11 | JSON output | ⬜ | |
| 12 | CSV output | ⬜ | |
| 13 | JSON with filters | ⬜ | |
| 14 | CSV with filters | ⬜ | |
| 15 | Empty history | ⬜ | |
| 16 | Empty results from filter | ⬜ | |
| 17 | Date format variations | ⬜ | |
| 18 | Invalid date | ⬜ | |
| 19 | Help display | ⬜ | |
| 20 | Size formatting | ⬜ | |
| 21 | Speed formatting | ⬜ | |
| 22 | Duration formatting | ⬜ | |
| 23 | Filename truncation | ⬜ | |
| 24 | Count formatting | ⬜ | |
| 25 | Database path | ⬜ | |
| 26 | Missing database | ⬜ | |
| 27 | CSV escaping (commas) | ⬜ | |
| 28 | CSV escaping (quotes) | ⬜ | |
| 29 | CSV escaping (newlines) | ⬜ | |
| 30 | Format consistency | ⬜ | |

## Quick Test Script

Save this as `test-history.sh` for rapid testing:

```bash
#!/bin/bash

# Quick History Command Test Script
# USAGE: ./test-history.sh

set -e

CLI="./target/release/putio-sync"

echo "=== History Command Test Suite ==="
echo ""

# Test 1: Help
echo "Test 1: Display help"
$CLI history --help
echo ""

# Test 2: Basic history
echo "Test 2: Basic history (default 50 entries)"
$CLI history
echo ""

# Test 3: Downloaded only
echo "Test 3: Downloaded entries only"
$CLI history --action downloaded
echo ""

# Test 4: Deleted only
echo "Test 4: Deleted entries only"
$CLI history --action deleted
echo ""

# Test 5: Pagination
echo "Test 5: First 10 entries"
$CLI history --limit 10
echo ""

# Test 6: JSON format
echo "Test 6: JSON output"
$CLI history --output json --limit 5
echo ""

# Test 7: CSV format
echo "Test 7: CSV output"
$CLI history --output csv --limit 5
echo ""

# Test 8: Date filtering
echo "Test 8: History since 2025-11-01"
$CLI history --since "2025-11-01"
echo ""

# Test 9: Combined filters
echo "Test 9: Downloaded files since 2025-11-01, JSON output"
$CLI history --action downloaded --since "2025-11-01" --output json --limit 3
echo ""

echo "=== Manual Tests Required ==="
echo "Please test the following manually:"
echo "  - Different date formats (YYYY/MM/DD, MM/DD/YYYY, DD-MMM-YYYY)"
echo "  - Invalid action and date error handling"
echo "  - Empty database behavior"
echo "  - CSV escaping with special characters"
echo "  - Large pagination (offset beyond total)"
echo "  - Formatting accuracy for various sizes and speeds"
echo ""

echo "All automated tests completed!"
```

## Validation Checklist

After completing tests, verify:

- [ ] All history entries display correctly
- [ ] Action filtering works for both downloaded and deleted
- [ ] Date filtering works with various formats
- [ ] Pagination works correctly with limit and offset
- [ ] All three output formats (table, JSON, CSV) work
- [ ] JSON output is valid and parseable
- [ ] CSV output is valid and parseable with proper escaping
- [ ] Formatting is consistent and readable
- [ ] Error handling is graceful for invalid inputs
- [ ] Performance is acceptable for large history databases
- [ ] Help text is clear and accurate
- [ ] Filter combinations work correctly

## Cleanup After Testing

```bash
# History is read-only, no cleanup needed
# Database persists for future history queries
```

## Reporting Issues

When reporting issues, please include:

1. **Command executed**: Full command line
2. **Expected behavior**: What should happen
3. **Actual behavior**: What actually happened
4. **Database state**: Number of entries, mix of downloaded/deleted
5. **Error messages**: Full error output
6. **Environment**:
   - OS and version
   - Rust version (`rustc --version`)
   - CLI version (`putio-sync --version`)

## Success Criteria

All tests should:
- ✅ Execute without panics or crashes
- ✅ Display clear, formatted output
- ✅ Show accurate history data
- ✅ Handle edge cases gracefully (empty results, invalid inputs)
- ✅ Provide valid JSON and CSV when requested
- ✅ Match data between table, JSON, and CSV formats
- ✅ Apply filters correctly
- ✅ Support pagination properly

## Notes

- History is retrieved from the SQLite database (.putiosync.db)
- Database is created during first download operation
- History accumulates over time (downloads and deletes)
- Deleted entries have no speed or duration
- Downloaded entries include speed and duration if available
- Timestamps use SQLite datetime format: "YYYY-MM-DD HH:MM:SS"
- Date filters are converted to ISO 8601 format with time component
- Pagination offset allows browsing large history sets
- Default limit is 50 entries to keep output manageable
- All three output formats support the same filtering and pagination options
- CSV escaping follows RFC 4180 standard
