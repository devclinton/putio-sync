# Contract: `[subtitles]` additions (TOML)

All new keys are optional. A 025 config file loads, runs and saves with no change.

```toml
[subtitles]
languages = ["en", "es"]                 # 025, unchanged
excluded_categories = []                 # 025, unchanged; wins over any mode
mode = "download_then_whisper"           # download_only | download_then_whisper | whisper_only | both
                                         # absent = download_then_whisper (025 behaviour)
min_confidence = 65                      # 0..=100; OpenSubtitles matches below it are not saved

[subtitles.category_modes]               # category (case-insensitive) -> mode
anime = "whisper_only"

[subtitles.language_modes]               # wanted language -> mode, most specific
ja = "both"

[subtitles.opensubtitles]
api_key = ""                             # your own consumer key; empty = this part off. Env PUTIO_SYNC_OPENSUBTITLES_API_KEY
username = ""                            # optional, for a larger quota
password = ""                            # optional; env PUTIO_SYNC_OPENSUBTITLES_PASSWORD
user_agent = ""                          # empty = "putio-sync <version>"

[subtitles.transcription]                # 025, unchanged
```

## Resolution

`mode_for(category, language)`: `language_modes[language]`, else `category_modes[category]`, else `mode`, else
`download_then_whisper`.

## Validation (save time)

- `mode`, map values: one of the four names, else error naming the key and value.
- `language_modes` keys: known language (025 table); stored as canonical codes.
- `category_modes` keys: non-empty after trim; lowercased.
- A mode of `whisper_only` or `both` anywhere requires `transcription.enabled = true` and a valid
  `transcription.url`: error `mode "both" (language_modes.ja) needs transcription turned on with a server URL`.
- `min_confidence` 0 to 100.
- `password` set with empty `username`: error. `api_key` empty with `username` set: warning in the settings view
  only (the part stays off).

## Masking and secrets

`api_key` and `password` print `<redacted>` in `Debug`, show as `********` in API responses, and `********` in a
PUT keeps the stored value (as 025's transcription key). Env overrides win over the file for the effective value.

## Documentation

`config.example.toml` gets commented examples for every key above (Constitution: new settings documented).
