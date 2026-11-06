// Folder rules (feature 009): the rule card and editor of the Put.io page, and the
// Folder rules, Presets and Recently blocked cards of Settings. A rule is set on a Put.io
// folder by id; each part of the effective rule says where it comes from (this folder,
// a parent folder, or the global Download settings). Feature 020 adds the Only download
// and Never download patterns, in the filter language of query.js. Feature 021 adds Keep:
// the empty-folder cleanup and the doctor leave the folder and its sub-folders alone.

import { del, get, post, put } from "./api.js";
import { confirm, h, toast } from "./app.js";
import { dateTime } from "./format.js";
import { filterMatcher } from "./query.js";

const FLAGS = [
  ["skip", "Skip folder", "Download nothing here"],
  ["delete_blocked", "Delete blocked files", "Remove files the type filter blocks from Put.io at each sync"],
  ["delete_after_download", "Delete after download", "Remove each file from Put.io once downloaded"],
  ["keep", "Keep out of cleanup", "The empty-folder cleanup and the doctor leave this folder and its sub-folders alone"],
];
// feature 040: what to do when a file is already on disk
const EXISTING = [
  ["resume", "Resume"],
  ["overwrite", "Overwrite"],
  ["keep_both", "Keep both"],
];
const existingLabel = (v) => (EXISTING.find(([k]) => k === v) || [v, v])[1];
const SHOWN_EXTS = 8;

const quiet = (e) => e.status === 401 || e.status === 429;
const reason = (e) => (e.details ? `${e.error}: ${e.details}` : e.error || String(e));
const yesNo = (v) => (v ? "yes" : "no");
const exts = (list) => (list.length > SHOWN_EXTS ? `${list.slice(0, SHOWN_EXTS).join(", ")} … (${list.length})` : list.join(", "));
const parseExts = (text) => text.split(/[\s,]+/).filter(Boolean);
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

function presetLabel(name, presets) {
  const p = presets.find((x) => x.name === name);
  return p ? p.label : name;
}

/** "this folder", "from Movies" or "global" */
function source(from, path) {
  if (from == null) return "global";
  return from === path ? "this folder" : `from ${from}`;
}

/** The effective type filter in words */
function filterText(filter) {
  if (filter.only) return `Only ${exts(filter.only)}`;
  return filter.blocked.length ? `All except ${exts(filter.blocked)}` : "Everything";
}

const matchers = new Map();
/** The matcher of pattern `text`, kept so a long list builds each one once */
function matcher(text) {
  if (!matchers.has(text)) matchers.set(text, filterMatcher(text));
  return matchers.get(text);
}

/** What the effective rule does with file `name` in folder `folder`: null, or { badge, why } */
function verdict(eff, name, folder) {
  if (!eff) return null;
  if (eff.skip.value) return { badge: "skipped", why: "Folder is skipped" };
  const path = folder ? `${folder}/${name}` : name;
  const never = eff.exclude.value;
  if (never && matcher(never)(name, path)) return { badge: "blocked", why: `Matches never-download pattern ${never}` };
  const only = eff.include.value;
  if (only && !matcher(only)(name, path)) return { badge: "blocked", why: `Doesn't match only-download pattern ${only}` };
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : null;
  const f = eff.filter;
  if (f.only && !(ext && f.only.includes(ext))) return { badge: "blocked", why: ext ? `${ext} is not allowed here` : "No extension; not allowed here" };
  if (!f.only && ext && f.blocked.includes(ext)) return { badge: "blocked", why: `${ext} is blocked` };
  return null;
}

// ---- Put.io page ------------------------------------------------------------

/**
 * The rule card of Put.io folder `id`. `onChange` runs after the effective rule loads
 * (for the file badges). `badge(name)` gives the badge of a file in the folder.
 */
