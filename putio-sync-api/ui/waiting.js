// Feature 066: the one line that says why a row is not downloading. The API sends a
// stable code and its fields (`waiting` on Active items and queue rows); this turns it
// into words, a link to the setting behind it and, for an error, the guide to read.
// Plain functions with no DOM, so tests/ui/waiting.test.mjs can run them under
// `node --test`. A new code needs a case in `waitingLine`; an unknown one shows nothing.

import { bytes, relative } from "./format.js";
import { GUIDES } from "./guides.js";
import { fieldId, sectionOfKey, settingsHref } from "./settings/sections.js";

// Config key → the name of its field in Settings, and where the link lands when the key
// is edited in a card of its own
const SETTINGS = {
  "download.bandwidth_schedule": ["Bandwidth schedule", "card-bandwidth"],
  "download.bandwidth_limit_bytes_per_sec": ["Bandwidth limit (bytes/s)"],
  "download.min_free_space_mb": ["Minimum free space (MB)"],
  "download.target_directory": ["Download folder"],
  "download.workers": ["Workers"],
};

const guide = (name) => GUIDES.find((g) => g.slug === `guides/${name}`) || null;

/** `{label, href}` for a config key the API named, or null for one this UI doesn't know */
export function settingLink(key) {
  const known = SETTINGS[key];
  if (!known) return null;
  const [label, target] = known;
  return { label, href: settingsHref(sectionOfKey(key), target || fieldId(key)) };
}

/** "2026-11-28T06:00" → "2026-11-28 06:00": the daemon's wall clock, shown as sent */
const clock = (local) => String(local).replace("T", " ");

function space(w) {
  return [bytes(w.free_bytes), bytes(w.min_free_bytes)];
}

// What failed and what to do about it, by `kind`
const FAILED = {
  network: "Failed: the connection dropped and the retries ran out. Retry it; if it fails again, check the network.",
  http: "Failed: Put.io answered with an error. Retry it; if it fails again, the log has the status code.",
  io: "Failed: the file could not be written. Check the download folder and its permissions, then retry it.",
  disk_full: "Failed: the disk is full. Free some space, then retry it.",
  integrity: "Failed: the file did not match its size or checksum. Retry it to download it again.",
};

/**
 * The line for a `waiting` object: `{ text, tone, link, guide }`, or null for none or a
 * code this UI doesn't know. `tone` is "muted", "warn" or "error"; `link` is
 * `{label, href}` to the setting or page behind the reason; `guide` is a GUIDES row.
 */
export function waitingLine(w) {
  if (!w || !w.code) return null;
  const line = (text, tone = "muted", link = settingLink(w.setting), g = null) => ({ text, tone, link, guide: g });
  switch (w.code) {
    case "bandwidth_pause":
      return line(w.until ? `All downloads are paused; they start again ${relative(w.until)}.` : "All downloads are paused.", "warn", { label: "Status", href: "#/status" });
    case "bandwidth_schedule": {
      const entry = w.entry || 0;
      return line(w.until_local ? `Paused by bandwidth schedule entry ${entry} until ${clock(w.until_local)} (the daemon's clock).` : `Paused by bandwidth schedule entry ${entry}.`, "warn");
    }
    case "bandwidth_limit":
      return line("Paused: the bandwidth limit is set to paused.", "warn");
    case "downloads_paused":
      return line("Local downloads are paused; Resume local downloads starts them again.", "warn", null);
    case "disk_space": {
      const [free, min] = space(w);
      return line(`Waiting for disk space: ${free} free, ${min} must stay free.`, "warn");
    }
    case "disk_unreadable":
      return line("Waiting: the free space of the download folder can't be read, so nothing starts.", "warn", settingLink(w.setting), guide("troubleshooting"));
    case "file_too_big": {
      const [free, min] = space(w);
      return line(`Too big for the space left: ${free} free, ${min} must stay free. Smaller files go first.`, "warn");
    }
    case "waiting_turn":
      return line(w.workers === 1 ? "Waiting its turn: 1 download runs at a time." : `Waiting its turn: ${w.workers} downloads run at a time.`);
    case "paused":
      return line("Paused; Resume puts it back in the queue.");
    case "held":
      return line("Put.io keeps downloading; local download held");
    case "putio_queue":
      return line("Waiting in Put.io's queue; Put.io has not started it yet.");
    case "waiting_for_sync":
      return line(w.until ? `Done on Put.io; the next sync ${relative(w.until)} picks it up` : "Done on Put.io; the next sync picks it up");
    case "choose_files":
      return line("Waiting for you to choose its files; nothing of it downloads until then.", "warn", null);
    case "not_chosen":
      return line("Left out by your choice of files; it stays on Put.io.", "muted", null);
    case "failed":
      return line(FAILED[w.kind] || "Failed. Retry it; the log says what went wrong.", "error", w.kind === "io" ? settingLink("download.target_directory") : w.kind === "disk_full" ? settingLink("download.min_free_space_mb") : null, guide("troubleshooting"));
    default:
      return null;
  }
}

/**
 * The line for an Active item's skipped files (`files.skipped`): they have no queue row,
 * so the API cannot say which rule left each one out. Null when none was skipped. For a
 * transfer whose files were picked by hand (`chosen`, feature 070) the choice left them out.
 */
export function skippedLine(files, chosen = false) {
  const n = files ? Number(files.skipped) || 0 : 0;
  if (n < 1) return null;
  if (chosen) {
    return { text: n === 1 ? "Left out by your choice of files: 1 file. It stays on Put.io." : `Left out by your choice of files: ${n} files. They stay on Put.io.`, tone: "muted", link: null, guide: null };
  }
  return {
    text: n === 1 ? "Left out by a filter or folder rule, or removed from the queue: 1 file." : `Left out by a filter or folder rule, or removed from the queue: ${n} files.`,
    tone: "muted",
    link: { label: "Rules", href: settingsHref("rules") },
    guide: guide("folder-rules"),
  };
}

/** The line under a transfer Put.io reports as failed: what to do, and the guide */
export function putioErrorLine(item) {
  if (!item || item.stage !== "putio" || item.state !== "error") return null;
  return { text: "Put.io could not finish this transfer. Retry or remove it on the Transfers page.", tone: "muted", link: null, guide: guide("troubleshooting") };
}
