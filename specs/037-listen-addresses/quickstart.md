# Quickstart: Listen Addresses and Interfaces

Automated: `TMPDIR=<scratch> make ci` (unit and integration tests on loopback only).

Manual, with a free port P (not 15432 on the dev Mac):

1. Several addresses and IPv6: `[api] bind = ["127.0.0.1:P", "[::1]:P"]`, run
   `putio-sync daemon`; `curl http://127.0.0.1:P/health` and `curl -g http://[::1]:P/health`
   both answer; the log has one "API listening" line per address.
2. Interface: `bind = "lo0:P"` (macOS) / `"lo:P"` (Linux): `/health` answers on 127.0.0.1:P.
   `bind = "nope0:P"`: the log says it is waiting for `nope0`; nothing listens.
3. `server`: `putio-sync server --port P2` → `GET /tailscale` shows port P2;
   `putio-sync server` with no flags listens on the configured list.
4. UI: Settings → Network shows "Listening now". Change the API list to `192.0.2.1:P` and
   save: the confirm says this page will be cut off; cancel. Enter `nope0:P` only: the save
   is refused.
5. Metrics: `[metrics] bind = ["127.0.0.1:9743", "[::1]:9743"]`; both scrape.
6. Docker/snap defaults: no `bind` → `0.0.0.0:15432` and `0.0.0.0:9742` as before.

See [contracts/listen-api.md](contracts/listen-api.md) for response shapes.
