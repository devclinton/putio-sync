# Contract: `[subtitles]` configuration

```toml
[subtitles]
languages = []                  # ordered wanted languages, e.g. ["en", "es"]; empty = stage off
excluded_categories = []        # categories that skip the stage, e.g. ["radarr"] when Bazarr handles them

[subtitles.transcription]
enabled = false                 # off by default
url = ""                        # "http://host:8000" (path added: /v1/audio/transcriptions) or a full URL
model = ""                      # sent as `model` when set
api_key = ""                    # Bearer token when set; masked in the API, never logged
timeout_secs = 900              # per request, 10..=7200
language = ""                   # optional hint; empty = server detects
chunk_secs = 600                # audio part length, 60..=1800
ffmpeg_path = ""                # empty = search PATH
```

Rules:

- Language entries accept ISO 639-1/639-2 codes and English names; stored as the canonical code (two-letter where it
  exists). Duplicates removed in order. More than 10 entries or an unknown entry is a validation error naming it.
- `enabled = true` requires an `http`/`https` URL with a host.
- Not configurable on purpose: the 100 MB size rule, the `sample` name rule, forced/SDH naming.
- Applies now: changes apply to new items and to waiting steps (steps read config when they run).
- Env override follows the existing config mechanism; `api_key` may come from the env.
- Documented in `config.example.toml` with the defaults above.
