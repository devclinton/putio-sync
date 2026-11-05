// Integrations section (feature 046): Radarr / Sonarr (arr.js), Plugins and Event hooks.
// Feature 027: an editor per hook and a Send test event button; header and env values are
// shown only as set, with a Replace button. Feature 028: a card per Lua plugin with its
// state, the hosts it contacts, its settings by type (secrets shown only as set), and Send
// test event with the lines it logged; Reload reads the plugins folder again. Feature 031:
// action buttons (Sign in with Plex), Load buttons for fields with choices, and path rules.

import { get, post } from "../api.js";
import { blankHook, eventChoices, fromSettings, runLabel, targetLabel, toSettings } from "../hooks.js";
import * as plugins from "../plugins.js";
import { h, toast } from "../app.js";
import { relative } from "../format.js";
import { arrCard } from "./arr.js";
import { groupCard, reason } from "./form.js";

const PLUGINS = {
  id: "plugins",
  key: "plugins",
  title: "Plugins",
  fields: [["plugins_dir", "Plugins folder", "Empty for plugins next to the config file; each plugin is a folder with plugin.toml and main.lua", "s?"]],
  advanced: ["plugins_dir"],
  extra: pluginsExtra,
};
const HOOKS = { id: "hooks", key: "hooks", title: "Event hooks", fields: [], extra: hooksExtra };

export const cards = (ctx) => [arrCard(ctx), groupCard(PLUGINS, ctx), groupCard(HOOKS, ctx)];

/**
 * One editor per hook: name, on/off, command or URL, headers or env, events, timeout,
 * and for a saved hook its last run and a Send test event button
 */
function hooksExtra(data) {
  let rows = (data.hooks || []).map(fromSettings);
  const saved = new Set(rows.map((r) => r.name));
  const status = new Map();
  const list = h("div", { class: "stack" });
  const none = h("p", { class: "muted", text: "No hooks. A hook runs a command or calls a webhook with each event as JSON; see EVENTS.md for the fields." });
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
        field("Runs", kind),
        ...target,
        field("Timeout (s)", timeout, "1 to 600; a webhook is tried up to 3 times"),
        enabled,
      ),
      events(row),
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

// ---- Lua plugins (features 028, 031) -----------------------------------------

/**
 * One card per plugin found: state, what it contacts, Enabled, its settings by type,
 * timeout, last run and Send test event. Settings for a plugin not found are kept.
 * A plugin's actions (031) are buttons: an answer shows its message, code and link, fills
 * a field's choices, and its `next` step runs on its own for up to 10 minutes. What an
 * action saves is saved already; the card shows it.
 */
