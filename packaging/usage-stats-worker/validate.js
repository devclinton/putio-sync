// Validates one usage stats payload (schema 1) against an allow-list, and turns it into the
// counts the worker keeps. Plain JS with no Worker APIs, so `node --test` can run it.
// Anything not on the lists is rejected, never stored.

export const SCHEMA = 1;
/** Largest body accepted, in bytes */
export const MAX_BODY = 4096;

const BUILDS = ["snap", "flatpak", "unraid", "truenas", "casaos", "umbrel", "container", "homebrew", "deb", "winget", "pkg", "setup", "archive", "unknown"];
const OSES = ["linux", "macos", "windows", "freebsd", "netbsd", "openbsd", "other"];
const ARCHES = ["x86_64", "aarch64", "arm", "x86", "riscv64", "powerpc64", "s390x", "other"];
const PLUGINS = ["discord", "emby", "jellyfin", "ntfy", "plex", "pushover", "telegram"];
const FILES = ["0", "1-10", "11-100", "101-1000", "1000+"];
const LIBRARY = ["<100GB", "100GB-1TB", "1-10TB", "10TB+"];
const FLAGS = ["arr", "watch_folders", "event_hooks", "dlna", "tailscale", "metrics", "subtitles", "crash_reports"];
const STAGES = ["pre_download", "pre_processing", "post_processing"];
const TOP = ["schema", "install_id", "version", "build", "os", "arch", "features", "sizes"];

const UUID4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const VERSION = /^\d{1,3}\.\d{1,3}\.\d{1,3}(-[0-9A-Za-z]([0-9A-Za-z.]{0,14}[0-9A-Za-z])?)?$/;
/** Most distinct values kept per field in one day's record; the rest count as `field:other` */
export const MAX_VALUES = 200;

const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
const sameKeys = (obj, keys) => Object.keys(obj).length === keys.length && keys.every((k) => k in obj);

/**
 * `{ ok: true, id, counts }` for a valid payload, `{ ok: false, error }` otherwise.
 * `counts` is a list of `[field, value]` pairs to add one to; `id` is the install id, used only
 * to count an install once a day.
 */
export function validate(p) {
  const bad = (error) => ({ ok: false, error });
  if (!isObject(p)) return bad("not an object");
  if (!sameKeys(p, TOP)) return bad("unexpected or missing keys");
  if (p.schema !== SCHEMA) return bad("unsupported schema");
  if (typeof p.install_id !== "string" || !UUID4.test(p.install_id)) return bad("install_id");
  if (typeof p.version !== "string" || !VERSION.test(p.version)) return bad("version");
  if (!BUILDS.includes(p.build)) return bad("build");
  if (!OSES.includes(p.os)) return bad("os");
  if (!ARCHES.includes(p.arch)) return bad("arch");

  const f = p.features;
  if (!isObject(f) || !sameKeys(f, [...FLAGS, "pipeline_stages", "plugins"])) return bad("features");
  if (!FLAGS.every((k) => typeof f[k] === "boolean")) return bad("features");
  const s = f.pipeline_stages;
  if (!isObject(s) || !sameKeys(s, STAGES) || !STAGES.every((k) => typeof s[k] === "boolean")) return bad("pipeline_stages");
  if (!Array.isArray(f.plugins) || f.plugins.length > PLUGINS.length || new Set(f.plugins).size !== f.plugins.length || !f.plugins.every((n) => PLUGINS.includes(n))) return bad("plugins");

  const z = p.sizes;
  if (!isObject(z) || !sameKeys(z, ["files_per_day", "library"])) return bad("sizes");
  if (!FILES.includes(z.files_per_day) || !LIBRARY.includes(z.library)) return bad("sizes");

  const counts = [
    ["installs", "all"],
    ["version", p.version],
    ["build", p.build],
    ["os", p.os],
    ["arch", p.arch],
    ["files_per_day", z.files_per_day],
    ["library", z.library],
    ...FLAGS.filter((k) => f[k]).map((k) => ["feature", k]),
    ...STAGES.filter((k) => s[k]).map((k) => ["stage", k]),
    ...f.plugins.map((n) => ["plugin", n]),
  ];
  return { ok: true, id: p.install_id, counts };
}

/**
 * Add one to each `[field, value]` in a day's tally (`{ "field:value": n }`). At most `cap`
 * distinct values are kept per field; a new value past that is folded into `field:other`, so a
 * flood of made-up values cannot grow the record without bound.
 */
export function tally(day, counts, cap = MAX_VALUES) {
  const out = { ...day };
  const distinct = {};
  for (const key of Object.keys(out)) {
    const field = key.slice(0, key.indexOf(":"));
    distinct[field] = (distinct[field] || 0) + 1;
  }
  for (const [field, value] of counts) {
    let key = `${field}:${value}`;
    if (!(key in out)) {
      if ((distinct[field] || 0) >= cap) key = `${field}:other`;
      if (!(key in out)) distinct[field] = (distinct[field] || 0) + 1;
    }
    out[key] = (out[key] || 0) + 1;
  }
  return out;
}

/**
 * Read a request body as text, counting bytes, and stop as soon as it passes `limit`.
 * Returns the text, or `null` when the body is too large.
 */
export async function readLimited(request, limit = MAX_BODY) {
  if (!request.body) return "";
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel().catch(() => {});
      return null;
    }
    chunks.push(value);
  }
  const all = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) {
    all.set(c, at);
    at += c.byteLength;
  }
  return new TextDecoder().decode(all);
}
