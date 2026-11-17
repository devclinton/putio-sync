# Contract: UI language setting

## `config.toml`

```toml
[ui]
# Language for the CLI and the web UI's "Automatic" choice: a code from
# `putio-sync languages` (for example "de" or "pt-PT"). Leave unset to follow the
# browser in the web UI and the system locale in the CLI.
# language = "de"
```

- Unset or empty: automatic.
- Unknown value: treated as unset. Config load stays quiet; the CLI warns once when it picks the
  language (see cli.md), and the doctor's Config check reports it as a warning.
- `PUTIO_SYNC_LANG` and `--lang` override it for the CLI only; they never write it.
- 056's setup wizard writes this key.

## `GET /api/v1/settings/ui`

Auth as the other settings groups.

```json
{ "language": "de" }
```

`language` is `null` when unset.

## `PUT /api/v1/settings/ui`

```json
{ "language": "fr" }
```

- `null` or `""` clears it.
- An unknown code → `400` with `{"error": "unknown language \"xx\""}`.
- Saved through the same config writer as other groups; no restart needed (the CLI reads it each
  run; the web UI reads it on load).

## Web UI

The Language card's first option is renamed from "Browser default" to "Automatic". With it, the
UI uses, in order: the config's `language`, the browser's languages, English. A language picked in
the card is still saved per browser and wins over the config. The sign-in page, shown before
settings can be read, uses the saved browser choice or the browser's languages.
