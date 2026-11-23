// Anonymous usage stats receiver for putio-sync (feature 058). A Cloudflare Worker.
//
//   POST /        one payload (schema 1), checked against an allow-list in validate.js
//   GET  /summary daily aggregate counts: ?days=14 (1 to 90). Off unless PUBLIC_SUMMARY=1.
//
// Privacy: the request's IP address and headers are never read, logged or stored. Only the
// counts are kept, one KV entry per day. To count an install once a day, a SHA-256 of
// "day:install id" is kept for 2 days, and the install id itself is not.
// Not deployed by the project: see README.md.

import { MAX_BODY, readLimited, tally, validate } from "./validate.js";

const JSON_HEADERS = { "content-type": "application/json", "cache-control": "no-store" };
const DAY_TTL = 60 * 60 * 24 * 400;
const SEEN_TTL = 60 * 60 * 48;

const reply = (status, body) => new Response(JSON.stringify(body), { status, headers: JSON_HEADERS });
const today = () => new Date().toISOString().slice(0, 10);

async function sha256(text) {
  const bytes = new TextEncoder().encode(text);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function receive(request, env) {
  const type = request.headers.get("content-type") || "";
  if (!type.startsWith("application/json")) return reply(415, { error: "send JSON" });
  const length = request.headers.get("content-length");
  if (length === null) return reply(411, { error: "length required" });
  if (!/^\d+$/.test(length)) return reply(400, { error: "bad content-length" });
  if (Number(length) > MAX_BODY) return reply(413, { error: "too large" });
  // The header can lie, so the bytes are counted as they are read too.
  const text = await readLimited(request, MAX_BODY);
  if (text === null) return reply(413, { error: "too large" });
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return reply(400, { error: "not JSON" });
  }
  const checked = validate(body);
  if (!checked.ok) return reply(400, { error: checked.error });

  const day = today();
  const seen = `seen:${await sha256(`${day}:${checked.id}`)}`;
  if (await env.STATS.get(seen)) return reply(200, { counted: false });
  await env.STATS.put(seen, "1", { expirationTtl: SEEN_TTL });
  const key = `day:${day}`;
  const before = (await env.STATS.get(key, "json")) || {};
  await env.STATS.put(key, JSON.stringify(tally(before, checked.counts)), { expirationTtl: DAY_TTL });
  return reply(200, { counted: true });
}

async function summary(url, env) {
  const asked = Number.parseInt(url.searchParams.get("days") || "14", 10);
  const days = Math.min(90, Math.max(1, Number.isFinite(asked) ? asked : 14));
  const out = {};
  const now = Date.now();
  const dates = Array.from({ length: days }, (_, i) => new Date(now - i * 86400000).toISOString().slice(0, 10));
  const rows = await Promise.all(dates.map((d) => env.STATS.get(`day:${d}`, "json")));
  dates.forEach((d, i) => {
    if (rows[i]) out[d] = rows[i];
  });
  return reply(200, { days: out });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "POST" && url.pathname === "/") return receive(request, env);
    if (request.method === "GET" && url.pathname === "/summary" && env.PUBLIC_SUMMARY === "1") {
      const res = await summary(url, env);
      res.headers.set("access-control-allow-origin", "*");
      return res;
    }
    return reply(404, { error: "not found" });
  },
};
