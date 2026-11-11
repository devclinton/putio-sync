# Research: Organise TV Shows on Put.io

## R1 Where names come from

- **Decision**: the 050 release reader (`ReleaseInfo`), moved to `putio-sync-core::release`, plus
  an `Episode` reader in the same file that keeps the original letter case and also reads the
  last episode of a multi-episode file and the episode-title words.
- **Rationale**: one tokeniser for subtitles and organising; `download` can't depend on
  `subtitles`. `ReleaseInfo.title` is lowercase (for comparing), which is wrong for folder names,
  so the new reader keeps case and capitalises words that are all lowercase.
- **Alternatives**: *arr data — streamed files never come from the *arr (022 downloads those),
  so there is nothing to look up. Sonarr's `/parse` or TVDB — network, keys, and no Sonarr for
  stream-only users; rejected (VII).

## R2 Season packs

- **Decision**: when the file name has an episode number but no show title (`S01E03.mkv`), the
  show (and the year) comes from the nearest folder whose name reads as a release
  (`Show.Name.S01.1080p…`).
- **Rationale**: packs are common; the folder holds the release name.

## R3 Where moves go and safety checks

- **Decision**: `[organise] tv_folder` is a Put.io path from the synced root (the account root).
  Applying refuses when folder rules make that folder a download folder (the next sync would
  download the moved file) and when the target name already exists.
- **Rationale**: FR-008, FR-011. Put.io allows duplicate names, so the check is ours.

## R4 Apply order and undo

- **Decision**: read the file (`GET /files/{id}`) for its parent and name, record them, ensure the
  folders (`files/list` + `files/create-folder`, cached per batch), rename (`POST /files/rename`)
  then move (`POST /files/move`). If the move fails after a rename, rename back. Undo is the
  reverse with the recorded parent and name, refused when that folder is gone.
- **Rationale**: the file id never changes, so 022's stream record and MP4 version stay valid.

## R5 Plugins

- **Decision**: with `use_plugins` (default on), the organiser asks the 028 `folder` and `rename`
  hooks with `decision = "stream"`. A folder answer is read from the synced root (the Put.io
  counterpart of the download folder); a name answer replaces the built-in name. A plugin answer
  plans a move even for a file the built-in reader can't place.
- **Rationale**: the brief asks whether hooks may override built-in naming; plugins already
  receive `decision`, so they can tell a streamed file from a download.

## R6 UI budget

- **Measured**: the eager UI is 499,424 bytes against a 499,712-byte cap (488 KiB).
- **Decision**: `ui/organise.js` holds the whole panel (settings and moves) and is imported with
  `import()` when the Put.io page's "Organise" button is pressed; the eager cost is the button.
- **Alternatives**: raise the cap (022 did, 484 → 488) — the brief keeps it; trim other pages —
  unrelated changes.

## R7 Movies (024)

- **Decision**: `OrganiseKind` (`tv` now), stored per move; `organise::plan_target` tries each
  enabled kind. 024 adds `Movie`, `movies`/`movies_folder` settings and its layout function. The
  table, routes, apply and undo don't change.