function pluginsExtra(data) {
  const stored = data.plugins || [];
  let found = null;
  let rows = [];
  const list = h("div", { class: "stack" }, h("p", { class: "muted", text: "Loading plugins…" }));
  const field = (label, input, hint) => h("label", { class: "field" }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);
  const changed = () => list.closest("form")?.dispatchEvent(new Event("change"));

  const input = (row, f, onHosts) => {
    const set = (v) => {
      row.values[f.key] = v;
      onHosts();
    };
    if (f.type === "boolean") return h("input", { type: "checkbox", checked: row.values[f.key], onchange: (ev) => set(ev.currentTarget.checked) });
    if (f.type === "string_list") return h("textarea", { class: "input", rows: 3, spellcheck: "false", oninput: (ev) => set(ev.currentTarget.value) }, row.values[f.key]);
    if (f.type === "path_map") return h("textarea", { class: "input", rows: 3, spellcheck: "false", placeholder: "/downloads => /data", oninput: (ev) => set(ev.currentTarget.value) }, row.values[f.key]);
    const type = f.type === "url" ? "url" : f.type === "integer" || f.type === "number" ? "number" : "text";
    return h("input", { class: "input", type, step: f.type === "number" ? "any" : undefined, value: row.values[f.key], autocomplete: "off", spellcheck: "false", oninput: (ev) => set(ev.currentTarget.value) });
  };

  const secret = (row, f) => {
    const s = row.secrets[f.key];
    const box = h("div");
    const draw = () => {
      box.replaceChildren(
        s.set && !s.replace
          ? h("div", { class: "toolbar" }, h("span", { class: "muted", text: "set" }), h("button", { type: "button", class: "btn btn-small", onclick: () => ((s.replace = true), draw(), changed()) }, "Replace"))
          : h("input", { class: "input", type: "password", value: s.value, autocomplete: "new-password", "aria-label": f.label || f.key, oninput: (ev) => (s.value = ev.currentTarget.value) }),
      );
    };
    draw();
    return box;
  };

  // The card of each plugin, so an action can draw its card again
  const cards = new Map();
  const redrawCard = (name) => {
    const c = cards.get(name);
    if (c && c.el.isConnected) c.el.replaceWith(card(c.status));
  };

  const runAction = async (row, id, input = {}, startedAt = Date.now()) => {
    row.flow = { ...(row.flow || {}), busy: true, error: null };
    redrawCard(row.name);
    try {
      const a = await post("/plugins/action", { name: row.name, action: id, input, config: plugins.overrides(row) });
      for (const key of plugins.choiceFields(row, id)) row.choices[key] = a.choices || [];
      if (a.saved) {
        plugins.applyAnswer(row, a);
        toast(`${row.name}: saved ${a.saved.join(", ")}`);
      }
      const wait = a.next && !a.done ? plugins.nextWait(a.next, startedAt, Date.now()) : null;
      row.flow = { message: a.message, code: a.code, open_url: a.open_url, busy: wait != null };
      if (a.next && !a.done && wait == null) row.flow.error = "Stopped after 10 minutes; try again";
      redrawCard(row.name);
      if (wait != null) setTimeout(() => list.isConnected && runAction(row, a.next.action, a.next.input, startedAt), wait);
    } catch (e) {
      row.flow = { ...(row.flow || {}), busy: false, error: e.status === 401 || e.status === 429 ? null : reason(e) };
      redrawCard(row.name);
    }
  };

  // A field with choices: a Load button, then a list to pick from (ticks for a list field)
  const pick = (row, f, onHosts) => {
    const opts = row.choices[f.key];
    const load = h("button", { type: "button", class: "btn btn-small", disabled: row.flow && row.flow.busy, onclick: () => runAction(row, f.choices) }, opts ? "Reload" : "Load");
    if (!opts) return h("div", { class: "toolbar" }, input(row, f, onHosts), load);
    if (f.type === "string_list") {
      const on = plugins.ticked(row, f.key);
      const all = [...opts, ...on.filter((v) => !opts.some((o) => o.value === v)).map((v) => ({ value: v, label: v }))];
      return h(
        "div",
        {},
        all.length ? null : h("p", { class: "muted", text: "Nothing to pick from" }),
        ...all.map((o) => h("label", { class: "field check" }, h("input", { type: "checkbox", checked: on.includes(o.value), onchange: (ev) => plugins.tick(row, f.key, o.value, ev.currentTarget.checked) }), h("span", { text: o.label }))),
        load,
      );
    }
    const current = row.values[f.key];
    const all = current && !opts.some((o) => o.value === current) ? [{ value: current, label: current }, ...opts] : opts;
    const select = h(
      "select",
      { class: "input", onchange: (ev) => ((row.values[f.key] = ev.currentTarget.value), onHosts()) },
      h("option", { value: "", text: "—" }),
      all.map((o) => h("option", { value: o.value, selected: o.value === current, text: o.label })),
    );
    return h("div", { class: "toolbar" }, select, load);
  };

  const actionsBox = (row, status) => {
    const shown = (status.actions || []).filter((a) => !a.hidden);
    const flow = row.flow || {};
    if (!shown.length && !flow.message && !flow.error) return null;
    return h(
      "div",
      { class: "spaced", "aria-live": "polite" },
      shown.length
        ? h("div", { class: "toolbar" }, ...shown.map((a) => h("button", { type: "button", class: "btn btn-small", title: a.help || undefined, disabled: flow.busy || status.state === "broken", onclick: () => runAction(row, a.id) }, a.label)))
        : null,
      flow.code ? h("p", { class: "big", text: flow.code }) : null,
      flow.message ? h("p", { text: flow.message }) : null,
      flow.open_url ? h("p", {}, h("a", { href: flow.open_url, target: "_blank", rel: "noopener noreferrer" }, "Open the sign-in page")) : null,
      flow.busy ? h("p", { class: "muted", text: "Waiting…" }) : null,
      flow.error ? h("p", { class: "error-text", text: flow.error }) : null,
    );
  };

  const testBox = (row, status) => {
    const out = h("span", { class: "muted", "aria-live": "polite" });
    const logs = h("pre", { class: "muted", hidden: true });
    const show = (run) => {
      out.textContent = run ? `Last run: ${run.event_type}, ${plugins.runLabel(run)}, ${relative(run.started_at)}` : "Not run yet";
      out.className = run && run.result !== "success" ? "error-text" : "muted";
      const lines = (run && run.logs) || [];
      logs.textContent = lines.map((l) => `${l.level}: ${l.message}`).join("\n");
      logs.hidden = !lines.length;
    };
    show((status.runs || []).at(-1));
    const send = async (ev) => {
      const button = ev.currentTarget;
      button.disabled = true;
      out.textContent = "Sending…";
      try {
        const run = await post("/plugins/test", { name: row.name });
        show(run);
        toast(`Test event to ${row.name}: ${plugins.runLabel(run)}`);
      } catch (e) {
        if (e.status !== 401 && e.status !== 429) {
          out.textContent = reason(e);
          out.className = "error-text";
        }
      }
      if (button.isConnected) button.disabled = false;
    };
    const saved = stored.some((s) => s.name === row.name);
    return h("div", { class: "spaced" }, h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small", onclick: send, disabled: status.state === "broken" }, "Send test event"), out), saved ? null : h("small", { class: "muted", text: "Tests use the saved settings." }), logs);
  };

  const card = (status) => {
    const row = rows.find((r) => r.name === status.name);
    const hosts = h("span");
    const showHosts = () => {
      const needed = plugins.hostsFor(status, row.values);
      hosts.textContent = needed.length ? `Contacts: ${needed.join(", ")}` : "No network access";
    };
    showHosts();
    const head = [status.version ? `v${status.version}` : null, status.source, plugins.stateLabel(status.state)].filter(Boolean).join(" · ");
    const body = row
      ? [
          h(
            "div",
            { class: "fields" },
            h("label", { class: "field check" }, h("input", { type: "checkbox", checked: row.enabled, onchange: (ev) => (row.enabled = ev.currentTarget.checked) }), h("span", { text: "Enabled" })),
            ...row.fields.map((f) => field((f.label || f.key) + (f.required ? " *" : ""), f.secret ? secret(row, f) : f.choices ? pick(row, f, showHosts) : input(row, f, showHosts), f.help)),
            field("Time limit (s)", h("input", { class: "input", type: "number", min: 1, max: 60, value: row.timeout_secs, oninput: (ev) => (row.timeout_secs = ev.currentTarget.value) }), "Longest one event may take, 1 to 60"),
          ),
          actionsBox(row, status),
          testBox(row, status),
        ]
      : [];
    const el = h(
      "fieldset",
      { class: "hook-card" },
      h("legend", { text: status.name }),
      h("p", { class: status.state === "broken" ? "error-text" : "muted", text: status.error ? `${head}: ${status.error}` : head }),
      status.description ? h("p", { text: status.description }) : null,
      status.events && status.events.length ? h("p", { class: "muted", text: `Events: ${status.events.join(", ")}` }) : null,
      h("p", { class: "muted" }, hosts),
      ...body,
    );
    cards.set(status.name, { el, status });
    return el;
  };

  const draw = () => {
    const orphans = stored.filter((s) => !found.some((p) => p.name === s.name));
    list.replaceChildren(
      ...(found.length ? found.map(card) : [h("p", { class: "muted", text: "No plugins found." })]),
      ...orphans.map((s) => h("p", { class: "muted", text: `Settings for ${s.name} are kept, but no plugin of that name is in the folder.` })),
    );
  };
  const load = () =>
    get("/plugins")
      .then((s) => {
        found = s.plugins || [];
        // Plugins with a broken manifest have no fields to edit
        rows = found.filter((p) => p.config_fields).map((p) => plugins.fromStatus(p, stored.find((x) => x.name === p.name)));
        if (list.isConnected || !list.parentNode) draw();
      })
      .catch((e) => list.replaceChildren(h("p", { class: "error-text", text: reason(e) })));
  load();
  const reload = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    try {
      toast((await post("/plugins/reload")).message);
      await load();
    } catch (e) {
      if (e.status !== 401 && e.status !== 429) toast(reason(e));
    }
    button.disabled = false;
  };
  return {
    nodes: [h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small", onclick: reload }, "Reload plugins")), list],
    read: () => {
      if (!found) return { plugins: stored };
      const edited = rows.map(plugins.toSettings);
      return { plugins: [...edited, ...stored.filter((s) => !edited.some((e) => e.name === s.name))] };
    },
  };
}
