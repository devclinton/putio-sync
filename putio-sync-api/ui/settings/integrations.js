// Integrations section (feature 046): Radarr / Sonarr (arr.js) and Event hooks; plugins
// have their own section since 048 (plugins.js). Feature 027: an editor per hook and a
// Send test event button; header and env values are shown only as set, with a Replace button.

import { get, post } from "../api.js";
import { RUNS_AS, blankHook, eventChoices, fromSettings, moveRow, runLabel, stagePosition, targetLabel, toSettings } from "../hooks.js";
import { docs, h, toast } from "../app.js";
import { relative } from "../format.js";
import { arrCard } from "./arr.js";
import { groupCard, reason } from "./form.js";

const HOOKS = { id: "hooks", key: "hooks", title: "Event hooks", fields: [], extra: hooksExtra };

// Feature 038: how the hooks that are pipeline steps run
const PIPELINE = {
  id: "pipeline",
  key: "pipeline",
  title: "Processing pipeline",
  fields: [
    ["concurrency", "Items at once", "How many items run their steps at the same time; 1 to 16", "n", 1, 16],
    ["optional_retries", "Optional step retries (seconds)", "One per line: the wait before each retry of a failing optional step. Up to 10 lines; the step is given up after the last", "ln"],
    ["staging", "Watch-folder staging", "Keep a watch-folder download out of sight until its steps are done", [["auto", "Auto", "On while a step could change files"], ["on", "On", "Always stage watch-folder downloads"], ["off", "Off", "Never stage"]]],
    ["history_days", "Keep step history (days)", "How long History shows each finished item's steps; their output is cleared after a day. 1 to 3650", "n", 1, 3650],
  ],
};

export const cards = (ctx) => [arrCard(ctx), groupCard(HOOKS, ctx), groupCard(PIPELINE, ctx)];

/**
 * One editor per hook: name, on/off, command or URL, headers or env, events, timeout,
 * and for a saved hook its last run and a Send test event button
 */
