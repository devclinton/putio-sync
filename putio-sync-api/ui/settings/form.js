// One settings group as a card (features 015, 046): a form built from the GET body. Save PUTs
// the whole group and shows the API's message verbatim; a 400 shows its error under the form.
// Each card previews a save as you type, says what each field affects and whether it applies
// now, and has a Defaults button. A `part` card holds only some of a group's fields and saves
// them on top of the latest saved values, so two cards of one group don't undo each other.
// A card reports unsaved changes and problems to the page through `ctx.report`.

import { get, previewSettings, put, settingsAssistant } from "../api.js";
import { describeEntry } from "../bandwidth.js";
import { h, toast } from "../app.js";

let assist = null;

/** GET /settings/assistant once per page (help, defaults, risks); null when it fails */
export const assistant = (fresh = false) => (fresh || !assist ? (assist = settingsAssistant().catch(() => null)) : assist);

export const reason = (e) => (e.details ? `${e.error}: ${e.details}` : e.error || String(e));
export const quiet = (e) => e.status === 401 || e.status === 429;

export const riskList = (risks) =>
  risks.map((r) => h("p", { class: r.level === "danger" ? "error-text" : "warn-text" }, h("strong", { text: r.level === "danger" ? "Danger: " : "Warning: " }), r.message));

const shown = (v) => {
  const item = (x) => (x && typeof x === "object" ? ("days" in x ? describeEntry(x) : JSON.stringify(x)) : x);
  const s = v == null ? "empty" : Array.isArray(v) ? v.map(item).join(", ") || "none" : typeof v === "object" ? JSON.stringify(v) : String(v);
  const max = Array.isArray(v) ? 200 : 60;
  return s.length > max ? `${s.slice(0, max - 3)}…` : s;
};

/** A preview: what would change and whether it applies now, the refusal, and the risks */
export function previewNodes(p) {
  if (!p.valid) return h("p", { class: "error-text", text: `Would be refused: ${p.error}` });
  const changes = p.changes.length
    ? h(
        "ul",
        { class: "changes" },
        p.changes.map((c) => h("li", {}, h("code", { text: c.key }), ` ${shown(c.before)} → ${shown(c.after)} `, h("span", { class: `badge ${c.live ? "ok" : "warn"}`, text: c.live ? "applies now" : "needs a restart" }))),
      )
    : h("p", { class: "muted", text: "Nothing would change." });
  return h("div", {}, changes, riskList(p.risks || []));
}

/** A card that loads `/settings/<g.key>`, with Retry when that fails */
export function groupCard(g, ctx) {
  const card = h("section", { class: "card", id: `card-${g.id}` });
  const load = async () => {
    card.replaceChildren(h("h2", { text: g.title }), h("p", { class: "muted", text: "Loading…" }));
    try {
      const [data, a] = await Promise.all([get(`/settings/${g.key}`), assistant()]);
      if (!ctx.alive()) return;
      ctx.saved[g.key] = data;
      card.replaceChildren(form(g, data, load, a && a.groups[g.key], ctx));
      ctx.report(g.id, { dirty: false, problem: false });
    } catch (e) {
      if (!ctx.alive() || quiet(e)) return;
      ctx.report(g.id, { problem: true });
      card.replaceChildren(
        h("h2", { text: g.title }),
        h("p", { class: "error-text", text: reason(e) }),
        h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"),
      );
    }
  };
  load();
  return card;
}

