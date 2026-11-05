# Quickstart: try 049

## Automated

```bash
cargo test -p putio-sync-arr path_mapping recommend
cargo test -p putio-sync-api --test arr_settings --test secrets_sweep
make ci
```

## By hand with Docker (optional)

1. Run putio-sync with `target_directory = "/downloads"` and a volume `~/dl:/downloads`.
2. Run Sonarr with the same host folder at another path: `~/dl:/data/downloads`, on the same
   Docker network, container name `sonarr`.
3. In putio-sync, Settings › Integrations › Radarr / Sonarr › Add Sonarr (host `sonarr`,
   API key), then **Set up** on its row.
4. Expect: "Download client created", "Category sonarr", "Sonarr sees the folder at
   /data/downloads", and a proposal "Map /downloads/ → /data/downloads/ for host putio-sync".
5. Click **Create mapping**, confirm. Expect "Mapping created" then "Folder visible to Sonarr".
   Sonarr › Settings › Download Clients › Remote Path Mappings shows one mapping.
6. Click **Set up** again: every step reads already fine; still one mapping, one client.
7. Edit the mapping in Sonarr to a wrong local path; Set up proposes "Change mapping"; confirm;
   still one mapping, now right.
8. With Import Extra Files off in Sonarr, the step shows a recommendation; Turn on sets it with
   `srt` added.
9. `ls ~/dl` shows no `putio-sync-check-*` folder.
