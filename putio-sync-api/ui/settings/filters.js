// Rules and filters section (feature 046): name patterns for every folder (part of the Download
// group, with a preview over the whole account, feature 020), folder rules, presets and
// recently blocked (rules.js), and the release check (feature 042).

import { post } from "../api.js";
import { h } from "../app.js";
import { previewView, settingsCards } from "../rules.js";
import { groupCard, quiet, reason } from "./form.js";

const FILTERS = {
  id: "filters",
  key: "download",
  part: true,
  title: "File filters",
  fields: [
    ["whitelist_patterns", "Only download", "Patterns, e.g. *.mkv OR *.mp4; empty for all", "s?"],
    ["blacklist_patterns", "Never download", "e.g. *sample* OR *.exe", "s?"],
    ["blocked_extensions", "Blocked extensions", "One per line, without the dot", "l"],
  ],
  extra: patternPreview,
};
const PRECHECK = {
  id: "precheck",
  key: "precheck",
  title: "Release check",
  fields: [
    ["min_size_mb", "Minimum size (MB)", "The largest wanted file must be at least this big; 0 to 100000", "n", 0, 100000],
    ["lookup_timeout_secs", "Lookup timeout (s)", "Total time for all caches; 1 to 30", "n", 1, 30],
    ["caches", "Magnet caches", "One URL per line, with {HASH} or {hash}. Only the info hash is sent.", "l"],
    ["enabled", "Enabled", "Check releases before they reach Put.io or the NAS", "b"],
    ["dry_run", "Check only (don't reject or delete)", "Log and count what would be rejected", "b"],
    ["allow_archives", "Allow archives", "Count .rar, .7z and split parts as wanted", "b"],
    ["magnet_lookup", "Look up magnets", "Find a magnet's .torrent in the caches before adding it", "b"],
    ["hold_magnet_secs", "Hold magnets (s)", "Hold magnets no cache knows; 0 (off) to 3600", "n", 0, 3600],
    ["hold_retry_secs", "Ask again every (s)", "10 to 600", "n", 10, 600],
    ["first_lookup_secs", "First ask (s)", "While holding; 1 to the lookup timeout", "n", 1, 30],
  ],
  advanced: ["lookup_timeout_secs", "caches", "hold_magnet_secs", "hold_retry_secs", "first_lookup_secs"],
  extra: precheckExtra,
};

export function cards(ctx) {
  const [rules, presets, blocked] = settingsCards(ctx.alive);
  return [groupCard(FILTERS, ctx), rules, presets, blocked, groupCard(PRECHECK, ctx)];
}

/** Preview of the typed Only and Never download patterns over the whole account (020) */
function patternPreview() {
  const out = h("div", { class: "rule-preview", "aria-live": "polite" });
  const value = (name) => document.getElementById(`set-download-${name}`).value;
  const run = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    out.replaceChildren(h("p", { class: "muted", text: "Checking…" }));
    try {
      const p = await post("/rules/preview", { folder_id: 0, global: { include: value("whitelist_patterns"), exclude: value("blacklist_patterns") } });
      out.replaceChildren(h("div", {}, previewView(p)));
    } catch (e) {
      out.replaceChildren(quiet(e) ? "" : h("p", { class: "error-text", text: reason(e) }));
    }
    if (button.isConnected) button.disabled = false;
  };
  return {
    nodes: [
      h(
        "div",
        { class: "toolbar spaced" },
        h("button", { type: "button", class: "btn btn-small", onclick: run }, "Preview patterns"),
        h("small", { class: "muted", text: "Lists your whole Put.io account; this can take a while." }),
      ),
      out,
    ],
    read: () => ({}),
  };
}

const TRISTATE = [
  ["", "Default"],
  ["true", "On"],
  ["false", "Off"],
];

function precheckExtra(data) {
  const categories = overrides("categories", "Per category", "The *arr category, e.g. sonarr", data.categories);
  const watch = overrides("watch", "Per watch folder", "A watched folder, as listed above", data.watch);
  categories.node.classList.add("adv");
  watch.node.classList.add("adv");
  return {
    nodes: [categories.node, watch.node, effectiveTable(data.effective || [])],
    read: () => ({ categories: categories.read(), watch: watch.read() }),
  };
}

