# Quickstart: Event Hooks

1. `make ci` passes (unit and API tests cover every scenario below with mocks).
2. Command hook by hand: write `/tmp/hook.sh` (`#!/bin/sh` / `cat > /tmp/last-event.json;
   env | sort > /tmp/last-env`), `chmod +x`, add
   `[[hooks]] name="t" events=["*"] command=["/tmp/hook.sh"]`, start
   `putio-sync server`, then `curl -X POST -H "Authorization: Bearer $TOKEN"
   -d '{"name":"t"}' localhost:15432/hooks/test`. Expect `result: success`,
   `/tmp/last-event.json` of type `hook.test`, and `/tmp/last-env` without any
   `PUTIO_SYNC_TOKEN`/`PUTIO_*` token variables.
3. Webhook: `python3 -m http.server`-style listener or `nc -l 9999`; add a URL hook with a
   header and `?token=abc` in the URL; send a test event; the listener sees the header;
   the daemon log and `GET /hooks` show the URL without `?token=abc` and no header value.
4. Settings › Hooks: add, save, Send test event; the result shows beside the hook.
5. `GET /metrics` has `putio_sync_hook_runs_total{hook="t",event="hook.test",result="success"} 1`.
6. `putio-sync doctor`: Hooks check passes; rename `/tmp/hook.sh` and it reports a problem.
