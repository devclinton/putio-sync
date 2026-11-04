// Event hooks (feature 027): the rows of the Settings › Event hooks editor to and from
// `[[hooks]]` entries, and run results for people. No DOM here, so
// tests/ui/hooks.test.mjs runs it in node.

export const MASK = "********";

/** Every event a hook can ask for; `hook.test` is sent only by the test button */
export const EVENTS = [
  ["download.finished", "Download finished"],
  ["download.failed", "Download failed"],
  ["transfer.added", "Transfer added"],
  ["transfer.finished", "Transfer finished"],
  ["transfer.failed", "Transfer failed"],
  ["sync.completed", "Sync run complete"],
  ["storage.low", "Put.io storage low"],
];

/** An editor row for a new hook */
export function blankHook(n = 1) {
  return { name: `hook-${n}`, enabled: true, kind: "command", program: "", args: "", url: "", pairs: [], all: false, events: ["download.finished"], timeout_secs: 30 };
}

/** What a hook runs, as `GET /hooks` would show it (the program, or the URL cut down) */
export function targetLabel(row) {
  if (row.kind === "url") {
    const text = row.url.trim();
    if (!text) return "";
    let u;
    try {
      u = new URL(text);
    } catch {
      return "<invalid url>";
    }
    if (!/^https?:$/.test(u.protocol)) return "<invalid url>";
    const [first, ...more] = u.pathname.split("/").filter(Boolean);
    let out = `${u.protocol}//${u.host}${first ? `/${first}` : ""}`;
    if (more.length) out += "/…";
    else if (u.search) out += "…";
    return out;
  }
  return row.program.trim();
}

/** The event checkboxes: each type with whether this row asks for it */
export function eventChoices(row) {
  return EVENTS.map(([value, label]) => ({ value, label, checked: row.all || row.events.includes(value) }));
}

const seconds = (ms) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`);

/** One run for people: "200 OK in 40 ms", "exit 3 in 1.2 s", "timed out after 30 s" */
export function runLabel(run) {
  if (!run) return "Not run yet";
  if (run.result === "timeout") return run.error || "timed out";
  const took = ` in ${seconds(run.duration_ms)}`;
  if (run.http_status != null) {
    const status = run.result === "success" ? `${run.http_status} OK` : `HTTP ${run.http_status}`;
    return status + took + (run.attempts > 1 ? ` (${run.attempts} tries)` : "");
  }
  if (run.exit_code != null) return (run.exit_code === 0 ? "exit 0" : `exit ${run.exit_code}`) + took;
  return (run.error || run.result) + took;
}

/** A `[[hooks]]` entry from `GET /settings/hooks` as an editor row */
export function fromSettings(hook) {
  const url = hook.url != null;
  const command = hook.command || [];
  const values = (url ? hook.headers : hook.env) || {};
  return {
    name: hook.name,
    enabled: hook.enabled !== false,
    kind: url ? "url" : "command",
    program: command[0] || "",
    args: command.slice(1).join("\n"),
    url: hook.url || "",
    pairs: Object.entries(values).map(([key, value]) => ({ key, value, masked: value === MASK })),
    all: (hook.events || []).includes("*"),
    events: (hook.events || []).filter((e) => e !== "*"),
    timeout_secs: hook.timeout_secs ?? 30,
  };
}

/** An editor row as the `[[hooks]]` entry `PUT /settings/hooks` takes */
export function toSettings(row) {
  const out = { name: row.name.trim(), enabled: row.enabled, events: row.all ? ["*"] : [...row.events], timeout_secs: Number(row.timeout_secs) };
  const pairs = {};
  for (const p of row.pairs) if (p.key.trim()) pairs[p.key.trim()] = p.value;
  if (row.kind === "url") {
    out.url = row.url.trim();
    out.headers = pairs;
  } else {
    const args = row.args.split("\n").map((a) => a.trim()).filter(Boolean);
    out.command = [row.program.trim(), ...args];
    out.env = pairs;
  }
  return out;
}
