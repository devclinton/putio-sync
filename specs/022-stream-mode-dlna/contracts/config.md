# Contract: Config

```toml
[download]
# "download" (default): files are downloaded to target_directory.
# "stream": files stay on Put.io; the media server's Put.io root shows them.
mode = "stream"
# Ask Put.io for an MP4 version of videos kept on Put.io that aren't MP4 (default true)
stream_mp4 = true

[[download.rules]]
path = "TV"
mode = "download"   # this folder (and below) is downloaded even though the account streams

[dlna]
enabled = true
roots = ["downloads", "putio"]
# putio_folder unset: the Put.io root shows the synced folder (the account root)
```

- Unknown `mode` values are a load error: `invalid download.mode 'x': expected download or stream`
  (and `download.rules <path>` for a rule).
- `dlna.putio_folder` is no longer required when `roots` has `putio`.
- Doctor/config check warnings:
  - `download.mode`: "stream mode keeps files on Put.io, but the media server is off" / "…does
    not share the Put.io root".
  - `download.rules <path>`: "stream mode on the Radarr/Sonarr category folder `<cat>`;
    Radarr/Sonarr items are downloaded anyway".
