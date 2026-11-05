// Plugins section (features 028, 031, 048). Each plugin that is on has a tab with its
// state, the hosts it contacts, its settings by type (secrets shown only as set), time limit,
// action buttons (031: Sign in with Plex, Load buttons for fields with choices), Send test
// event with the lines it logged, and Disable. The others are one short list with Enable,
// which names the hosts the plugin will contact and turns it on in a save of its own; the
// save approves those hosts. Reload reads the plugins folder again.

import { get, post, put } from "../api.js";
import * as plugins from "../plugins.js";
import { confirm, fill, h, toast, toastError } from "../app.js";
import { relative } from "../format.js";
import { groupCard, quiet, reason } from "./form.js";

const PLUGINS = {
  id: "plugins",
  key: "plugins",
  title: "Plugins",
  fields: [["plugins_dir", "Plugins folder", "Empty for plugins next to the config file; each plugin is a folder with plugin.toml and main.lua", "s?"]],
  advanced: ["plugins_dir"],
  extra: pluginsExtra,
};

export const cards = (ctx) => [groupCard(PLUGINS, ctx)];

// The tab shown, kept while the card is drawn again after a save
let selected = null;
const NOT_OK = ["broken", "needs_approval", "missing_config"];

/**
 * A tab per plugin that is on and a list of the rest (048). Settings for a plugin not found
 * are kept. A plugin's actions (031) are buttons: an answer shows its message, code and link,
 * fills a field's choices, and its `next` step runs on its own for up to 10 minutes. What an
 * action saves is saved already; the tab shows it. Enable and Disable save that plugin's
 * `enabled` alone, on top of the saved settings, and leave other edits unsaved.
 */
