# Feature Specification: Subtitle Translation

**Feature Branch**: `026-subtitle-translation`

**Created**: 2026-11-05

**Status**: Draft. It builds on 038 (the pipeline) and 025 (subtitles).

**Input**: Roadmap entry 026, "Translate subtitles into the languages wanted through an LLM: a
base URL, model and optional API key, using the OpenAI-compatible chat API so Ollama, Open
WebUI, LM Studio and hosted APIs all work. Subtitles are translated in chunks with timestamps
kept exactly, and a test button checks the connection and model". The user added: "point to an
ollama model or then also maybe calling an api through callback".

## Context

- 025 saves `<video>.<lang>.srt` from Put.io or from transcription. Often there is a subtitle in
  one language and not in the others that are wanted.
- 038 gives the pipeline a Translating stage, with one branch per target language that run in
  parallel. In Transmission mode, Radarr and Sonarr wait for it.
- The user runs Ollama at home. Some users would rather use their own translation service: a
  DeepL wrapper, a script, or a hosted job API that answers later. Hence a second translator
  kind, a callback API.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Translate with my local model (Priority: P1)

The operator sets a translator in Settings:
- a base URL, for example `http://ollama.lan:11434/v1`;
- a model, for example `qwen2.5:14b`;
- an API key, which can be left empty.

They press **Test**, and putio-sync confirms that the server answers and the model translates a
sample line. From then on, when an item's video has a subtitle in one language but not in a
wanted language, putio-sync translates it. It saves `<video>.<lang>.srt`, with every cue's
timing exactly the same as the source.

**Why this priority**: This is the feature the user asked for, and it works with Ollama, LM
Studio, Open WebUI and hosted OpenAI-compatible APIs.

**Independent Test**: A mock chat server echoes each line with a prefix. Translating a 300-cue
English SRT to Spanish gives 300 cues with the same numbers and timestamps and the translated
text, in more than one request.

**Acceptance Scenarios**:

1. **Given** English exists and Spanish is wanted, **When** the Translating stage runs,
   **Then** `<video>.es.srt` is written, with the same cue count and identical timestamps.
2. **Given** Spanish and Japanese are wanted, **When** the stage runs, **Then** both
   translations run in parallel, as two branches on the timeline, each with its own progress.
3. **Given** a long subtitle, **When** it is translated, **Then** it is sent in chunks, each with
   a little overlap of context, and the progress shows the share of cues done.
4. **Given** the model returns a chunk with the wrong number of lines, or broken numbering,
   **When** the reply is checked, **Then** the chunk is retried (smaller if needed). After the
   retries, the step fails without writing a broken file.
5. **Given** a wanted language already has a subtitle, **When** the stage runs, **Then** nothing
   is translated for it.
6. **Given** several source languages exist, **When** a source is picked, **Then** the first
   wanted language that exists is used. If none exists, the transcribed one is used, then any
   other.
7. **Given** Test is pressed with a wrong model name, **Then** the error from the server is shown
   ("model not found"), and nothing is saved.

---

### User Story 2 - Translate through my own API (callback) (Priority: P2)

Instead of a chat model, the operator picks **Custom API (callback)**. They set:
- a URL that putio-sync POSTs the subtitle to, with the source and target language;
- optional headers.

The service can answer in one of two ways:
- at once, with the translated SRT;
- with "accepted", and later POST the result to a callback URL that putio-sync gives it. The
  callback URL is unique to that job and only good for that job.

**Why this priority**: It lets users plug in DeepL, a script, or a slow batch service, without
putio-sync having to support each one.

**Independent Test**: In one case, a mock service answers with SRT at once. In the other, it
answers 202 and then calls the callback URL with SRT. Both save the file. A callback with a
wrong or reused job token is refused.

**Acceptance Scenarios**:

1. **Given** the API answers 200 with SRT, **When** a translation runs, **Then** the file is
   checked (the timestamps match the source) and saved.
2. **Given** the API answers 202, **When** it later POSTs the result to the callback URL,
   **Then** the result is checked and saved, and the branch finishes. Until then, the branch
   shows "waiting for callback".
3. **Given** no callback arrives within the timeout (default 1 hour), **When** the timeout
   passes, **Then** the branch fails, like any optional step.
4. **Given** a POST to a callback URL with an unknown, expired or used token, **When** it
   arrives, **Then** it is refused, and nothing is saved.
5. **Given** the result's timestamps differ from the source, **When** it is checked, **Then**
   the timings are taken from the source cue by cue if the cue count matches. Otherwise the
   result is refused.

---

### User Story 3 - Choose per category and translate by hand (Priority: P3)

The operator turns translation off for a category. On the Local files page (044), a video's
**Translate subtitles** action runs the translation for a file that is already on disk.

**Acceptance Scenarios**:

1. **Given** translation is off for `radarr`, **When** a `radarr` item runs, **Then** the
   Translating stage is skipped with the reason "not for this category".
2. **Given** a video on disk with `x.en.srt`, **When** Translate is pressed for Spanish,
   **Then** `x.es.srt` is written, and the result is shown on the page.

### Edge Cases

- Formatting in cues (`<i>`, `{\an8}`, line breaks) is kept. The model is told to keep tags,
  and a cue that loses or adds a line break is accepted. Tags the source has but the result
  lacks are added back around the cue when they wrap the whole cue.
- Cues with no words (music notes `♪`, sound effects in brackets) are sent as they are. The
  model may translate the words inside brackets.
- Right-to-left target languages (Arabic, Hebrew): the text is saved as returned, in UTF-8.
- A model that adds commentary ("Here is the translation:"): the reply format is strict, so
  the commentary is stripped or the chunk is retried.
