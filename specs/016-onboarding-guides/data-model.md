# Data model: Onboarding guides (016)

Nothing is stored. These are the shapes in `ui/guides.js` and the book.

## Guide

| Field | Meaning | Rules |
|-------|---------|-------|
| `slug` | Path under the docs root, without `.html` | `guides/<name>`; matches a file `site/book/src/guides/<name>.md` and a `SUMMARY.md` entry |
| `title` | Shown in the palette and on links | English source string, translated by the UI catalogs; matches the chapter's H1 |
| `kind` | `setup`, `task` or `switch` | Sets the palette detail ("Setup guide", "Guide", "Switching guide") |
| `words` | Extra English search words | For example `grafana prometheus` for the metrics guide; may be empty |

The index (`guides/index`) is in the list as a `task`-less entry with kind `index`.

## Platform map

`channel.kind` (from `GET /update`) to setup guide slug. The table is in
[contracts/ui-links.md](contracts/ui-links.md). Kinds missing from the map, or a failed
`/update`, go to `guides/index`.

## Guide status (pages only)

A page is either walked through or untested. Untested pages start with the R9 blockquote.
There is no field for this in the UI.