export function ruleCard(id, onChange) {
  const el = h("section", { class: "card rule-card" }, h("h2", { text: "Folder rule" }), h("p", { class: "muted", text: "Loading…" }));
  let data = null;
  let presets = [];

  const load = async () => {
    try {
      const [folder, all] = await Promise.all([get(`/rules/folder/${id}`), get("/rules")]);
      data = folder;
      presets = all.presets;
    } catch (e) {
      if (quiet(e)) return;
      data = null;
      el.replaceChildren(h("h2", { text: "Folder rule" }), h("p", { class: "error-text", text: reason(e) }));
      return;
    }
    if (!el.isConnected) return;
    show();
    onChange();
  };

  const show = () => {
    const eff = data.effective;
    const path = data.path;
    const row = (k, v, from) => [h("dt", { text: k }), h("dd", {}, v, h("small", { class: "muted", text: ` · ${source(from, path)}` }))];
    const preset = eff.preset.value ? presetLabel(eff.preset.value, presets) : "none";
    const own = data.rule;
    const tools =
      id === 0
        ? h("p", { class: "muted spaced" }, "The root uses the ", h("a", { href: "#/settings/rules?field=card-filters", text: "File filters" }), ".")
        : h(
            "div",
            { class: "toolbar spaced" },
            h("button", { type: "button", class: "btn btn-small", onclick: edit }, own ? "Edit rule" : "Set rule"),
            own ? h("button", { type: "button", class: "btn btn-small", onclick: (ev) => clear(ev.currentTarget) }, "Clear rule") : null,
            h("button", { type: "button", class: "btn btn-small", onclick: (ev) => toggleKeep(ev.currentTarget) }, eff.keep.value ? "Don't keep" : "Keep folder"),
          );
    el.replaceChildren(
      h("h2", { text: own ? "Folder rule · this folder" : "Folder rule" }),
      h(
        "dl",
        { class: "kv rule-kv" },
        row("Preset", preset, eff.preset.from),
        row("Types", filterText(eff.filter), eff.filter.from),
        row("Only download", eff.include.value || "everything", eff.include.from),
        row("Never download", eff.exclude.value || "nothing", eff.exclude.from),
        FLAGS.map(([k, label]) => row(label, yesNo(eff[k].value), eff[k].from)),
        row("When a file exists", existingLabel(eff.on_existing.value), eff.on_existing.from),
      ),
      tools,
    );
  };

  const clear = async (button) => {
    button.disabled = true;
    try {
      const r = await del(`/rules/folder/${id}`);
      toast(r.message);
      await load();
    } catch (e) {
      if (!quiet(e)) toast("Couldn't clear the rule", "error", reason(e));
      if (button.isConnected) button.disabled = false;
    }
  };

  // One click: change only this folder's keep setting; a folder kept from a parent gets
  // its own "no"
  const toggleKeep = async (button) => {
    button.disabled = true;
    const keep = !data.effective.keep.value;
    try {
      const r = await put(`/rules/folder/${id}`, { ...(data.rule || {}), keep });
      toast(keep ? "Folder kept out of the empty-folder cleanup" : "Folder no longer kept", "ok", r.message);
      await load();
    } catch (e) {
      if (!quiet(e)) toast("Couldn't change the keep setting", "error", reason(e));
      if (button.isConnected) button.disabled = false;
    }
  };

  const edit = () => {
    const rule = data.rule || { preset: null, allow: [], block: [] };
    const eff = data.effective;
    const fid = (k) => `rule-${k}`;
    const presetSel = h(
      "select",
      { class: "input", id: fid("preset") },
      h("option", { value: "", text: "Inherit" }),
      presets.map((p) => h("option", { value: p.name, selected: p.name === rule.preset, text: p.label })),
    );
    const allow = h("input", { class: "input", id: fid("allow"), autocomplete: "off", spellcheck: "false", placeholder: "e.g. nfo, srt" });
    const block = h("input", { class: "input", id: fid("block"), autocomplete: "off", spellcheck: "false", placeholder: "e.g. iso" });
    allow.value = (rule.allow || []).join(", ");
    block.value = (rule.block || []).join(", ");
    // feature 020: blank inherits; "Don't inherit" sends "" to clear a parent's pattern
    const pattern = (k, placeholder) => {
      const input = h("input", { class: "input", id: fid(k), autocomplete: "off", spellcheck: "false", placeholder });
      input.value = rule[k] || "";
      const inherited = eff[k].from !== data.path && eff[k].value;
      const reset = rule[k] === "" || inherited ? h("input", { type: "checkbox", id: fid(`${k}-reset`), checked: rule[k] === "" }) : null;
      return { input, reset, inherited, value: () => input.value.trim() || (reset && reset.checked ? "" : null) };
    };
    const include = pattern("include", "e.g. *.mkv OR *.mp4");
    const exclude = pattern("exclude", "e.g. *sample* OR *.exe");
    const patternField = (p, label) =>
      h(
        "div",
        { class: "field" },
        h("label", { for: p.input.id, text: label }),
        p.input,
        h("small", { text: p.inherited ? `Blank inherits ${p.inherited}` : "Patterns with AND, OR and NOT; blank inherits" }),
        p.reset ? h("label", { class: "field check", for: p.reset.id }, p.reset, h("span", { text: "Don't inherit" })) : null,
      );
    const flags = FLAGS.map(([k, label, hint]) => {
      const v = rule[k];
      const sel = h(
        "select",
        { class: "input", id: fid(k) },
        h("option", { value: "", text: `Inherit (${yesNo(eff[k].value)})`, selected: v == null }),
        h("option", { value: "true", text: "Yes", selected: v === true }),
        h("option", { value: "false", text: "No", selected: v === false }),
      );
      return [k, sel, h("label", { class: "field", for: fid(k) }, h("span", { text: label }), sel, h("small", { text: hint }))];
    });
    const existingSel = h(
      "select",
      { class: "input", id: fid("on_existing") },
      h("option", { value: "", text: `Inherit (${existingLabel(eff.on_existing.value)})`, selected: rule.on_existing == null }),
      EXISTING.map(([v, text]) => h("option", { value: v, text, selected: rule.on_existing === v })),
    );
    const field = (input, label, hint) => h("label", { class: "field", for: input.id }, h("span", { text: label }), input, h("small", { text: hint }));
    const out = h("div", { class: "rule-preview", "aria-live": "polite" });
    const error = h("p", { class: "error-text", role: "alert", hidden: true });

    const body = () => {
      const b = { preset: presetSel.value || null, allow: parseExts(allow.value), block: parseExts(block.value), include: include.value(), exclude: exclude.value() };
      for (const [k, sel] of flags) b[k] = sel.value === "" ? null : sel.value === "true";
      b.on_existing = existingSel.value || null;
      return b;
    };
    const fail = (e) => {
      if (quiet(e)) return;
      error.textContent = reason(e);
      error.hidden = false;
    };
    const runPreview = async () => {
      error.hidden = true;
      out.replaceChildren(h("p", { class: "muted", text: "Checking…" }));
      try {
        const p = await post("/rules/preview", { folder_id: id, rule: body() });
        out.replaceChildren(h("div", {}, previewView(p)));
        return p;
      } catch (e) {
        out.replaceChildren();
        fail(e);
        return null;
      }
    };
    const save = async (ev) => {
      ev.preventDefault();
      const button = ev.submitter || form.querySelector("[type=submit]");
      button.disabled = true;
      error.hidden = true;
      const b = body();
      const deletes = b.delete_blocked ?? eff.delete_blocked.value;
      if (deletes && !b.skip) {
        const p = await runPreview();
        if (!p) return void (button.disabled = false);
        if (p.would_delete > 0) {
          const ok = await confirm({
            title: "Delete blocked files",
            text: `With this rule the next sync deletes ${plural(p.would_delete, "blocked file", "blocked files")} under ${p.path} from Put.io. This cannot be undone.`,
            verb: "Save rule",
          });
          if (!ok) return void (button.disabled = false);
        }
      }
      try {
        const r = await put(`/rules/folder/${id}`, b);
        toast(r.message);
        await load();
      } catch (e) {
        fail(e);
        if (button.isConnected) button.disabled = false;
      }
    };

    const form = h(
      "form",
      { class: "rule-form", onsubmit: save },
      h("h2", { text: `Rule for ${data.path}` }),
      h(
        "div",
        { class: "fields" },
        field(presetSel, "Preset", "Replaces the inherited type filter"),
        field(allow, "Also allow", "Extensions, comma separated"),
        field(block, "Also block", "Extensions, comma separated"),
        patternField(include, "Only download"),
        patternField(exclude, "Never download"),
        flags.map((f) => f[2]),
        field(existingSel, "When a file exists", "Resume, overwrite, or keep both under a new name"),
      ),
      error,
      h(
        "div",
        { class: "form-foot" },
        h("button", { type: "submit", class: "btn btn-primary" }, "Save"),
        h("button", { type: "button", class: "btn", onclick: runPreview }, "Preview"),
        h("button", { type: "button", class: "btn", onclick: show }, "Cancel"),
      ),
      out,
    );
    el.replaceChildren(form);
    presetSel.focus();
  };

  load();
  return {
    el,
    badge: (name) => (data ? verdict(data.effective, name, data.path) : null),
  };
}

