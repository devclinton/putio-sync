# Research: Local Files Page

## R1 Keeping every path inside its root

- **Decision**: The client sends a root key and a path relative to it, `/`-separated. The server
  rejects an absolute path, any `..` or `.` segment, empty segments in the middle, backslashes and
  NUL. It canonicalizes the root once per request, canonicalizes the target's parent folder, and
  requires that parent to start with the canonical root. The last segment is not followed: it is
  read with `symlink_metadata`, so a link is listed, deleted or renamed as a link.
- **Rationale**: Canonicalizing the parent catches a symlinked folder anywhere in the chain, which
  a string prefix check (the old routes) does not. Not following the last segment means deleting
  a link never deletes its target, and `remove_dir_all` does not follow links inside a folder.
- **Alternatives**: `openat`-style fd-relative walking (`cap-std`): closes the race between check
  and use, but adds a dependency for a single-operator tool where the only writers are
  putio-sync itself and the *arrs. Rejected under Principle VII.

## R2 Roots

- **Decision**: `downloads` is `[download] target_directory`; `watch-N` is the N-th
  `[watch] paths` entry, listed only when `[watch] enabled`. Keys, not paths, go in URLs, read
  from the live config on each request so a settings change applies at once.
- **Rationale**: The client can never name an arbitrary folder; it can only pick a root the
  config has. Category subfolders (`sonarr/`) are just folders under `downloads`.

## R3 Partial downloads in use

- **Decision**: `DownloadManager::partial_paths_in_use()` returns the `.downloading` path for each
  target in `paths_in_use`. Delete refuses a partial whose canonical path is in that set, or a
  folder that contains one (409 Conflict). Listings set `in_use` on those entries.
- **Rationale**: Deleting a partial under a running worker makes the worker fail mid-write and
  retry. The Queue page's cancel is the right tool there.

## R4 Re-queue a rejected watch file

- **Decision**: Only in a watch root, only for `<name>.rejected` where `<name>` ends in
  `.torrent` or `.magnet` and does not exist. Steps: rename back; compute the watcher's checksum
  (file SHA-256 for a torrent, SHA-256 of the trimmed content for a magnet); delete that
  `processed_torrents` row; drop the path from the shared watcher's in-memory processed set when
  the API has the watcher. The next scheduled scan checks it again.
- **Rationale**: `reject_file` records the checksum as processed, so a plain rename back would be
  skipped as "already uploaded". No scan is started from the API: the daemon's loop is the only
  scanner, so a second concurrent scan cannot upload the same file twice.
- **Alternatives**: Upload directly, skipping the check: out of scope (the spec keeps the release
  check in charge); "Not fake" flags stay in History.

## R5 Folder size for the delete confirmation

- **Decision**: `GET /local/files/{root}/summary?path=` walks the entry (without following links)
  and returns files, folders, bytes and whether it holds an active partial. The UI calls it
  before asking to delete a folder.
- **Rationale**: Listings stay cheap (one `read_dir`); the walk only happens for the one folder
  about to be deleted.

## R6 Listing size

- **Decision**: At most 2,000 entries per listing, sorted folders first then by name
  (case-insensitive), with `truncated: true` when cut.
