// The plugin workbench (feature 055) without the DOM: docs anchors, line numbers, error
// lines, Run panel requests and run summaries. Loaded with settings/workbench.js only, so
// tests/ui/workbench.test.mjs runs it in node.

/** mdBook's id for a heading: `download.finished` → `downloadfinished` */
export const anchor = (heading) =>
  String(heading)
    .toLowerCase()
    .replace(/[^\w\- ]/g, "")
    .replace(/ /g, "-");

/** "1\n2\n3" for a text of three lines, for the gutter beside a code area */
export function lineNumbers(text) {
  const count = String(text).split("\n").length;
  return Array.from({ length: count }, (_, i) => i + 1).join("\n");
}

/** The file and line in an error such as `main.lua:3: boom` or `[string "main.lua"]:3:`;
 *  null when it names none */
export function errorLine(text) {
  const m = /([\w./-]+\.(?:lua|toml))"?\]?:(\d+)/.exec(String(text || ""));
  return m ? { file: m[1].split("/").pop(), line: Number(m[2]) } : null;
}

/** JSON typed in a box; empty is null. Throws with the box's name when it does not parse */
export function parseJson(text, what) {
  const t = String(text || "").trim();
  if (!t) return null;
  try {
    return JSON.parse(t);
  } catch (e) {
    throw new Error(`${what} is not JSON: ${e.message}`);
  }
}

/**
 * The `POST /plugins/workbench/run` body from the Run panel's values: `kind` is event, tick,
 * health, action or hook; `json` is the event data, action input or hook file as typed.
 * `unsaved` (`{manifest, script}`) runs the editor's text instead of the saved files.
 */
export function runBody({ name, kind, eventType, action, hook, json, real, status, body, unsaved }) {
  const call = { kind };
  if (kind === "event") Object.assign(call, { event_type: eventType, data: parseJson(json, "The event data") });
  else if (kind === "action") Object.assign(call, { action, input: parseJson(json, "The action input") });
  else if (kind === "hook") Object.assign(call, { hook, file: parseJson(json, "The file") || {} });
  const request = { name, call, real: !!real };
  if (!real) request.reply = { status: Number(status) || 200, body: String(body ?? "") };
  if (unsaved) Object.assign(request, unsaved);
  return request;
}

const size = (bytes) => (bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KiB` : `${(bytes / 1024 / 1024).toFixed(1)} MiB`);

/** One line for a run: what it called, how it ended, how long and how much memory */
export function runSummary(run) {
  const c = run.request.call;
  const what = c.kind === "event" ? c.event_type : c.kind === "action" ? `action ${c.action}` : c.kind === "hook" ? `${c.hook} hook` : c.kind;
  const ended = { success: "OK", failure: "failed", timeout: "timed out" };
  const how = run.problems.length ? "did not load" : !run.defined ? "has no such function" : ended[run.run.result] || run.run.result;
  const parts = [`${run.request.name} ${what}`, how, `${run.run.duration_ms} ms`];
  if (run.memory_bytes != null) parts.push(size(run.memory_bytes));
  parts.push(run.request.real ? "for real" : "dry run");
  if (run.unsaved) parts.push("unsaved text");
  return parts.join(" · ");
}

/** A request as one line: `POST https://host/path → 200 in 40 ms`, or why it was not sent */
export function requestLine(r) {
  const head = `${r.method} ${r.url}`;
  if (r.error) return `${head} → ${r.error}`;
  if (!r.sent) return `${head} → not sent (dry run)`;
  return `${head} → ${r.status ?? "?"}` + (r.duration_ms != null ? ` in ${r.duration_ms} ms` : "");
}

/** The store as text for the before and after boxes; "No store" without the permission */
export const storeText = (value) => (value == null ? "No store" : JSON.stringify(value, null, 2));

/** What a plugin may be in the workbench: edit (a folder Lua plugin with the editor on),
 *  copy (bundled, editor on), wasm (re-install), or view */
export function mode(source, editor) {
  if (source.runtime === "wasm") return "wasm";
  if (source.editable) return "edit";
  if (source.source === "bundled" && editor) return "copy";
  return "view";
}