const SHOWN_ITEMS = 20;

const fullName = (i) => (i.path ? `${i.path}/${i.name}` : i.name);

/** A rules preview: the counts, the files refused and a sample of those downloaded */
export function previewView(p) {
  const head = `${plural(p.files, "file", "files")}: ${p.download} download, ${p.blocked} blocked, ${p.skipped} skipped, ${p.would_delete} would be deleted`;
  const items = p.items.slice(0, SHOWN_ITEMS);
  const more = p.items.length - items.length + (p.truncated ? 1 : 0);
  const downloads = (p.downloads || []).slice(0, SHOWN_ITEMS);
  return [
    h("p", { class: "spaced", text: head }),
    items.length
      ? h(
          "ul",
          { class: "rule-items" },
          items.map((i) =>
            h(
              "li",
              {},
              h("span", { class: "name", text: fullName(i) }),
              h("small", { class: "muted", text: ` ${i.reason}` }),
              i.would_delete ? h("span", { class: "badge bad", text: "would be deleted" }) : null,
            ),
          ),
        )
      : null,
    more > 0 ? h("p", { class: "muted", text: p.truncated ? "and more" : `and ${more} more` }) : null,
    downloads.length
      ? [
          h("p", { class: "spaced muted", text: p.download > downloads.length ? `Downloaded, first ${downloads.length}:` : "Downloaded:" }),
          h("ul", { class: "rule-items" }, downloads.map((i) => h("li", {}, h("span", { class: "name", text: fullName(i) })))),
        ]
      : null,
  ];
}

