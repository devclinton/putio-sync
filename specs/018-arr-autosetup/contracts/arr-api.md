# Contract: Radarr/Sonarr API calls putio-sync makes

These are the calls `putio-sync-arr::ArrApi` makes, all v3, with the API key in the `X-Api-Key`
header and a 10 s timeout. The exceptions are discovery's 3 s probes and the 5 s host tests.
Tests mock each call with mockito.

| Call | Key | Used for | Notes |
|---|---|---|---|
| `GET {base}/ping` | no | discovery | `{"status":"OK"}` |
| `GET {base}/` | no | discovery | HTML `<title>` is `Radarr` or `Sonarr`. `initialize.json` is never read, because it carries the key |
| `GET /api/v3/system/status` | yes | test, setup | `appName`, `version`, `instanceName`. 401 → `unauthorized`. Version major < 3 → `unsupported_version` |
| `GET /api/v3/downloadclient` | yes | categories, setup | list of client resources. `fields[]` are `{name, value}`. The password is masked `********` |
| `GET /api/v3/downloadclient/schema` | yes | setup | template for the `Transmission` implementation (default fields) |
| `POST /api/v3/downloadclient/test` | yes | host choice, already-set-up check | 200 = pass. 400 = `[{propertyName, errorMessage}]` → `arr_test_failed` / `arr_cannot_reach` |
| `POST /api/v3/downloadclient` | yes | create | the *arr re-tests it. 400 → `arr_refused` |
| `PUT /api/v3/downloadclient/{id}` | yes | update, restore, disable blackhole | restore and blackhole use `?forceSave=true` |
| `DELETE /api/v3/downloadclient/{id}` | yes | undo a create (R7) | rare |
| `GET /api/v3/remotepathmapping` | yes | path check | `[{host, remotePath, localPath}]` |
| `GET /api/v3/filesystem?path=…&allowFoldersWithoutTrailingSlashes=true` | yes | path check | `directories[].name` of the parent |
| `GET /api/v3/queue/details`, `DELETE /api/v3/queue/{id}?blocklist=true` | yes | blocklisting (feature 011) | unchanged |

## Client body putio-sync sends

The body starts from the schema's `Transmission` entry, with these values set:

```json
{
  "name": "putio-sync",
  "implementation": "Transmission",
  "configContract": "TransmissionSettings",
  "protocol": "torrent",
  "enable": true,
  "priority": 1,
  "fields": [
    {"name": "host", "value": "<chosen host>"},
    {"name": "port", "value": 15432},
    {"name": "useSsl", "value": false},
    {"name": "urlBase", "value": "/transmission/"},
    {"name": "username", "value": "putio-sync"},
    {"name": "password", "value": "<download-client password>"},
    {"name": "movieCategory", "value": "radarr"}
  ]
}
```

- Sonarr uses `tvCategory` in place of `movieCategory`.
- `priority` 1 is the highest the *arr allows (its range is 1–50).
- Every other field keeps the schema default.
- For an update, the body keeps the existing `id`, `tags` and any fields putio-sync doesn't set.