/** Rows of key, enabled, minimum size and archives; empty fields come from the defaults */
function overrides(name, title, placeholder, map) {
  const rows = [];
  const body = h("tbody");
  const table = h("table", { class: "table override-table" }, h("thead", {}, h("tr", {}, h("th", { text: "Folder" }), h("th", { text: "Enabled" }), h("th", { text: "Min size (MB)" }), h("th", { text: "Archives" }), h("th"))), body);
  const none = h("p", { class: "muted", text: "None: each uses the settings above." });
  const sync = () => {
    table.hidden = !rows.length;
    none.hidden = rows.length > 0;
  };
  const select = (value, label) =>
    h("select", { class: "input", "aria-label": label }, TRISTATE.map(([v, t]) => h("option", { value: v, selected: String(value ?? "") === v, text: t })));
  const add = (key = "", o = {}) => {
    const r = {
      key: h("input", { class: "input", value: key, placeholder, "aria-label": "Folder", autocomplete: "off", spellcheck: "false" }),
      enabled: select(o.enabled, "Enabled"),
      min: h("input", { class: "input", type: "number", min: 0, max: 100000, step: 1, value: o.min_size_mb ?? "", placeholder: "Default", "aria-label": "Minimum size (MB)" }),
      archives: select(o.allow_archives, "Allow archives"),
    };
    const remove = () => {
      rows.splice(rows.indexOf(r), 1);
      tr.remove();
      sync();
    };
    const tr = h(
      "tr",
      {},
      h("td", { "data-label": "Folder" }, r.key),
      h("td", { "data-label": "Enabled" }, r.enabled),
      h("td", { "data-label": "Min size (MB)" }, r.min),
      h("td", { "data-label": "Archives" }, r.archives),
      h("td", { class: "actions-cell" }, h("button", { type: "button", class: "btn btn-small", "aria-label": `Remove ${key || "this row"}`, onclick: remove }, "Remove")),
    );
    rows.push(r);
    body.append(tr);
    sync();
    return r;
  };
  for (const [key, o] of Object.entries(map || {})) add(key, o);
  sync();
  const flag = (sel) => (sel.value === "" ? undefined : sel.value === "true");
  const node = h(
    "div",
    { class: "spaced", dataset: { overrides: name } },
    h("h3", { class: "sub", text: title }),
    table,
    none,
    h("button", { type: "button", class: "btn btn-small spaced", onclick: () => add().key.focus() }, "Add"),
  );
  const read = () => {
    const out = {};
    for (const r of rows) {
      const key = r.key.value.trim();
      if (!key) continue;
      const min = r.min.value.trim();
      out[key] = { enabled: flag(r.enabled), min_size_mb: min === "" ? undefined : Number(min), allow_archives: flag(r.archives) };
    }
    return out;
  };
  return { node, read };
}

/** What the check does per category and watch folder (FR-020), as saved */
function effectiveTable(rows) {
  const wanted = (r) => {
    if (r.wanted == null) return "From the upload folder's rule";
    const list = r.wanted.slice(0, 6).join(", ") + (r.wanted.length > 6 ? ", …" : "");
    return `${list} (${r.wanted_from ? `rule on ${r.wanted_from}` : "video default"})`;
  };
  return h(
    "div",
    { class: "spaced" },
    h("h3", { class: "sub", text: "What gets checked" }),
    rows.length
      ? h(
          "table",
          { class: "table" },
          h("thead", {}, h("tr", {}, h("th", { text: "Folder" }), h("th", { text: "Checked" }), h("th", { class: "right", text: "Min size" }), h("th", { text: "Archives" }), h("th", { text: "Wanted types" }))),
          h(
            "tbody",
            {},
            rows.map((r) =>
              h(
                "tr",
                {},
                h("td", { class: "name-cell name", "data-label": "Folder" }, r.key, h("small", { text: r.kind === "watch" ? " watch folder" : " category" })),
                h("td", { "data-label": "Checked", text: r.enabled ? "Yes" : "No" }),
                h("td", { class: "num right nowrap", "data-label": "Min size", text: `${r.min_size_mb} MB` }),
                h("td", { "data-label": "Archives", text: r.allow_archives ? "Allowed" : "No" }),
                h("td", { "data-label": "Wanted types", text: wanted(r) }),
              ),
            ),
          ),
        )
      : h("p", { class: "muted", text: "No categories or watch folders yet." }),
    h("p", { class: "muted spaced", text: "As saved; changes apply after a restart." }),
  );
}
