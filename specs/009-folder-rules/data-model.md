# Data model: Per-folder file rules

## Config (`config.toml`, `[download]`)

```toml
[download]
blocked_extensions = ["exe", "msi", ...]   # the root filter (unchanged key)
delete_after_download = false              # root value
skip_existing = true                       # root value

[[download.rules]]
path = "Movies"            # required; relative to the Put.io root
preset = "video"           # optional; built-in or [download.presets] name
allow = ["nfo"]            # optional; extra allowed extensions
block = ["txt"]            # optional; extra blocked extensions
skip = false               # optional; download nothing below this folder
delete_blocked = true      # optional; delete type-blocked files from Put.io
delete_after_download = true  # optional
skip_existing = false      # optional

[download.presets.comics]  # user preset
only = ["cbz", "cbr"]

[download.presets.video]   # overrides the built-in list
only = ["mkv", "mp4", "m2ts"]
```

### FolderRule

| Field | Type | Rules |
|---|---|---|
| `path` | string | Normalised: split on `/`, empty segments dropped, each segment character-replaced like download paths. Refused: empty (the root is `[download]`), `.` or `..` segments. Unique across rules. |
| `preset` | string? | Must name a built-in or user preset. |
| `allow`, `block` | string[] | Extensions: leading `.` stripped, lowercased; refused if empty or containing `/`, `\` or whitespace. Omitted when empty. |
| `skip`, `delete_blocked`, `delete_after_download`, `skip_existing` | bool? | Absent = inherit. |

### Preset

| Field | Type | Rules |
|---|---|---|
| name (table key) | string | `[a-z0-9][a-z0-9_-]{0,31}` |
| `only` | string[]? | Allow list; if present must be non-empty. |
| `block` | string[] | Block list; used when `only` is absent. Extensions validated as above. |

Built-ins (labels shown in the UI): `video` "Video only", `audio` "Audio only", `ebooks` "Ebooks only", `no-binaries` "No binaries", `everything` "Everything". A user entry under a built-in name replaces it (`overridden`); deleting that entry resets it. A user preset used by a rule cannot be deleted.

## Resolved (in memory, `core::rules`)

- **RuleSet**: global filter and booleans, rules keyed by normalised path, presets (built-ins merged with overrides). Built from `[download]`; part of the download manager's config, so `update_config` hot-reloads it.
- **Effective** (for a folder path): `filter {only, blocked}` + `filter_from`, `preset` + its source, `skip`, `delete_blocked`, `delete_after_download`, `skip_existing`, each with its source. Source = `None` (global) or the rule path.
- **Decision** (for a file in a folder): `Download`, `Blocked { reason, rule }`, or `Skipped { rule }`. Reasons: `NotAllowed { extension, preset }` (allow list), `Blocked { extension }`.

## Database: `rule_decisions`

```sql
CREATE TABLE IF NOT EXISTS rule_decisions (
    file_id INTEGER PRIMARY KEY,
    filename TEXT NOT NULL,
    folder_path TEXT NOT NULL,
    decision TEXT NOT NULL CHECK(decision IN ('blocked', 'skipped', 'deleted')),
    reason TEXT NOT NULL,
    rule_path TEXT,            -- NULL: the global settings
    decided_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
```

Index on `decided_at`. Upsert on decision (first time per process, and on delete); row removed when the file is queued; whole table cleared from the UI.

State per file: *(none)* → `blocked`/`skipped` → (`deleted` if delete_blocked) ; any → *(none)* when queued.

## Metrics

- `putio_sync_rule_decisions_total{decision="blocked|skipped|deleted"}`: counter, once per file id per process (deleted: per successful delete).
- `putio_sync_files_blocked_total`: unchanged name; now counts blocked and skipped files (help text updated).