function pluginsExtra(data, form) {
  let stored = data.plugins || [];
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

  // The tab panel of each plugin, so an action can draw it again
  const panels = new Map();
  const redrawCard = (name) => {
    const c = panels.get(name);
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
      row.flow = plugins.carryFlow(row.flow, a, wait != null);
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

  // 048: copy the code; with a link, also open it (in the click, so it is not blocked as a
  // pop-up). Where the browser won't copy (plain http) the code is selected to copy by hand.
  const copyCode = (row, url) => {
    const copied = navigator.clipboard && window.isSecureContext ? navigator.clipboard.writeText(row.flow.code) : Promise.reject();
    if (url) window.open(url, "_blank", "noopener");
    copied.then(
      () => toast("Code copied"),
      () => {
        row.flow.manual = true;
        redrawCard(row.name);
        const el = panels.get(row.name)?.el.querySelector(".code");
        if (el) getSelection().selectAllChildren(el);
      },
    );
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
      flow.code
        ? h(
            "div",
            { class: "toolbar" },
            h("p", { class: "big code", text: flow.code }),
            h("button", { type: "button", class: "btn btn-small", onclick: () => copyCode(row) }, "Copy"),
            flow.open_url ? h("button", { type: "button", class: "btn btn-small btn-primary", onclick: () => copyCode(row, flow.open_url) }, `Copy and open ${plugins.linkLabel(flow.open_url)}`) : null,
          )
        : null,
      flow.code && flow.manual ? h("p", { class: "muted", text: "Couldn't copy (the browser allows it over HTTPS or localhost only): click the code to select it, then press Ctrl+C (⌘C)." }) : null,
      flow.message ? h("p", { text: flow.message }) : null,
      flow.open_url ? h("p", {}, h("a", { href: flow.open_url, target: "_blank", rel: "noopener noreferrer" }, `Open ${plugins.linkLabel(flow.open_url)}`)) : null,
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
      const needed = row ? plugins.hostsFor(status, row.values) : status.hosts_needed;
      hosts.textContent = needed.length ? `Contacts: ${needed.join(", ")}` : "No network access";
    };
    showHosts();
    const head = [status.version ? `v${status.version}` : null, status.source, plugins.stateLabel(status.state)].filter(Boolean).join(" · ");
    const body = row
      ? [
          h(
            "div",
            { class: "fields" },
            ...row.fields.map((f) => field((f.label || f.key) + (f.required ? " *" : ""), f.secret ? secret(row, f) : f.choices ? pick(row, f, showHosts) : input(row, f, showHosts), f.help)),
            field("Time limit (s)", h("input", { class: "input", type: "number", min: 1, max: 60, value: row.timeout_secs, oninput: (ev) => (row.timeout_secs = ev.currentTarget.value) }), "Longest one event may take, 1 to 60"),
          ),
          actionsBox(row, status),
          testBox(row, status),
        ]
      : [];
    const el = h(
      "div",
      { class: "plugin-panel", role: "tabpanel", id: `plugin-panel-${status.name}`, "aria-labelledby": `plugin-tab-${status.name}`, hidden: status.name !== selected },
      h("p", { class: status.state === "broken" ? "error-text" : "muted", text: status.error ? `${head}: ${status.error}` : head }),
      status.description ? h("p", { text: status.description }) : null,
      status.events && status.events.length ? h("p", { class: "muted", text: `Events: ${status.events.join(", ")}` }) : null,
      h("p", { class: "muted" }, hosts),
      ...body,
      h("div", { class: "toolbar spaced" }, h("button", { type: "button", class: "btn btn-small", onclick: (ev) => turn(status, false, ev.currentTarget) }, "Disable")),
    );
    panels.set(status.name, { el, status });
    return el;
  };

  const pickTab = (name) => {
    selected = name;
    for (const t of list.querySelectorAll("[role=tab]")) t.setAttribute("aria-selected", String(t.id === `plugin-tab-${name}`));
    for (const p of list.querySelectorAll("[role=tabpanel]")) p.hidden = p.id !== `plugin-panel-${name}`;
  };
  const tab = (status) =>
    h(
      "button",
      { type: "button", role: "tab", id: `plugin-tab-${status.name}`, "aria-controls": `plugin-panel-${status.name}`, "aria-selected": String(status.name === selected), onclick: () => pickTab(status.name) },
      status.name,
      NOT_OK.includes(status.state) ? h("span", { class: "badge bad", text: "!", title: plugins.stateLabel(status.state) }) : null,
    );

  // A plugin that is off: one line, and Enable unless it does not load
  const item = (status) => {
    const broken = status.state === "broken";
    return h(
      "li",
      {},
      h(
        "div",
        {},
        h("strong", { text: status.name }),
        status.version ? h("span", { class: "muted", text: ` v${status.version}` }) : null,
        h("span", { class: broken ? "error-text one-line" : "muted one-line", title: (broken ? status.error : status.description) || null, text: broken ? status.error : plugins.summary(status.description) }),
      ),
      broken ? null : h("button", { type: "button", class: "btn btn-small", onclick: (ev) => turn(status, true, ev.currentTarget) }, "Enable"),
    );
  };

  // Enable or Disable: save that plugin's `enabled` alone on top of the saved settings, then
  // draw again from what was saved. Other unsaved edits stay, and so does the unsaved mark.
  const turn = async (status, on, button) => {
    const row = rows.find((r) => r.name === status.name) || plugins.fromStatus(status, null);
    const saved = plugins.fromStatus(status, stored.find((s) => s.name === status.name));
    const edited = JSON.stringify(plugins.toSettings(row)) !== JSON.stringify(plugins.toSettings(saved));
    const ok = on
      ? await confirm({ title: `Enable ${status.name}?`, text: plugins.approvalText(status, row), verb: "Enable" })
      : !edited || (await confirm({ title: `Disable ${status.name}?`, text: "Its unsaved changes will be dropped; its saved settings are kept.", verb: "Disable" }));
    if (!ok) return;
    const clean = form.clean();
    button.disabled = true;
    try {
      const body = { ...data, plugins: plugins.withEnabled(stored, status, on) };
      delete body.effective;
      delete body.status;
      await put("/settings/plugins", body);
      data.plugins = stored = (await get("/settings/plugins")).plugins || [];
      rows = rows.filter((r) => r.name !== status.name);
      if (on) selected = status.name;
      await load();
      if (clean) form.rebase();
      form.onSaved();
      toast(`${status.name} is ${on ? "on" : "off"}`);
    } catch (e) {
      if (!quiet(e)) toastError(e);
      if (button.isConnected) button.disabled = false;
    }
  };

  const draw = () => {
    const { tabs, available } = plugins.split(found, stored);
    if (!tabs.some((p) => p.name === selected)) selected = tabs.length ? tabs[0].name : null;
    panels.clear();
    const orphans = stored.filter((s) => !found.some((p) => p.name === s.name));
    fill(
      list,
      found.length ? null : h("p", { class: "muted", text: "No plugins found." }),
      tabs.length ? h("div", { class: "tabs", role: "tablist", "aria-label": "Plugins that are on" }, tabs.map(tab)) : null,
      tabs.map(card),
      available.length ? h("div", { class: "plugin-avail" }, h("h3", { class: "sub", text: "Available plugins" }), h("ul", { class: "plugin-list" }, available.map(item))) : null,
      orphans.map((s) => h("p", { class: "muted", text: `Settings for ${s.name} are kept, but no plugin of that name is in the folder.` })),
    );
  };
  // Rows keep unsaved edits across a reload while the plugin's fields stay the same
  const load = () =>
    get("/plugins")
      .then((s) => {
        found = s.plugins || [];
        const old = new Map(rows.map((r) => [r.name, r]));
        // Plugins with a broken manifest have no fields to edit
        rows = found
          .filter((p) => p.config_fields)
          .map((p) => {
            const r = old.get(p.name);
            return r && JSON.stringify(r.fields) === JSON.stringify(p.config_fields) ? r : plugins.fromStatus(p, stored.find((x) => x.name === p.name));
          });
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
      if (!quiet(e)) toast(reason(e));
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
