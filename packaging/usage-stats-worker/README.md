# putio-sync usage stats receiver

A minimal Cloudflare Worker that receives putio-sync's opt-in anonymous usage stats (feature
058, part 2) and keeps daily counts. **It is not deployed by the project.** putio-sync sends
nothing until someone turns usage stats on and sets `[usage_stats] endpoint` to a receiver.

## What it accepts

`POST /` with `Content-Type: application/json`, at most 4 KB, one payload of `"schema": 1`.
`validate.js` checks every field against an allow-list (install kinds, OS, architectures,
bundled plugin names, size buckets, a UUID v4 install id, a version) and refuses anything with
an extra or missing key. A user plugin name, path, host or any other text cannot get in.

## What it keeps

- One KV entry per day, `day:YYYY-MM-DD`, holding counts keyed by field and value, e.g.
  `os:linux`, `feature:dlna`, `plugin:ntfy`, `library:1-10TB`. Entries expire after 400 days.
- To count an install once a day, a SHA-256 of `day:install id` for 2 days. The install id itself
  is not stored.
- The request's IP address and headers are never read, logged or stored. `[observability]` is
  off in `wrangler.toml.example`; keep Workers Logs and Logpush off too.

KV has no atomic increment, so two payloads arriving at the same moment can lose one count.
That is fine for rough numbers; use D1 or a Durable Object if exact counts matter.

## Deploy

```sh
cd packaging/usage-stats-worker
cp wrangler.toml.example wrangler.toml
npx wrangler kv namespace create STATS     # copy the id into wrangler.toml
npx wrangler deploy
```

Then set `[usage_stats] endpoint = "https://<your-worker>.workers.dev/"` in putio-sync's
config. `GET /summary?days=14` (1 to 90) returns the daily counts as JSON and is public, so
publish only what you are happy to show. Put an access rule in front of it if not.

## Test

`putio-sync-api/tests/ui/usage_worker.test.mjs` runs the validator under `node --test`
(it runs with `cargo test -p putio-sync-api`).
