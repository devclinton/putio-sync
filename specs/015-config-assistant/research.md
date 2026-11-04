# Research: Configuration Assistant

## R1. How a preview stays identical to a save

- **Decision**: split each group's PUT into `apply_<group>(body, &mut Config) -> Result<()>`
  (the existing closure body) and the side effects. `update()` and the new `preview()` both
  call it on a clone; `preview()` then runs `Config::validate` and diffs the group view,
  never takes the write lock, never writes.
- **Rationale**: one code path for parsing and validation means FR-002 holds by
  construction.
- **Alternatives**: a `?dry_run=true` flag on PUT (mixes a read into a write route and its
  side effects); validating in the browser (duplicates rules, drifts).

## R2. Applies now or needs a restart

- **Decision**: move the per-group live lists (`DOWNLOAD_LIVE`, the watcher's engine logic,
  logging's `RUST_LOG` logic, cleanup's engine logic) into `live_<group>(&AppState) ->
  (&[&str], &str)`, used by the save message, the preview and the help.
- **Rationale**: the three places can never disagree.

## R3. Where the "what it affects" text lives

- **Decision**: a static table in the API settings module (`AFFECTS`), keyed by group and
  field, served by `GET /settings/assistant`. A test checks every field of every group view
  has an entry.
- **Rationale**: keeps the UI bundle small (cap 300 KB) and tested against the views.
- **Alternatives**: in `settings.js` next to the hints (costs about 3 KB of UI and has no
  completeness test).

## R4. Risk rules

- **Decision**: `assistant::risks(&Config) -> Vec<Risk>` calls `risks_in(config, &Host)`,
  where `Host { in_container, root_dev }` is read from the system; tests pass their own.
  Rules:
  1. `delete_temp_folder` (danger): `download.delete_after_download` and the download folder
     (or its nearest existing parent) is under the system temp dir, `/tmp` or `/var/tmp`.
  2. `delete_container_layer` (danger): delete-after-download, in a container, and the
     download folder's nearest existing parent is on the same device as `/` (Unix only).
  3. `watch_in_download` (warning): the watcher is on and a watch folder is the download
     folder or inside it.
  4. `archive_in_watch` (warning): the archive folder is a watch folder or inside one.
  5. `cleanup_no_min_age` (warning): `cleanup.enabled` and `min_age_hours == 0`.
  6. `many_workers` (warning): `download.workers > 10`.
  7. `verbose_log_file` (warning): `logging.file_enabled` and level `trace` or `debug`.
- **Rationale**: these are the combinations that lose data or cause loops or load; each is
  decidable from the config and the host alone.
- **Alternatives considered and left out**: an open API without a token (the token can come
  from the environment, so the config alone can't tell); deleting the `.torrent` after
  upload with no archive (it is the default and expected).
- Paths are compared after `expand_paths` and lexically (`Path::starts_with`), plus with
  `canonicalize` when both exist, so `/tmp` on macOS (a symlink to `/private/tmp`) matches.

## R5. Export and import

- **Decision**: export is `toml::to_string_pretty(settings_table(config))`, so it leaves
  out `[auth]`, `[arr]` and `api.auth_token` exactly as the save path does. Import parses the
  text as a table, removes and reports those keys as ignored, lints it with
  `config_lint::lint` for unknown keys, deep-merges it over the current `settings_table`,
  deserialises a `Config`, puts back the current `auth`, `arr` and `api.auth_token`, and
  runs `expand_paths`. The API then validates, diffs (dotted keys) and, on `apply: true`,
  saves with `save_keeping_file_credentials` and updates the running config, the download
  manager, the doctor and the cleanup service.
- **Rationale**: reuses the save path's secret handling and comment-preserving writer.
- **Limits**: body over 1 MB → 400; TOML parse error → 400 with message and line only (the
  parser's own text can quote a line that holds a token, as in the doctor).

## R6. Defaults

- **Decision**: `GET /settings/assistant` returns each group's view of `Config::default()`.
  The UI rebuilds the card's form from those values, keeping fields the view does not hold
  (the precheck `effective` table is not part of defaults) and runs the preview.
- The default download folder is the default `target_directory`; the preview shows it like
  any other change.

## R7. UI size

- **Decision**: one shared `assist()` helper in `settings.js` adds the preview panel, help
  lines and Defaults button to every group form; one small backup card. Budget: under 9 KB.
