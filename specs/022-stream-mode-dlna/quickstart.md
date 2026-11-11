# Quickstart: Stream Mode with DLNA

## Automated

```bash
cargo test -p putio-sync-core rules config_lint
cargo test -p putio-sync-download --test stream_mode
cargo test -p putio-sync-db stream_files
cargo test -p putio-sync-api --test ui_js
```

Expected: stream decisions per account and per rule; a pass queues nothing in stream mode,
records the files, asks for one MP4 version for the `.mkv` only, and asks nothing on the second
pass; the UI helper shows the warning only when stream is used without the Put.io root.

## By hand (NAS)

1. Settings › Downloads › Mode: **Stream from Put.io**. Save.
2. The warning "Players can't see files kept on Put.io" appears; press **Turn on the media
   server**. Settings › Media server now shows Running with Downloads and Put.io shared.
3. Add a transfer on Put.io. After it finishes and the next sync runs, nothing appears in the
   Queue; the log has `Kept on Put.io for streaming` with the file id.
4. On the TV (or VLC › Local network › UPnP), open putio-sync › Put.io: the file is there and
   plays. For an `.mkv`, Put.io's file page shows the MP4 conversion queued.
5. Set the mode back to **Download to the NAS**: the next sync queues the file.
