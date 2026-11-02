// Release flags (feature 042): mark a release fake, or a rejection not fake, from its row,
// and undo it. Pages load the flags for their rows with one loadFlags() per render and put
// flagControl() in each row's actions.

import { del, get, post } from "./api.js";
import { confirm, h, toast, toastError } from "./app.js";

const MAX_TARGETS = 200;
const MAX_NOTE = 500;

/** "file_id:31" for `{file_id: 31}` */
const keyOf = (target) => Object.entries(target).map(([k, v]) => `${k}:${v}`)[0];

/**
 * The flags on `targets` (`{file_id: 31}`, `{history_id: 4}`, …), in one request.
 * Resolves with `{flags: Map(key → flag), arr: Set(file ids an *arr item holds)}`; a failed
 * request gives none, so rows still render.
 */
export async function loadFlags(targets) {
  const out = { flags: new Map(), arr: new Set() };
  if (!targets.length) return out;
  const q = new URLSearchParams();
  for (const t of targets.slice(0, MAX_TARGETS)) for (const [k, v] of Object.entries(t)) q.append(k, v);
  try {
    const res = await get(`/releases/flags?${q}`);
    for (const f of res.flags || []) out.flags.set(keyOf(f.target), f);
    out.arr = new Set(res.arr_file_ids || []);
  } catch {
    // Flags are optional on every page
  }
  return out;
}

/**
 * The row's flag control: a badge with Undo when flagged, else a button that opens the
 * dialog. `rejected` rows offer "Not fake"; others offer "Fake". `onChange` runs after a
 * flag is saved or undone.
 */
export function flagControl(state, { target, name, rejected = false, onChange }) {
  const flag = state && state.flags.get(keyOf(target));
  if (flag) {
    const fake = flag.kind === "flagged_fake";
    return [
      h("span", { class: `badge ${fake ? "bad" : "ok"}`, title: flag.note || null, text: fake ? "flagged fake" : "not fake" }),
      h("button", { type: "button", class: "btn btn-small", "aria-label": `Undo the flag on ${name}`, onclick: (ev) => undo(ev.currentTarget, flag, name, onChange) }, "Undo"),
    ];
  }
  const arr = target.arr_hash != null || Boolean(state && state.arr.has(target.file_id));
  const label = rejected ? "Not fake" : "Flag";
  return h("button", { type: "button", class: "btn btn-small", "aria-label": `${label}: ${name}`, onclick: (ev) => open(ev.currentTarget, { target, name, rejected, arr, onChange }) }, label);
}

async function open(button, { target, name, rejected, arr, onChange }) {
  const choice = await ask(name, rejected, arr);
  if (!choice) return;
  button.disabled = true;
  try {
    const res = await post("/releases/flags", { target, kind: rejected ? "not_fake" : "fake", ...choice });
    if (res.dry_run) toast(`Flagged ${name} as fake`, "ok", "Dry run: it wasn't failed or deleted.");
    else if (res.blocklisted) toast(`Flagged ${name} as fake`, "ok", "It's failed and will be blocklisted.");
    else toast(rejected ? `Marked ${name} as not fake` : `Flagged ${name} as fake`);
    onChange();
  } catch (e) {
    button.disabled = false;
    toastError(e);
  }
}

async function undo(button, flag, name, onChange) {
  const ok = await confirm({
    title: "Undo the flag?",
    text: flag.kind === "flagged_fake" ? `${name} won't be flagged fake any more. A fail or blocklist already sent to Radarr or Sonarr isn't undone.` : `${name} won't be marked not fake any more.`,
    verb: "Undo",
  });
  if (!ok) return;
  button.disabled = true;
  try {
    await del(`/releases/flags/${flag.id}`);
    toast(`Removed the flag on ${name}`);
    onChange();
  } catch (e) {
    button.disabled = false;
    toastError(e);
  }
}

/** The flag dialog. Resolves with `{note, blocklist}`, or null when cancelled. */
function ask(name, rejected, arr) {
  const note = h("textarea", { class: "input", rows: "3", maxlength: String(MAX_NOTE), placeholder: rejected ? "What it really was (optional)" : "What gave it away (optional)" });
  const blocklist = arr ? h("input", { type: "checkbox" }) : null;
  const flag = h("button", { class: "btn btn-primary", value: "ok" }, rejected ? "Not fake" : "Flag fake");
  const dialog = h(
    "dialog",
    { "aria-label": rejected ? "Mark not fake" : "Flag fake" },
    h(
      "form",
      { method: "dialog", class: "flag-form" },
      h("h2", { text: rejected ? "Not fake?" : "Flag as fake?" }),
      h("p", { class: "name", text: name }),
      h("label", { class: "field" }, h("span", { text: "Note" }), note),
      arr
        ? h(
            "label",
            { class: "field check" },
            blocklist,
            h("span", {}, "Also fail and blocklist it", h("small", { text: "Its files are deleted from Put.io, and Radarr or Sonarr sees it failed and blocklists it." })),
          )
        : null,
      h("div", { class: "dialog-actions" }, h("button", { class: "btn", value: "cancel" }, "Cancel"), flag),
    ),
  );
  dialog.returnValue = "cancel";
  document.body.append(dialog);
  return new Promise((resolve) => {
    dialog.addEventListener(
      "close",
      () => {
        const text = note.value.trim();
        resolve(dialog.returnValue === "ok" ? { note: text || null, blocklist: Boolean(blocklist && blocklist.checked) } : null);
        dialog.remove();
      },
      { once: true },
    );
    dialog.showModal();
    note.focus();
  });
}
