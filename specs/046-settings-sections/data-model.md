# Data Model: Settings Split into Sections

All UI-side; nothing new is stored on the server.

## Section

`{ id, title, help, cards: [[cardId, title], …] }` in `ui/settings/sections.js`.

| id | Title | Cards |
|----|-------|-------|
| general | General | appearance, credentials, notifications |
| putio | Put.io | putio (Put.io), storage (Put.io storage), cleanup (Put.io cleanup), watcher (Torrent watcher) |
| downloads | Downloads | download (Downloads), bandwidth (Bandwidth), verify (Verification) |
| rules | Rules and filters | filters (File filters), folder-rules, presets, blocked (Recently blocked), precheck (Release check) |
| integrations | Integrations | arr (Radarr / Sonarr), plugins, hooks (Event hooks) |
| network | Network and access | network, tailscale |
| advanced | Advanced | logging, backup (Backup and restore) |

Every card's root element has id `card-<cardId>`.

## Card (group card definition)

`{ id, key, title, fields, advanced?, part?, extra?, beforeSave?, locked? }`

- `key`: the settings group (`GET/PUT /settings/<key>`).
- `fields`: field tuples as before: `[name, label, hint, kind, min, max]`.
- `part`: only some of the group's fields are on this card (R2).

Download group split:

| Card | Fields | Advanced |
|------|--------|----------|
| download | workers, target_directory, max_retries, sort_order, on_existing, resume_enabled, delete_after_download, delete_empty_folders, buffer_size_mb, chunk_size_mb, retry_initial_delay_secs, progress_style, reverse_sort | buffer_size_mb, chunk_size_mb, retry_initial_delay_secs, progress_style, reverse_sort |
| bandwidth | bandwidth_limit_bytes_per_sec, slow_limit_bytes_per_sec + schedule (`bandwidth_schedule`) | slow_limit_bytes_per_sec |
| verify | verify, verify_max_redownloads, verify_read_limit_mb_per_sec, verify_follow_bandwidth_pause | the last three |
| filters | whitelist_patterns, blacklist_patterns, blocked_extensions + pattern preview | — |

Other advanced fields: watcher refresh_cycle, parent_folder_id; cleanup min_age_hours, nested;
storage run_at; tailscale socket; network metrics_bind; plugins plugins_dir; logging
console_format, module_filters; precheck lookup_timeout_secs, caches, hold_magnet_secs,
hold_retry_secs, first_lookup_secs and the two override tables.

## Card state (reported to the shell)

`ctx.report(cardId, { dirty: bool, problem: bool })`: a section's mark is "unsaved" when any of
its cards is dirty; its problem count is its problem cards plus assistant risks whose first key
falls in the section.
