# Quickstart: Media Server Refresh

## Automated

```bash
cargo test -p putio-sync-plugins          # host additions, path rules, media plugins vs mocks
cargo test -p putio-sync-api --test plugins --test secrets_sweep
cargo test -p putio-sync-doctor plugins
node --test putio-sync-api/tests/ui/plugins.test.mjs
TMPDIR=<scratch> CARGO_PROFILE_RELEASE_STRIP=none make ci
```

Expected: tests in `putio-sync-plugins/tests/mediaserver.rs` show one Plex refresh per
folder for a 20-file season, one Jellyfin request with every due folder, a retry then a
drop on failure, the PIN flow saving the token, and no token in any log line, run error or
action answer.

## By hand (against a real Plex)

1. Settings → Plugins → plex → "Sign in with Plex"; open the link, approve. The token
   field shows "set" and the message names the account.
2. "Load" next to Server; pick the server. "Load" next to Libraries; tick TV.
3. If Plex sees the downloads under another path, add `"/downloads => /data/tv"`.
4. Enable, Save, approve hosts. Press Test: the run log shows reachable, token accepted,
   mapping ok.
5. `putio-sync doctor`: the Plugins item lists plex ok.
6. Download an episode; within about 25 s Plex shows it.
   `curl :9742/metrics | grep plugin_counter` shows `refreshes 1`.