- Rate limits (HTTP 429): back off as the server's Retry-After says, and count it toward the
  step's time, not its retries.
- An API key, or a callback header value, never appears in logs or in step output.
- Two items translate at once: the 038 concurrency limit holds, plus a translator limit of
  requests in flight (default 2).
- The callback arrives after putio-sync restarted: the job is kept in the database, so it is
  still accepted while its token is valid.
- putio-sync is not reachable from the callback service, for example a hosted API calling a
  home NAS: the Test button says when the callback base URL isn't set, and the callback mode
  needs that URL.

## Requirements *(mandatory)*

### Functional Requirements

**Common**

- **FR-001**: Translation MUST run as the Translating stage of the 038 pipeline. It has one
  branch per wanted language that is missing, and the branches run in parallel. The step
  changes files.
- **FR-002**: Target languages MUST be 025's wanted languages. A language with a subtitle
  already present MUST be skipped.
- **FR-003**: The source MUST be picked in this order: the first wanted language present, then
  the transcribed subtitle, then any other.
- **FR-004**: Output MUST keep the source's cue count, numbering and timestamps exactly. A
  result that can't be made to match MUST NOT be saved.
- **FR-005**: The translated subtitle MUST be saved as `<video>.<lang>.srt` (UTF-8), next to
  the video, or in the 038 staging folder.
- **FR-006**: Translation steps MUST be optional in 038 terms: they are retried, then given up.
  One failed language MUST NOT stop the others.
- **FR-007**: The operator MUST be able to choose one translator kind: **chat model**
  (OpenAI-compatible) or **custom API (callback)**. The default is none, and then the stage is
  skipped as not configured.
- **FR-008**: Settings MUST have a **Test** button. It checks the chosen translator with a
  short sample and shows the result, or the server's error.

**Chat model**

- **FR-009**: The chat model translator MUST take a base URL, a model and an optional API key,
  and call the OpenAI-compatible chat completions API. That covers Ollama, LM Studio, Open
  WebUI and hosted APIs.
- **FR-010**: Subtitles MUST be sent in chunks: a configurable number of cues (default 40),
  with up to 3 earlier cues as context that is not translated again. Each reply MUST be checked
  for its line count and numbering.
- **FR-011**: A chunk that fails the check MUST be retried. It is split in half after the
  second failure, and given up after the step's retries.
- **FR-012**: The operator MUST be able to set the number of requests in flight (default 2), a
  timeout per request, and an optional extra instruction. For example: "informal Spanish
  (Spain)".

**Custom API (callback)**

- **FR-013**: The custom API translator MUST take a URL, optional headers and a callback
  timeout (default 1 hour). It POSTs JSON with these fields: `job_id`, `source_lang`,
  `target_lang`, `srt`, `video_name` and `callback_url`.
- **FR-014**: A `200` reply with SRT (as text or as `{ "srt": … }`) MUST be taken as the
  result. A `202` reply MUST put the branch in "waiting for callback".
- **FR-015**: putio-sync MUST accept the result at the `callback_url` it gave. The URL holds a
  random token that is good for one job and one use, and only until the timeout. Any other
  call MUST be refused, and the refusal logged. The callback route MUST be reachable without
  the API token, because the token in the URL is the only credential it needs.
- **FR-016**: The callback base URL (how the service reaches putio-sync) MUST be set in
  Settings. Callback mode MUST NOT be saved without it.
- **FR-017**: Jobs waiting for a callback MUST survive restarts.

**Observability**

- **FR-018**: Metrics MUST count translations by translator kind and result, chunks sent, chunk
  retries and translation seconds. Labels hold no file names. Language labels are limited to
  the configured languages.
- **FR-019**: Logs MUST record each translation's video, source and target language, cue count,
  chunks, retries and duration. They MUST NOT record API keys, header values or callback tokens.

### Key Entities

- **Translator settings**: the kind (none, chat or callback), plus that kind's fields, the
  number of requests in flight, timeouts, the extra instruction and the categories that are
  off.
- **Translation job**: one video and one target language. It holds the source subtitle, its
  state (running, waiting for callback, done or failed), the chunks done and, in callback mode,
  the token and its expiry.
- **Cue**: number, start, end and text. The number and times are never changed by translation.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every saved translation has the same cue count and identical timestamps as its
  source, in 100% of tested cases.
- **SC-002**: With a local 14B model on the user's hardware, a 1,000-cue subtitle is translated
  in under 10 minutes. That is a guide, not a gate, since it depends on the hardware.
- **SC-003**: Test reports a wrong URL, a wrong model or a wrong key within 15 seconds, with a
  message that names which one is wrong.
- **SC-004**: A failed or slow translation never holds an item back past the 038 retry policy,
  or past the callback timeout.
- **SC-005**: A callback URL can't be used twice, and can't be used for another job.

## Assumptions

- Ollama, LM Studio and Open WebUI all expose the OpenAI-compatible `/v1/chat/completions`
  route. Ollama's own `/api/chat` is not needed.
- The prompt asks for numbered lines back, one per cue. Multi-line cues are joined with a
  marker and split again afterwards. Planning fixes the format.
- The callback service gets the SRT text in the request. Large subtitles (about 100 KB) fit in
  one POST.
- A callback URL looks like `<callback base>/translate/callback/<token>`. The callback base is
  usually putio-sync's own LAN or tailnet address.
- No translation memory or glossary. Each subtitle is translated on its own.
- This feature translates SRT only. Other formats are turned into SRT by 025 first.
