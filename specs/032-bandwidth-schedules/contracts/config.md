# Contract: `[download]` bandwidth keys

```toml
[download]
# The default limit, outside every schedule entry. Absent: unlimited.
bandwidth_limit_bytes_per_sec = 5242880     # 5 MiB/s
# "Slow for 1 hour" uses this (or the schedule's limit, if lower). Default 1 MiB/s.
slow_limit_bytes_per_sec = 1048576

# Entries are checked in order; the first one containing the daemon's local time wins.
[[download.bandwidth_schedule]]
days = "daily"
start = "01:00"
end = "07:00"
limit = "unlimited"

[[download.bandwidth_schedule]]
days = "mon-fri"
start = "18:00"
end = "23:00"
limit = "paused"

[[download.bandwidth_schedule]]
days = "fri,sat"
start = "22:00"
end = "02:00"          # past midnight: Friday 22:00 to Saturday 02:00, and Sat to Sun
limit = 2097152
```

Errors (refused by config loading, the wizard, Settings saves and import):

| Mistake | Message |
|---------|---------|
| `start = "25:00"` | `download.bandwidth_schedule entry 1: start '25:00': expected HH:MM between 00:00 and 23:59` |
| `end = "7"` | `download.bandwidth_schedule entry 1: end '7': expected HH:MM between 00:01 and 24:00` |
| `days = "mon-fry"` | `download.bandwidth_schedule entry 1: days 'mon-fry': unknown day 'fry'` |
| `days = ""` | `download.bandwidth_schedule entry 1: days '': expected at least one day` |
| start = end | `download.bandwidth_schedule entry 1: start and end are both 08:00; use 00:00 to 24:00 for a whole day` |
| `limit = 0` | `download.bandwidth_schedule entry 1: limit 0: write "paused" to stop downloads` |
| `limit = "fast"` | TOML/JSON error from the limit parser: `expected "unlimited", "paused" or bytes per second` |
| 51 entries | `download.bandwidth_schedule: at most 50 entries` |
| `bandwidth_limit_bytes_per_sec = 0` | `download.bandwidth_limit_bytes_per_sec must be at least 1; leave it out for unlimited` |
| `slow_limit_bytes_per_sec = 0` | `download.slow_limit_bytes_per_sec must be at least 1` |

Warnings (assistant risks, Settings banner and the doctor's Config check):

| Id | When |
|----|------|
| `bandwidth_overlap` | two entries share any minute: "Bandwidth schedule entries 1 and 3 overlap; entry 1 wins where they do." |
| `bandwidth_paused_week` | no minute of the week is unpaused |
| `bandwidth_low` | a limit (default, slow or entry) below 64 KiB/s |
