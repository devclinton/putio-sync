// Event hooks (feature 027): the rows of the Settings › Integrations › Event hooks editor to and from
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
  ["verify.passed", "Download verified"],
  ["verify.failed", "Verification failed"],
  ["verify.redownload", "Re-downloading after a mismatch"],
  ["pipeline.step_failed", "Pipeline step failed"],
  ["pipeline.completed", "Pipeline finished with warnings"],
  ["pipeline.finished", "Item ready (pipeline finished)"],
  ["putio.unreachable", "Put.io unreachable"],
  ["putio.recovered", "Put.io reachable again"],
  ["auth.expired", "Put.io token refused"],
  ["storage.recovered", "Put.io storage recovered"],
  ["local_disk.low", "Download disk low"],
  ["local_disk.recovered", "Download disk recovered"],
  ["arr.unreachable", "Radarr / Sonarr unreachable"],
  ["arr.recovered", "Radarr / Sonarr reachable again"],
  ["sync.started", "Sync run started"],
  ["download.started", "Download started"],
  ["download.progress", "Download progress (every 10 s)"],
  ["subtitles.found", "Subtitles found"],
  ["subtitles.missing", "Subtitles missing"],
  ["translation.finished", "Translation finished"],
  ["translation.failed", "Translation failed"],
  ["cleanup.deleted", "Deleted from Put.io after download"],
  ["daemon.started", "Daemon started"],
  ["daemon.stopping", "Daemon stopping"],
  ["config.changed", "Configuration changed"],
];

/** What a hook runs: a plugin only as a pipeline step (feature 057) */
export function kindChoices(row) {
  const kinds = [["command", "Command"], ["url", "Webhook URL"]];
  if (row.stage || row.kind === "plugin") kinds.push(["plugin", "Plugin"]);
  return kinds;
}

/** What a hook runs as: an event hook, or a step of a pipeline stage (feature 038) */
export const RUNS_AS = [
  ["", "Event hook"],
  ["pre_download", "Pre-download step"],
  ["pre_processing", "Pre-processing step"],
  ["post_processing", "Post-processing step"],
];

/** An editor row for a new hook */
export function blankHook(n = 1) {
  return { name: `hook-${n}`, enabled: true, kind: "command", program: "", args: "", url: "", plugin: "", pairs: [], all: false, events: ["download.finished"], timeout_secs: 30, stage: "", required: true, categories: "" };
}

/** What a hook runs, as `GET /hooks` would show it (the program, or the URL cut down) */
export function targetLabel(row) {
  if (row.kind === "plugin") return (row.plugin || "").trim() ? `plugin ${row.plugin.trim()}` : "";
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
    // Any part of the path or query can be the secret, so none of it is shown
    const hasPath = u.pathname.split("/").some(Boolean);
    return `${u.protocol}//${u.host}${hasPath || u.search ? "/…" : ""}`;
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
    kind: hook.plugin != null ? "plugin" : url ? "url" : "command",
    plugin: hook.plugin || "",
    program: command[0] || "",
    args: command.slice(1).join("\n"),
    url: hook.url || "",
    pairs: Object.entries(values).map(([key, value]) => ({ key, value, masked: value === MASK })),
    all: (hook.events || []).includes("*"),
    events: (hook.events || []).filter((e) => e !== "*"),
    timeout_secs: hook.timeout_secs ?? 30,
    stage: hook.stage || "",
    required: hook.required !== false,
    categories: (hook.categories || []).join(", "),
  };
}

/** An editor row as the `[[hooks]]` entry `PUT /settings/hooks` takes */
export function toSettings(row) {
  const out = { name: row.name.trim(), enabled: row.enabled, events: row.all ? ["*"] : [...row.events], timeout_secs: Number(row.timeout_secs) };
  if (row.stage) {
    // A step takes no events; it runs when an item reaches its stage
    out.events = [];
    out.stage = row.stage;
    out.required = row.required;
    out.categories = row.categories.split(",").map((c) => c.trim()).filter(Boolean);
  }
  const pairs = {};
  for (const p of row.pairs) if (p.key.trim()) pairs[p.key.trim()] = p.value;
  if (row.kind === "plugin") {
    // A plugin's on_item is the step (feature 057); it takes no headers or environment
    out.plugin = (row.plugin || "").trim();
  } else if (row.kind === "url") {
    out.url = row.url.trim();
    out.headers = pairs;
  } else {
    const args = row.args.split("\n").map((a) => a.trim()).filter(Boolean);
    out.command = [row.program.trim(), ...args];
    out.env = pairs;
  }
  return out;
}

/** The names of the command hooks in `next` (settings entries) whose program or arguments are
 *  new or differ from the same-named hook in `saved`. Saving these runs code as the daemon's
 *  user, so the page asks first and sends `confirm_exec`. */
export function changedCommands(saved, next) {
  const known = new Map((saved || []).filter((x) => x.command).map((x) => [x.name, JSON.stringify(x.command)]));
  return (next || []).filter((x) => x.command && known.get(x.name) !== JSON.stringify(x.command)).map((x) => x.name);
}

/** `rows` with the one at `i` moved by `delta` places (the order inside a stage is the list order); the same rows when it can't move. */
export function moveRow(rows, i, delta) {
  const j = i + delta;
  if (i < 0 || i >= rows.length || j < 0 || j >= rows.length) return rows;
  const out = [...rows];
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}

/** Position of a step among the hooks of its stage, as "2 of 3"; "" for an event hook. */
export function stagePosition(rows, i) {
  const stage = rows[i].stage;
  if (!stage) return "";
  const same = rows.filter((r) => r.stage === stage);
  return `${same.indexOf(rows[i]) + 1} of ${same.length}`;
}
