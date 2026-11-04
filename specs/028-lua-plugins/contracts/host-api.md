# Contract: Host API (version 1)

Shared by 028 (Lua) and, later, 029 (WebAssembly). Values are described in JSON terms:
a Lua table is a JSON object or array, `nil` is JSON null/absent. In Lua the API is the
global table `putio`; a WebAssembly host would offer the same functions as imports taking
and returning JSON.

| Call | Arguments | Returns | Permission |
|------|-----------|---------|------------|
| `log.debug/info/warn/error(message, fields?)` | string, object of scalars | — | none |
| `config` | — | object: the plugin's config and secret values, defaults applied | none |
| `http.request(options)` | `{method?, url, headers?, body?, timeout?}` | `{status, headers, body, truncated}` or `nil, error` | `http` |
| `store.get(key)` | string | value or `nil` | `store` |
| `store.set(key, value)` | string, JSON value | `true` or `nil, error` | `store` |
| `store.delete(key)` | string | — | `store` |
| `store.keys()` | — | array of strings | `store` |
| `time.now()` | — | number: Unix seconds with fractions | none |
| `time.iso(seconds?)` | number | string: RFC 3339 UTC | none |
| `json.encode(value)` / `json.decode(text)` | | string / value, or `nil, error` | none |
| `plugin.name`, `plugin.version` | | strings | none |
| `version` | | putio-sync's version | none |
| `api_version` | | integer `1` | none |

`http.request`:

- `method` default `GET`; `headers` an object of strings; `body` a string (use
  `json.encode`); `timeout` seconds, capped by the time left in the call.
- Refused (returns `nil, error`) when the scheme is not http(s) or the host is not approved:
  `"host not approved: example.com"`.
- Redirects followed up to 3 times, same host only. Response bodies over 1 MiB are cut and
  `truncated = true`. `status` is any HTTP status; a non-2xx is not an error.
- Errors never contain the URL path or query, only the host and what failed.

Limits per call: the plugin's `timeout_secs` (wall clock, host calls included) and
`memory_mb` (the whole Lua state). Absent: `os`, `io`, `debug`, `package`, `require`,
`dofile`, `loadfile`, `collectgarbage`, `string.dump`; `load` accepts text only; `print`
logs at info.

Additive changes (new calls, new fields) keep `api_version`; removing or changing a call
raises it.
