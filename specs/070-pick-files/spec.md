# Feature 070: Pick files inside a transfer

**Roadmap**: 070 · **Status**: built

## Problem

A torrent often carries files nobody wants on the NAS: a sample, subtitles in ten
languages, or eleven episodes beside the one that was missing. The only way to leave them
out is a folder rule or a glob filter, which applies to every transfer in that folder and
has to be written first.

## What it does

**Choose files** on a transfer, an Active item or a queue row opens the transfer's files
as a tree, with sizes and a tick box each. Ticked files download; the rest stay on Put.io.
The choice is for that transfer only and wins over the folder rules and filters for it. A
transfer added from the web UI can have its files chosen before anything downloads.

## User stories

- **US1** Untick the files of a transfer I don't want, and only the rest download.
- **US2** "Only this" keeps one file; "All but this" drops one.
- **US3** Tick a file a filter or folder rule blocks, and it downloads for this transfer.
- **US4** Add a `.torrent` and choose its files before it is added.
- **US5** Add a magnet link and have nothing of it download until I choose.
- **US6** See that a transfer waits for my choice, and which files my choice left out.

## Requirements

- **FR-001** A choice belongs to one Put.io transfer: a flag per file path (relative to
  the transfer's file or folder, names cleaned the way they are on disk) and what to do
  with files it does not name (`others`: `download` or `skip`).
- **FR-002** In the sync, a file of a transfer with a choice follows the choice alone:
  wanted files skip the folder rules, stream mode and filters (built-in and plugin);
  unwanted files are not queued and never deleted from Put.io. Other transfers are not
  affected.
- **FR-003** The choice is stored in the database, so it survives a restart and applies
  to files that sync later. It is forgotten when the transfer's files are gone from
  Put.io, or when the transfer leaves Put.io's list before it had any.
- **FR-004** Saving a choice applies it at once: queue rows of unwanted files that are
  pending, paused or failed are removed, and a sync queues newly wanted files. A file that
  is downloading or already downloaded is left alone.
- **FR-005** A choice can be `pending`: no files named, nothing downloads. The transfer
  shows the reason `choose_files` on the Active page, while Put.io downloads it and after,
  until files are chosen or the choice is forgotten.
- **FR-006** `POST /putio/transfers/preview` lists the files of a `.torrent` without
  sending anything to Put.io. `POST /putio/transfers` takes `choice` (saved for the new
  transfer before its files can be queued) or `choose_first` (a pending choice).
- **FR-007** "Let the rules decide" forgets the choice; the next sync treats the transfer
  like any other.
- **FR-008** An explicit Download from the Put.io page, and Radarr and Sonarr's own
  queueing, take every file as before: the choice is about what the sync picks up.
- **FR-009** The tree is native check boxes and buttons: reachable by keyboard, a folder
  box reads "mixed" when some of its files are ticked, and a folder's arrow says whether it
  is open. The count of chosen files is announced when it changes.

## Codes (added to 066)

| Code | Meaning | Where |
|---|---|---|
| `choose_files` | the transfer waits for its files to be chosen | `waiting` on `GET /active` items |
| `not_chosen` | the file was left out by the choice | `waiting` on `GET /putio/transfers/{id}/files` rows |

Active items also carry `chosen: true`; their skipped-files line then names the choice
instead of the rules. `GET /putio/transfers` rows carry `choice`: `"pending"` or
`"chosen"`.

## Left out, and why

- **Choosing a magnet's files at the moment it is added.** Put.io lists a magnet's files
  only once it has fetched the torrent, so there is nothing to show. The add form holds
  the transfer instead (US5) and the files are chosen from its row.
- **Stopping a download that is already running** when its file is unticked. It finishes;
  the picker says so beside the file. Cancel it on the Queue page.
- **Removing unticked files from Put.io.** They stay in the account; the Put.io page
  deletes them.
- **Queue order and priority.** They exist already and are not part of this.
- **The CLI.** The API routes are there; no `putio-sync` command wraps them yet.

## Success criteria

- **SC-001** With a choice saved, a sync queues exactly the ticked files of that transfer,
  whatever the rules say, and the same rules still decide for every other transfer.
- **SC-002** A held magnet downloads nothing until its files are chosen, across restarts.
