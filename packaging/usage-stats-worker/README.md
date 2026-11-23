# putio-sync usage stats receiver

A minimal Cloudflare Worker that receives putio-sync's opt-in anonymous usage stats (feature
058, part 2) and keeps daily counts. **It is not deployed by the project.** putio-sync sends
nothing until someone turns usage stats on and sets `[usage_stats] endpoint` to a receiver.

## What it accepts

`POST /` with `Content-Type: application/json`, at most 4 KB (4096 bytes; a request with no
`Content-Length` gets 411, one over the limit gets 413, and the body is byte-counted as it is read), one payload of `"schema": 1`.
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

Each field keeps at most 200 distinct values a day; further new values count as `field:other`,
so a flood of made-up versions cannot grow the day's record toward KV's 25 MiB value limit.

## Limits

- **Counts are approximate.** KV has no compare-and-swap, and a key takes about one write per
  second. Payloads arriving together can lose a count, and a burst can be rejected by KV. That is
  fine for rough numbers; use D1 or a Durable Object if exact counts matter.
- **Free-plan write quota.** Each counted POST makes 2 KV writes (the seen marker and the day
  record), and the free plan allows about 1000 writes a day, so roughly 500 installs a day.
- **Rate limiting.** Add Cloudflare rate limiting or WAF rules in front of the Worker. The Worker
  does not limit requests itself, and anyone can POST valid-looking payloads with fresh ids.

## Deploy

```sh
cd packaging/usage-stats-worker
cp wrangler.toml.example wrangler.toml
npx wrangler kv namespace create STATS     # copy the id into wrangler.toml
npx wrangler deploy
```

Then set `[usage_stats] endpoint = "https://<your-worker>.workers.dev/"` in putio-sync's
config. `GET /summary?days=14` (1 to 90) returns the daily counts as JSON. It is **off by
default** (404) because it is public with open CORS: set `PUBLIC_SUMMARY = "1"` under `[vars]`
in `wrangler.toml` only if you are happy to show the counts to anyone, or put an access rule in
front of it.

## Test

`putio-sync-api/tests/ui/usage_worker.test.mjs` runs the validator under `node --test`
(it runs with `cargo test -p putio-sync-api`).
