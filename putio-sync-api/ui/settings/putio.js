// Put.io section (feature 046): the app ID for linking with a code, storage, cleanup of empty
// folders and the torrent watcher.

import { h } from "../app.js";
import { groupCard } from "./form.js";

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
    ],
    advanced: ["run_at"],
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
