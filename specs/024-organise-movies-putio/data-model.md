# Data Model: Organise Movies on Put.io

## OrganiseConfig (`[organise]`), new fields
| Field | Type | Default | Rule |
|---|---|---|---|
| `movies` | bool | `false` | turns movie moves on |
| `movies_folder` | string | `"Movies"` | relative to the Put.io root, no `..` |

`any()` is `tv || movies`.

## OrganiseKind
`tv` | `movie`. Stored in `organise_moves.kind` (text; no migration) and used as the metric label.

## Movie (release reader)
| Field | Meaning |
|---|---|
| `title` | words before the year, case kept |
| `year` | release year |
| `part` | `1` for `CD1`, `Part1`, `Disc1`, `pt1` after the year |
| `extra` | extras folder name when an extra word follows the year |
| `sample` | true when `sample` follows the year |

State transitions of a move are 023's: planned → applied → undone, failed; dismiss removes.
