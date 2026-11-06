# Contract: OpenAI-compatible chat as used

## Request

`POST <endpoint>`; endpoint rule (research R2): path empty gives `<base>/v1/chat/completions`; path ending
`/chat/completions` is used as is; else `<base>/chat/completions`.

Headers: `Content-Type: application/json`; `Authorization: Bearer <api_key>` only when the key is set.

```json
{"model": "qwen2.5:14b", "stream": false, "temperature": 0,
 "messages": [
  {"role": "system", "content": "You translate subtitles from English (en) to Spanish (es). ..."},
  {"role": "user", "content": "Context, for reference only. Do not translate or output these lines:\n101| ...\n\nTranslate these lines:\n104| ...\n105| ...\n"}]}
```

The system prompt text is in research R3 and is built by `prompt::system(source, target, instruction)`.

## Line format

- Cue to line: `N| ` + cue text with each line break written as `<br>`. `N` is the cue's 1-based position in the whole file.
- Reply: one `N| translation` per requested number, same order. Commentary before the first line and after the last,
  `<think>` blocks and a code fence are dropped; a non-matching line between two lines continues the previous cue.
- Valid: exact set and order of numbers, no empty text (research R4).

Example: the request lines are `104| What are you doing<br>here?` and `105| <i>La la la</i>`; cue 106 is `♪` and has no
letters, so it is not sent and is copied as it is. The reply is `104| ¿Qué haces<br>aquí?` and `105| <i>La la la</i>`.

## Reply shape

```json
{"choices": [{"index": 0, "message": {"role": "assistant", "content": "104| ...\n105| ..."}, "finish_reason": "stop"}]}
```

Used: `choices[0].message.content`, `choices[0].finish_reason`. `finish_reason: "length"` is a failed attempt.

## Errors

| Status | Handling |
|---|---|
| connect error, DNS | failed attempt; Test: `problem: url` |
| `401`, `403` | fatal: key rejected; Test: `key` |
| `404`, message names the model | fatal: `model not found: <message>`; Test: `model` plus `models` from `GET <base>/models` |
| `404` otherwise | fatal: endpoint not found at the resolved URL; Test: `url` |
| `400` mentioning context, too long, maximum, token | failed attempt (split) |
| other `400` | fatal with the message |
| `429` | wait `Retry-After` (seconds), else 2/4/8..60 s, each at most 120 s; no attempt consumed; fail after 300 s total |
| `5xx`, timeout, reset | failed attempt |

Error message sources: `error.message`, `error` (string), else the first 300 characters of the body. The key is scrubbed.

## Compatibility notes (unconfirmed, checked by the Test button on the user's server)

- Ollama: `http://host:11434/v1`. Raise the context window (`num_ctx`) above the default or long chunks truncate.
- LM Studio: `http://host:1234/v1`; the model name is the loaded model's id.
- Open WebUI: its OpenAI-style route is `<host>/api/chat/completions`; use `http://host:3000/api` and an API key from
  Settings, Account.
- Hosted APIs: `https://api.openai.com/v1` and others with the same route.