function form(g, data, reload, help, ctx, start = null) {
  const inputs = {};
  const note = h("p", { class: "muted", role: "status" });
  const unsaved = h("span", { class: "badge warn", hidden: true, text: "Unsaved changes" });
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
  // What the field affects and whether a change applies now (feature 015)
  const affects = (name) => {
    const text = help && help.affects[name];
    if (!text) return null;
    const live = help.live.includes(name);
    return h("small", { class: "affects" }, `Affects: ${text}. `, h("span", { class: `badge ${live ? "ok" : "warn"}`, text: live ? "applies now" : "needs a restart" }));
  };
  const build = ([name, label, hint, kind, min, max]) => {
    const id = `set-${g.key}-${name}`;
    const value = data[name];
    let input;
    if (kind === "b") {
      input = h("input", { type: "checkbox", id, checked: Boolean(value) });
      inputs[name] = () => input.checked;
      return h("label", { class: "field check", for: id }, input, h("span", {}, label, hint ? h("small", { text: hint }) : null));
    }
    if (Array.isArray(kind)) {
      // Each option is a value, or [value, label, help]; the help line follows the choice
      const opts = kind.map((o) => (Array.isArray(o) ? o : [o, o]));
      if (value != null && !opts.some(([v]) => v === value)) opts.unshift([value, value]);
      input = h("select", { class: "input", id }, opts.map(([v, text]) => h("option", { value: v, selected: v === value, text })));
      inputs[name] = () => input.value;
      const help = opts.some((o) => o[2]) ? h("small") : null;
      if (help) {
        const update = () => (help.textContent = opts.find(([v]) => v === input.value)?.[2] || "");
        input.addEventListener("change", update);
        update();
        return h("label", { class: "field", for: id }, h("span", { text: label }), input, help);
      }
    } else if (kind === "l") {
      input = h("textarea", { class: "input", id, rows: 3, spellcheck: "false" });
      input.value = (value || []).join("\n");
      inputs[name] = () => input.value.split("\n").map((s) => s.trim()).filter(Boolean);
    } else {
      const num = kind[0] === "n";
      const optional = kind.endsWith("?");
      const locked = Boolean(g.locked && g.locked(data).includes(name));
      input = h("input", { class: "input", id, type: num ? "number" : "text", min, max, step: num ? 1 : null, required: !optional, disabled: locked, autocomplete: "off", spellcheck: "false" });
      input.value = value == null ? "" : String(value);
      inputs[name] = () => {
        const v = input.value.trim();
        if (v === "") return optional ? null : v;
        return num ? Number(v) : v;
      };
    }
    return h("label", { class: "field", for: id }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);
  };
  const fields = g.fields.map((f) => {
    const node = build(f);
    const line = affects(f[0]);
    if (line) (f[3] === "b" ? node.lastChild : node).append(line);
    if (g.advanced && g.advanced.includes(f[0])) node.classList.add("adv");
    return node;
  });
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const extra = g.extra ? g.extra(data) : null;
  const read = () => {
    const body = { ...(ctx.saved[g.key] || data) };
    for (const [name, value] of Object.entries(inputs)) body[name] = value();
    if (extra) Object.assign(body, extra.read());
    delete body.effective;
    delete body.status;
    return body;
  };

  // Unsaved: this card's own values against a snapshot taken before the first edit (R3)
  const own = () => JSON.stringify([Object.values(inputs).map((f) => f()), extra ? extra.read() : null]);
  let base = start;
  const touch = () => (base ??= own());
  const mark = () => {
    const dirty = base != null && own() !== base;
    unsaved.hidden = !dirty;
    ctx.report(g.id, { dirty });
  };

  // Preview the save 400 ms after the last edit; a newer edit drops an older answer
  const panel = h("div", { class: "preview", "aria-live": "polite", hidden: true });
  let timer = 0;
  let asked = 0;
  const preview = () => {
    mark();
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const mine = ++asked;
      try {
        const p = await previewSettings(g.key, read());
        if (mine !== asked || !panel.isConnected) return;
        panel.replaceChildren(h("h3", { text: "If you save" }), previewNodes(p));
        panel.hidden = false;
      } catch (e) {
        if (mine === asked && !quiet(e)) {
          panel.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
          panel.hidden = false;
        }
      }
    }, 400);
  };
  // A part card fills in only its own settings
  const mine = (defaults) => (g.part ? Object.fromEntries(Object.entries(defaults).filter(([k]) => k in inputs || (g.owns || []).includes(k))) : defaults);
  const defaults =
    help && help.defaults
      ? h(
          "button",
          {
            type: "button",
            class: "btn",
            title: "Fill in the defaults; nothing is saved until Save",
            onclick: () => {
              const fresh = form(g, { ...data, ...mine(help.defaults) }, reload, help, ctx, touch());
              el.replaceWith(fresh);
              fresh.querySelector("[role=status]").textContent = "Defaults filled in; not saved yet.";
              fresh.dispatchEvent(new Event("change"));
            },
          },
          "Defaults",
        )
      : null;

  const onSubmit = async (ev) => {
    ev.preventDefault();
    clearTimeout(timer);
    asked += 1;
    const body = read();
    if (g.beforeSave && !(await g.beforeSave(body, data))) return;
    save.disabled = true;
    error.hidden = true;
    note.textContent = "";
    try {
      const res = await put(`/settings/${g.key}`, body);
      const message = (res && res.message) || "Saved";
      toast(message);
      ctx.onSaved();
      await reload();
      // reload replaced the form; show the message on the new one
      const fresh = document.querySelector(`form[data-card="${g.id}"] [role=status]`);
      if (fresh) fresh.textContent = message;
    } catch (e) {
      if (quiet(e)) return;
      error.textContent = reason(e);
      error.hidden = false;
      ctx.report(g.id, { problem: true });
      if (save.isConnected) save.disabled = false;
    }
  };

  const el = h(
    "form",
    {
      "data-card": g.id,
      onsubmit: onSubmit,
      onpointerdown: touch,
      onkeydown: touch,
      onfocusin: touch,
      oninput: () => {
        note.textContent = "";
        preview();
      },
      onchange: preview,
    },
    h("h2", { text: g.title }),
    h("div", { class: "fields" }, fields),
    extra ? extra.nodes : null,
    panel,
    error,
    h("div", { class: "form-foot" }, save, defaults, note, unsaved),
  );
  return el;
}