// ---- Settings page ----------------------------------------------------------

/** Folder rules, Presets and Recently blocked; `alive()` is false once the page is left */
export function settingsCards(alive) {
  const rules = h("section", { class: "card", id: "card-folder-rules" });
  const presets = h("section", { class: "card", id: "card-presets" });
  const decisions = h("section", { class: "card", id: "card-blocked" });

  const loadRules = async () => {
    let all;
    try {
      all = await get("/rules");
    } catch (e) {
      if (alive() && !quiet(e)) {
        rules.replaceChildren(h("h2", { text: "Folder rules" }), h("p", { class: "error-text", text: reason(e) }));
        presets.replaceChildren();
      }
      return;
    }
    if (!alive()) return;
    rules.replaceChildren(h("h2", { text: "Folder rules" }), h("div", {}, rulesList(all, loadRules)));
    presets.replaceChildren(h("h2", { text: "Presets" }), h("div", {}, presetsList(all.presets, loadRules)));
  };

  const loadDecisions = async () => {
    let d;
    try {
      d = await get("/rules/decisions?limit=50");
    } catch (e) {
      if (alive() && !quiet(e)) decisions.replaceChildren(h("h2", { text: "Recently blocked" }), h("p", { class: "muted", text: reason(e) }));
      return;
    }
    if (alive()) decisions.replaceChildren(h("h2", { text: "Recently blocked" }), h("div", {}, decisionsList(d.items, loadDecisions)));
  };

  for (const [card, title] of [[rules, "Folder rules"], [presets, "Presets"], [decisions, "Recently blocked"]]) {
    card.append(h("h2", { text: title }), h("p", { class: "muted", text: "Loading…" }));
  }
  loadRules();
  loadDecisions();
  return [rules, presets, decisions];
}

function ruleSummary(r, presets) {
  const parts = [];
  if (r.preset) parts.push(presetLabel(r.preset, presets));
  if (r.allow && r.allow.length) parts.push(`allow ${r.allow.join(", ")}`);
  if (r.block && r.block.length) parts.push(`block ${r.block.join(", ")}`);
  if (r.include != null) parts.push(r.include ? `only ${r.include}` : "only: not inherited");
  if (r.exclude != null) parts.push(r.exclude ? `never ${r.exclude}` : "never: not inherited");
  for (const [k, label] of FLAGS) if (r[k] != null) parts.push(`${label.toLowerCase()}: ${yesNo(r[k])}`);
  return parts.join(" · ");
}

function rulesList(all, reload) {
  const g = all.global;
  const global = h("p", { class: "muted", text: `Root (global): ${g.blocked_extensions.length ? `blocks ${exts(g.blocked_extensions)}` : "blocks nothing"} · delete after download: ${yesNo(g.delete_after_download)} · when a file exists: ${existingLabel(g.on_existing).toLowerCase()}${g.include ? ` · only ${g.include}` : ""}${g.exclude ? ` · never ${g.exclude}` : ""}` });
  if (!all.rules.length) return [global, h("p", { class: "muted", text: "No folder rules. Set one from a folder on the Put.io page." })];
  const remove = async (button, path) => {
    const ok = await confirm({ title: "Remove folder rule", text: `Remove the rule for ${path}? Its folders go back to the inherited settings.`, verb: "Remove" });
    if (!ok) return;
    button.disabled = true;
    try {
      const r = await post("/rules/delete", { path });
      toast(r.message);
      reload();
    } catch (e) {
      if (!quiet(e)) toast("Couldn't remove the rule", "error", reason(e));
      if (button.isConnected) button.disabled = false;
    }
  };
  return [
    global,
    h(
      "ul",
      { class: "rule-items" },
      all.rules.map((r) =>
        h(
          "li",
          {},
          h("span", { class: "name", text: r.path }),
          h("small", { class: "muted", text: ` ${ruleSummary(r, all.presets) || "no settings"}` }),
          h("button", { type: "button", class: "btn btn-small", "aria-label": `Remove the rule for ${r.path}`, onclick: (ev) => remove(ev.currentTarget, r.path) }, "Remove"),
        ),
      ),
    ),
  ];
}

