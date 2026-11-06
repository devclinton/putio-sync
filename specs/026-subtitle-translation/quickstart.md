# Quickstart: Subtitle Translation (026)

Needs 025 (subtitles) set up with at least two wanted languages, for example `en` and `es`.

## Chat model with Ollama (US1)

1. Run Ollama and pull a model: `ollama pull qwen2.5:14b`. Give it room for a long prompt: set `OLLAMA_CONTEXT_LENGTH=8192`
   (or a Modelfile `PARAMETER num_ctx 8192`); the default can truncate 40-cue chunks.
2. Settings, Translation card: kind **Chat model**, base URL `http://<host>:11434/v1`, model `qwen2.5:14b`, key empty. Press
   **Test**. Expected: `ok`, two Spanish sample lines, the resolved endpoint. Try a wrong model name: expect "model not found"
   with the list of models; a wrong port: "unreachable". Save.
3. Put an English-only video through the pipeline (or use the Local files action below). In the Active timeline the
   Translating stage shows a `videos` line, then one branch per missing language, each with progress. When done,
   `Movie.es.srt` sits next to the video with the same cue count and times as `Movie.en.srt`.
4. Check: `diff <(grep -- '-->' Movie.en.srt) <(grep -- '-->' Movie.es.srt)` prints nothing.
5. Metrics: `curl -s localhost:<port>/metrics | grep putio_sync_translation`.

## Custom API with a callback (US2)

1. Run a tiny service that answers 202 and calls back (about 15 lines of Python with `http.server` and `urllib`: on
   `POST /translate` read the JSON, reply 202, then after 5 seconds POST an SRT (the `srt` field with each text upper-cased) to
   `callback_url`). Variant 2: reply 200 with the SRT at once.
2. Settings, Translation: kind **Custom API (callback)**, URL `http://<host>:9000/translate`, callback base URL
   `http://<this nas>:<api port>` (use the **Use this address** button). Saving without the base URL must be refused. Press Test:
   expect `will_call_back` (or `immediate`) and `base_url_reachable_from_here`.
3. Run a translation. The branch shows "waiting for callback" until the service calls back; then the file appears.
4. Replay the same callback: `curl -i -X POST --data-binary @sample.srt http://<nas>:<port>/translate/callback/<token>` gives `404`.
   A random token gives `404` too. Restart putio-sync while the service is still waiting: the job survives and the
   callback is accepted afterwards.
5. Make the service never call back and set `timeout_secs = 60`: after a minute the step fails and the item ends
   "done with warnings".
6. Logs: `grep translate/callback` in the log viewer shows `<redacted>` and never a token.

## Per category and by hand (US3)

1. Add `radarr` to the Translation card's excluded categories; a radarr download shows the stage skipped "not for this category".
2. Local files page, a video row menu, **Translate subtitles**: choose Spanish; the page follows the run and shows the saved
   file. A target that already has a file is disabled.

## Verify (developer)

- `make ci`.
- Migration: copy a DB that has 025's tables, start the daemon, check `pipeline_attempts` row count is unchanged and both new tables exist.
- First run against a real Ollama and a real hosted API (optional): record real error bodies for a wrong model and a wrong key
  in `contracts/chat.md` (open question Q1).
