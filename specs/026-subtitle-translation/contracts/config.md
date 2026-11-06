# Contract: `[translation]` configuration

```toml
[translation]
kind = "none"                   # "none" (stage off), "chat" (OpenAI-compatible model) or "callback" (custom API)
excluded_categories = []        # categories that skip the stage, e.g. ["radarr"]
max_in_flight = 2               # requests to the translator at once, across all languages, 1..=8
timeout_secs = 180              # per HTTP request, 10..=1800 (both kinds)
chunk_cues = 40                 # cues per request (chat), 5..=200
instruction = ""                # extra instruction, at most 500 characters, e.g. "informal Spanish (Spain)"

[translation.chat]
base_url = ""                   # "http://ollama.lan:11434/v1"; "/chat/completions" is appended (see chat.md)
model = ""                      # "qwen2.5:14b"
api_key = ""                    # sent as a Bearer token when set; masked in the API, never logged

[translation.callback]
url = ""                        # where putio-sync POSTs the subtitle
callback_base_url = ""          # how the service reaches putio-sync, e.g. "http://nas.lan:8088"
timeout_secs = 3600             # how long to wait for the callback, 60..=86400

[translation.callback.headers]  # optional; values are masked in the API, never logged
# Authorization = "Bearer ..."
```

Target languages come from `[subtitles] languages` (025). With none set the stage does nothing; Settings shows a note.

Rules:

- `kind = "chat"` requires `chat.base_url` (`http`/`https` with a host) and `chat.model` (not empty). `api_key` may be empty.
- `kind = "callback"` requires `callback.url` (`http`/`https` with a host) **and** `callback.callback_base_url`
  (`http`/`https`, a host, no user info, no query or fragment; a path prefix is allowed; a trailing `/` is removed).
  Saving without the base URL is refused (FR-016). Header names must be valid HTTP header names; at most 20; a value of
  `********` keeps the stored value, and a leftover mask for a new name is an error.
- `instruction` has newlines collapsed to spaces.
- A kind's fields are validated only for the kind that is selected; the other kind's fields are kept as typed.
- Applies now: changes apply to new items and to waiting steps; a running translation finishes with the settings it
  started with. A callback job already sent keeps its original expiry.
- Not configurable on purpose: context cues (3), split depth (3), the 429 wait budget (300 s), the callback body limit
  (4 MiB), the reply format.
- `api_key` and header values may come from the environment through the existing override mechanism.
- Documented in `config.example.toml` with the defaults above.