function presetsList(list, reload) {
  const call = async (button, fn, failText) => {
    button.disabled = true;
    try {
      const r = await fn();
      toast(r.message);
      reload();
    } catch (e) {
      if (!quiet(e)) toast(failText, "error", reason(e));
      if (button.isConnected) button.disabled = false;
    }
  };
  const editor = (p) => {
    const li = h("li", { class: "preset-edit" });
    const mode = h("select", { class: "input", "aria-label": "Kind" }, h("option", { value: "only", text: "Only these", selected: !!(p && p.only) }), h("option", { value: "block", text: "All except these", selected: !!(p && !p.only) }));
    const list = h("input", { class: "input", "aria-label": "Extensions", autocomplete: "off", spellcheck: "false", placeholder: "Extensions, comma separated" });
    list.value = p ? (p.only || p.block).join(", ") : "";
    const name = p ? null : h("input", { class: "input", "aria-label": "Preset name", autocomplete: "off", spellcheck: "false", placeholder: "name, e.g. comics" });
    const save = h("button", { type: "submit", class: "btn btn-small btn-primary" }, "Save");
    const cancel = h("button", { type: "button", class: "btn btn-small", onclick: reload }, "Cancel");
    const submit = (ev) => {
      ev.preventDefault();
      const n = p ? p.name : name.value.trim();
      const e = parseExts(list.value);
      const body = mode.value === "only" ? { only: e } : { block: e };
      call(save, () => put(`/rules/presets/${encodeURIComponent(n)}`, body), "Couldn't save the preset");
    };
    li.append(h("form", { class: "preset-form", onsubmit: submit }, p ? h("span", { class: "name", text: p.label }) : name, mode, list, h("div", { class: "row-actions" }, save, cancel)));
    return li;
  };
  const ul = h(
    "ul",
    { class: "rule-items" },
    list.map((p) => {
      const kind = p.only ? `only ${exts(p.only)}` : p.block.length ? `all except ${exts(p.block)}` : "everything";
      const used = p.used_by.length ? ` · used by ${p.used_by.join(", ")}` : "";
      const tag = p.builtin ? (p.overridden ? "edited" : "built-in") : p.name;
      const li = h(
        "li",
        {},
        h("span", { class: "name", text: p.label }),
        h("span", { class: "badge", text: tag }),
        h("small", { class: "muted", text: ` ${kind}${used}` }),
        h(
          "div",
          { class: "row-actions" },
          h("button", { type: "button", class: "btn btn-small", "aria-label": `Edit ${p.label}`, onclick: () => li.replaceWith(editor(p)) }, "Edit"),
          p.overridden || !p.builtin
            ? h("button", { type: "button", class: "btn btn-small", "aria-label": `${p.builtin ? "Reset" : "Delete"} ${p.label}`, onclick: (ev) => call(ev.currentTarget, () => del(`/rules/presets/${encodeURIComponent(p.name)}`), "Couldn't change the preset") }, p.builtin ? "Reset" : "Delete")
            : null,
        ),
      );
      return li;
    }),
  );
  const add = h("button", { type: "button", class: "btn btn-small spaced", onclick: () => ul.append(editor(null)) }, "Add preset");
  return [ul, add];
}

function decisionsList(items, reload) {
  if (!items.length) return h("p", { class: "muted", text: "No files blocked, skipped or deleted by a rule." });
  const clear = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    try {
      const r = await post("/rules/decisions/clear");
      toast(r.message);
      reload();
    } catch (e) {
      if (!quiet(e)) toast("Couldn't clear", "error", reason(e));
      if (button.isConnected) button.disabled = false;
    }
  };
  return [
    h(
      "ul",
      { class: "rule-items" },
      items.map((d) =>
        h(
          "li",
          {},
          h("span", { class: "name", text: d.path ? `${d.path}/${d.name}` : d.name }),
          h("span", { class: `badge${d.decision === "deleted" ? " bad" : d.decision === "blocked" ? " warn" : ""}`, text: d.decision }),
          h("small", { class: "muted", text: ` ${d.reason} · ${d.rule ? `rule ${d.rule}` : "global"} · ${dateTime(d.decided_at)}` }),
        ),
      ),
    ),
    h("button", { type: "button", class: "btn btn-small spaced", onclick: clear }, "Clear"),
  ];
}
