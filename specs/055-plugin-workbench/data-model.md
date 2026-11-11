# Data Model: Plugin Workbench

## Config

- `plugin_editor: bool`, top level, default `false`, left out of `config.toml` when false.

## Starter (built in)

| Field | Notes |
|---|---|
| `id` | `webhook-notifier`, `store-counter`, `tick-poller` |
| `title`, `description` | shown in **New plugin** |
| `manifest`, `script` | `plugin.toml` and `main.lua`; `name = "..."` is replaced with the new name |

## Source (one plugin's files)

| Field | Notes |
|---|---|
| `name` | plugin name |
| `runtime` | `lua` or `wasm` |
| `source` | `bundled` or `folder` |
| `editable` | folder Lua plugin and the editor on |
| `files[]` | `{path, text, version}` for `plugin.toml` and the entry; WebAssembly: `plugin.toml` text only (from the folder or the module) |
| `other_files[]` | other names in the folder, not editable |

`version` is `<length>-<modified ms>`; Save compares it.

## Problem

`{file, line?, message}`: a manifest error (`line` absent, message as a reload gives it) or a Lua
syntax error with its line.

## Call

One of `{kind: "event", event_type, data}`, `{kind: "tick"}`, `{kind: "health"}`,
`{kind: "action", action, input}`, `{kind: "hook", hook, file}`. `file` is a `HookFile`
(`id, name, size, extension, folder, decision, reason`).

## Run request

`{name, call, real: bool, reply?: {status, body}, manifest?, script?}`. `manifest`/`script` are
unsaved editor text (Lua only). Payloads and bodies are cut at 1 MiB.

## Run (result)

| Field | Notes |
|---|---|
| `id` | increasing number, for re-runs |
| `request` | the request, as sent |
| `unsaved` | ran editor text |
| `run` | the usual `PluginRun`: result, error, duration, log lines (up to 200) |
| `value` | what the function returned, JSON (`null` for nothing) |
| `answer` | sync hooks: what the sync makes of `value`, or why it is ignored |
| `defined` | false when the plugin has no such function |
| `memory_bytes` | in use at the end |
| `requests[]` | `{method, url, headers, body, sent, status?, duration_ms?, error?}`, secrets masked |
| `writes[]` | dry runs: `{path, bytes}` |
| `store_before`, `store_after` | objects, secrets masked; `null` without the store permission |
| `problems[]` | manifest or syntax problems that kept the plugin from loading |

History: the last 20 runs, in memory, newest first.