function hooksExtra(data) {
  let rows = (data.hooks || []).map(fromSettings);
  const saved = new Set(rows.map((r) => r.name));
  const status = new Map();
  const list = h("div", { class: "stack" });
  const none = h("p", { class: "muted" }, "No hooks. A hook runs a command or calls a webhook with each event as JSON. ", docs("events", "", "Event reference"));
  const changed = () => list.closest("form")?.dispatchEvent(new Event("change"));
  const redraw = () => {
    draw();
    changed();
  };
  const field = (label, input, hint) => h("label", { class: "field" }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);
  const text = (row, key, attrs = {}) =>
    h("input", { class: "input", value: row[key], autocomplete: "off", spellcheck: "false", oninput: (ev) => (row[key] = ev.currentTarget.value), ...attrs });

  const pairsTable = (row) => {
    const what = row.kind === "url" ? "header" : "variable";
    const body = h(
      "tbody",
      {},
      row.pairs.map((p, i) =>
        h(
          "tr",
          {},
          h("td", { "data-label": "Name" }, text(p, "key", { "aria-label": `${what} name`, placeholder: row.kind === "url" ? "Authorization" : "LIBRARY" })),
          h(
            "td",
            { "data-label": "Value" },
            p.masked
              ? h("div", { class: "toolbar" }, h("span", { class: "muted", text: "set" }), h("button", { type: "button", class: "btn btn-small", onclick: () => ((p.masked = false), (p.value = ""), redraw()) }, "Replace"))
              : text(p, "value", { "aria-label": `${what} value`, type: row.kind === "url" ? "password" : "text" }),
          ),
          h("td", { class: "actions-cell" }, h("button", { type: "button", class: "btn btn-small", "aria-label": `Remove ${what} ${i + 1}`, onclick: () => (row.pairs.splice(i, 1), redraw()) }, "Remove")),
        ),
      ),
    );
    return h(
      "div",
      { class: "spaced" },
      h("h3", { class: "sub", text: row.kind === "url" ? "Headers" : "Environment" }),
      row.pairs.length ? h("table", { class: "table override-table" }, h("thead", {}, h("tr", {}, ["Name", "Value", ""].map((t) => h("th", { text: t })))), body) : null,
      h("button", { type: "button", class: "btn btn-small", onclick: () => (row.pairs.push({ key: "", value: "", masked: false }), redraw()) }, row.kind === "url" ? "Add header" : "Add variable"),
    );
  };

  const events = (row) => {
    const boxes = eventChoices(row).map((c) =>
      h(
        "label",
        { class: "field check" },
        h("input", {
          type: "checkbox",
          checked: c.checked,
          disabled: row.all,
          onchange: (ev) => {
            row.events = ev.currentTarget.checked ? [...row.events, c.value] : row.events.filter((e) => e !== c.value);
          },
        }),
        h("span", { text: c.label }),
      ),
    );
    const all = h("label", { class: "field check" }, h("input", { type: "checkbox", checked: row.all, onchange: (ev) => ((row.all = ev.currentTarget.checked), redraw()) }), h("span", { text: "All events" }));
    return h("fieldset", { class: "hook-events" }, h("legend", { text: "Events" }), all, ...boxes);
  };

  const testBox = (row) => {
    if (!saved.has(row.name)) return h("p", { class: "muted", text: "Save to send a test event." });
    const out = h("span", { class: "muted", "aria-live": "polite" });
    const show = (run) => {
      out.textContent = run ? `Last run: ${run.event_type}, ${runLabel(run)}, ${relative(run.started_at)}` : "Not run yet";
      out.className = run && run.result !== "success" ? "error-text" : "muted";
      out.title = run && run.error ? run.error : "";
    };
    show((status.get(row.name)?.runs || [])[0]);
    const send = async (ev) => {
      const button = ev.currentTarget;
      button.disabled = true;
      out.textContent = "Sending…";
      try {
        const run = await post("/hooks/test", { name: row.name });
        show(run);
        toast(`Test event to ${row.name}: ${runLabel(run)}`);
      } catch (e) {
        if (e.status !== 401 && e.status !== 429) {
          out.textContent = reason(e);
          out.className = "error-text";
        }
      }
      if (button.isConnected) button.disabled = false;
    };
    return h("div", { class: "toolbar spaced" }, h("button", { type: "button", class: "btn btn-small", onclick: send }, "Send test event"), out);
  };

  // Feature 038: a hook can be a step of a pipeline stage instead of an event hook
  const runsAs = (row) =>
    h(
      "select",
      { class: "input", "aria-label": "Runs as", onchange: (ev) => ((row.stage = ev.currentTarget.value), redraw()) },
      RUNS_AS.map(([v, t]) => h("option", { value: v, selected: row.stage === v, text: t })),
    );
  const stepFields = (row, i) =>
    h(
      "fieldset",
      { class: "hook-events" },
      h("legend", { text: "Pipeline step" }),
      h("label", { class: "field check" }, h("input", { type: "checkbox", checked: row.required, onchange: (ev) => (row.required = ev.currentTarget.checked) }), h("span", { text: "Required" })),
      h("small", { class: "muted block", text: row.required ? "A failure holds the item until you retry or skip the step." : "A failure is retried a few times, then given up; the item finishes with a warning." }),
      field("Categories", text(row, "categories", { placeholder: "radarr, sonarr" }), "Comma-separated; empty runs for every item. Matches the Arr category."),
      h(
        "div",
        { class: "toolbar" },
        h("small", { class: "muted", text: `Runs ${stagePosition(rows, i)} in its stage` }),
        h("button", { type: "button", class: "btn btn-small", disabled: i === 0, "aria-label": `Move ${row.name} up`, onclick: () => ((rows = moveRow(rows, i, -1)), redraw()) }, "Up"),
        h("button", { type: "button", class: "btn btn-small", disabled: i === rows.length - 1, "aria-label": `Move ${row.name} down`, onclick: () => ((rows = moveRow(rows, i, 1)), redraw()) }, "Down"),
      ),
    );

  const card = (row, i) => {
    const kind = h(
      "select",
      { class: "input", onchange: (ev) => ((row.kind = ev.currentTarget.value), redraw()) },
      [["command", "Command"], ["url", "Webhook URL"]].map(([v, t]) => h("option", { value: v, selected: row.kind === v, text: t })),
    );
    const target =
      row.kind === "url"
        ? [field("URL", text(row, "url", { type: "url", placeholder: "https://ha.local:8123/api/webhook/…" }), (row.url || "").endsWith("…") ? "Shown cut down so its secrets stay hidden; leave it to keep the saved URL" : "Gets a POST with the event as JSON; shown cut down in logs")]
        : [
            field("Program", text(row, "program", { placeholder: "/config/hooks/on-download.py" }), "Absolute path; gets the event as JSON on stdin and runs as the daemon's user"),
            field("Arguments", h("textarea", { class: "input", rows: 2, spellcheck: "false", oninput: (ev) => (row.args = ev.currentTarget.value) }, row.args), "One per line"),
          ];
    const timeout = text(row, "timeout_secs", { type: "number", min: 1, max: 600, step: 1 });
    const enabled = h("label", { class: "field check" }, h("input", { type: "checkbox", checked: row.enabled, onchange: (ev) => (row.enabled = ev.currentTarget.checked) }), h("span", { text: "Enabled" }));
    return h(
      "fieldset",
      { class: "hook-card" },
      h("legend", { text: targetLabel(row) ? `${row.name} → ${targetLabel(row)}` : row.name }),
      h(
        "div",
        { class: "fields" },
        field("Name", text(row, "name", { "aria-label": `Hook ${i + 1} name`, pattern: "[A-Za-z0-9_\\-]{1,40}" }), "Letters, digits, - and _"),
        field("Runs as", runsAs(row)),
        field("Runs", kind),
        ...target,
        field("Timeout (s)", timeout, "1 to 600; a webhook is tried up to 3 times"),
        enabled,
      ),
      row.stage ? stepFields(row, i) : events(row),
      pairsTable(row),
      testBox(row),
      h("div", { class: "toolbar spaced" }, h("button", { type: "button", class: "btn btn-small", onclick: () => ((rows = rows.filter((_, j) => j !== i)), redraw()) }, "Remove hook")),
    );
  };

  const draw = () => {
    list.replaceChildren(...rows.map(card));
    none.hidden = rows.length > 0;
  };
  draw();
  get("/hooks")
    .then((s) => {
      for (const hook of s.hooks || []) status.set(hook.name, hook);
      if (list.isConnected) draw();
    })
    .catch(() => {});
  const add = () => {
    rows = [...rows, blankHook(rows.length + 1)];
    redraw();
    list.lastChild.querySelector("input").focus();
  };
  return {
    nodes: [none, list, h("button", { type: "button", class: "btn btn-small spaced", onclick: add }, "Add hook")],
    read: () => ({ hooks: rows.map(toSettings) }),
  };
}
