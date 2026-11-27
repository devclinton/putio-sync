// Put.io section (feature 046): the app ID for linking with a code, storage, cleanup of empty
// folders and the torrent watcher.

import { post } from "../api.js";
import { h } from "../app.js";
import { budgetLocked, budgetPreviewLines } from "../download_limits.js";
import { bytes } from "../format.js";
import { groupCard, quiet, reason } from "./form.js";

const APP_ID_FROM = { environment: "the environment (PUTIO_SYNC_APP_ID)", config: "this setting", built_in: "this build" };

const GROUPS = [
  {
    id: "putio",
    key: "putio",
    title: "Put.io",
    fields: [["app_id", "Put.io app ID", "Turns on linking with a code at put.io/link; empty for none", "s?"]],
    extra: putioExtra,
    locked: (data) => (data.status && data.status.env_override ? ["app_id"] : []),
  },
  {
    id: "storage",
    key: "storage",
    title: "Put.io storage",
    fields: [
      ["warn_free_percent", "Warn below (% free)", "Warn on the Put.io and Status pages and in the doctor; 1 to 89", "n", 1, 89],
      ["target_free_percent", "Free up to (% free)", "Free up space deletes old downloaded files until this is free; up to 90", "n", 2, 90],
      ["run_at", "Daily run at", "24-hour local time, HH:MM", "s"],
      ["auto_free", "Automatic free-up", "Once a day, free up space when below the warning level (daemon only)", "b"],
      ["budget_used_percent", "Storage budget (% used)", "The share of the account the storage budget keeps it under; 10 to 99", "n", 10, 99],
    ],
    advanced: ["run_at"],
    owns: ["budget_enabled"],
    extra: budgetExtra,
  },
  {
    id: "cleanup",
    key: "cleanup",
    title: "Put.io cleanup",
    fields: [
      ["run_at", "Daily run at", "24-hour local time, HH:MM", "s"],
      ["min_age_hours", "Minimum age (hours)", "Keep empty folders newer than this; 0 to 8760", "n", 0, 8760],
      ["enabled", "Daily run", "Remove empty Put.io folders once a day (daemon only)", "b"],
      ["nested", "Nested empty folders", "Also remove folders whose sub-folders are all empty", "b"],
      ["keep_patterns", "Keep patterns", "One per line; matching folders and what is inside them are never removed, e.g. Season* or Software/*", "l"],
    ],
    advanced: ["min_age_hours", "nested"],
  },
  {
    id: "watcher",
    key: "watcher",
    title: "Torrent watcher",
    guide: "guides/watch-folders",
    fields: [
      ["paths", "Watched folders", "One per line; .torrent files found here are uploaded", "l"],
      ["refresh_cycle", "Scan interval (s)", "1 to 127", "n", 1, 127],
      ["parent_folder_id", "Put.io folder ID", "Where transfers are saved; empty for the root", "n?", 0],
      ["archive_directory", "Archive folder", "Move uploaded .torrent files here; empty to leave them", "s?"],
      ["enabled", "Enabled", "Watch the folders (each must exist)", "b"],
      ["delete_after_upload", "Delete after upload", "Remove .torrent files once uploaded", "b"],
    ],
    advanced: ["refresh_cycle", "parent_folder_id"],
  },
];

/** The groups whose fields the settings catalog lists (061) */
export const groups = GROUPS;
export const cards = (ctx) => GROUPS.map((g) => groupCard(g, ctx));

/** Where the app ID in use comes from, and where to register one */
function putioExtra(data) {
  const st = data.status || {};
  const register = h("a", { href: st.register_url, target: "_blank", rel: "noopener", text: "Register a Put.io app" });
  const line = st.in_use
    ? `Linking with a code is on, with app ID ${st.in_use} from ${APP_ID_FROM[st.source] || st.source}.`
    : "Linking with a code is off: no app ID is set.";
  const nodes = h(
    "div",
    {},
    h("p", { class: "muted small", text: line }),
    st.env_override ? h("p", { class: "muted small", text: "Set by environment (PUTIO_SYNC_APP_ID), so it can't be changed here." }) : null,
    h("p", { class: "muted small" }, register, " (account menu → API → Create App; any website and callback URL will do), then enter the number Put.io shows for it."),
  );
  return { nodes, read: () => ({}) };
}

const BUDGET_SHOWN = 50;

/**
 * The storage budget's switch (feature 064). It deletes files, so it can be turned on
 * only after a preview of the budget entered, which lists what would go and deletes
 * nothing.
 */
function budgetExtra(data) {
  let previewed = null;
  const percent = () => {
    const input = document.getElementById("set-storage-budget_used_percent");
    return input ? Number(input.value) : data.budget_used_percent;
  };
  const box = h("input", { type: "checkbox", id: "set-storage-budget_enabled", checked: Boolean(data.budget_enabled) });
  const hint = h("small");
  const result = h("div", { class: "spaced", role: "status" });
  // Called on every edit of the card: a budget that was not previewed can't be on
  const sync = () => {
    const locked = budgetLocked(data, percent(), previewed);
    if (locked) box.checked = false;
    box.disabled = locked;
    hint.textContent = locked
      ? "Preview this budget first; then it can be turned on."
      : "Deletes the oldest files that are confirmed downloaded, about every 15 minutes while Put.io is over the budget (daemon only). Each deletion is logged and listed under Recent runs on the Put.io page.";
    return box.checked;
  };
  const preview = h("button", { type: "button", class: "btn btn-small" }, "Preview what it would delete");
  preview.addEventListener("click", async () => {
    const p = percent();
    preview.disabled = true;
    try {
      const plan = await post("/putio/storage/budget/preview", { used_percent: p });
      previewed = p;
      const more = plan.files.length - BUDGET_SHOWN;
      const lines = [
        ...budgetPreviewLines(plan, p).map((text) => h("p", { text })),
        plan.complete ? null : h("p", { class: "warn-text", text: "Put.io could not be read in full, so more files may qualify." }),
        plan.files.length ? h("ul", { class: "paths" }, plan.files.slice(0, BUDGET_SHOWN).map((f) => h("li", { text: `${f.path} (${bytes(f.size)}) → kept at ${f.local_path}` }))) : null,
        more > 0 ? h("p", { class: "muted", text: `…and ${more} more` }) : null,
        h("p", { class: "muted small", text: "Nothing was deleted. A file not confirmed downloaded here is never deleted." }),
      ].filter(Boolean);
      result.replaceChildren(...lines);
    } catch (e) {
      if (!quiet(e)) result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    } finally {
      preview.disabled = false;
      sync();
      box.closest("form")?.dispatchEvent(new Event("change"));
    }
  });
  sync();
  const nodes = h(
    "div",
    { class: "spaced" },
    h("h3", { class: "sub", text: "Storage budget" }),
    h("p", { class: "muted", text: "Keeps Put.io under the budget above by deleting the oldest finished files that are already downloaded, by the same rules as Free up space. Off unless you turn it on." }),
    h("label", { class: "field check", for: "set-storage-budget_enabled" }, box, h("span", {}, "Keep Put.io under the storage budget", hint)),
    h("div", { class: "toolbar" }, preview),
    result,
  );
  return { nodes, read: () => ({ budget_enabled: sync() }) };
}
