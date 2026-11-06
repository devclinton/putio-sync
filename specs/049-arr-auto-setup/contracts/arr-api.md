# Radarr/Sonarr calls (049)

All v3, `X-Api-Key` header, as in 018.

| Call | Use |
|---|---|
| `GET /api/v3/rootfolder` | `[{path}]`: search seeds (R3) |
| `GET /api/v3/filesystem?path=P&allowFoldersWithoutTrailingSlashes=true` | `{directories: [{name, path}]}`: search and marker check |
| `GET /api/v3/remotepathmapping` | `[{id, host, remotePath, localPath}]` (018, now with id) |
| `POST /api/v3/remotepathmapping` | `{host, remotePath, localPath}` → 201 with id; 400 with validation messages |
| `PUT /api/v3/remotepathmapping/{id}` | `{id, host, remotePath, localPath}` |
| `DELETE /api/v3/remotepathmapping/{id}` | remove |
| `GET /api/v3/config/downloadclient` | `{id, enableCompletedDownloadHandling?, …}` |
| `PUT /api/v3/config/downloadclient/{id}` | whole body back, one key changed |
| `GET /api/v3/config/mediamanagement` | `{id, importExtraFiles?, extraFileExtensions?, …}` |
| `PUT /api/v3/config/mediamanagement/{id}` | whole body back |
