# Quickstart: Local Files Page

## Automated

```bash
cargo test -p putio-sync-file-manager local_files
cargo test -p putio-sync-api --test local_files --test ui_assets --test secrets_sweep
cargo clippy --all-targets --all-features -- -D warnings
```

## By hand (local daemon)

1. Point `[download] target_directory` at a temp folder, turn on `[watch]` with one temp path, and
   start `putio-sync server`.
2. In the download folder create `sonarr/`, `sonarr/.Ep.mkv.downloading`, and a symlink
   `escape -> /`. In the watch folder create `x.magnet.rejected`.
3. Open the web UI → Local files. Downloads and the watch folder show; the partial is marked
   "Partial", the rejected file "Rejected", `escape` as a link.
4. Open `escape`: refused, it is not followed. Delete `escape`: the link goes, `/` is untouched.
5. Re-check `x.magnet.rejected`: it becomes `x.magnet`.
6. Delete `sonarr/`: the confirmation shows its size; after confirming it is gone.
7. `curl -H "Authorization: Bearer $T" "localhost:15432/local/files/downloads?path=../"` → 400.

## On the NAS (after deploy, not part of this change)

Check `/downloads/sonarr`, `/downloads/radarr`, `/watches/radarr` and `/watches/sonarr` show.
