// Feature 070: the file picker of one transfer, a dialog with its files as a tree of
// folders, each with a tick box and its size. Opened from the Transfers, Active and Queue
// pages for a transfer Put.io already lists (`chooseFiles`), and from the add form for a
// `.torrent` before it is added (`pickFromList`). The logic is pick.js.
//
// Every control is a native checkbox or button, so the tree works from the keyboard and
// reads out in a screen reader: a folder's box is "mixed" when some of its files are
// ticked, and its arrow says whether the folder is open.

import { del, get, put } from "./api.js";
import { h, toast, toastError } from "./app.js";
import { bytes } from "./format.js";
import { allBut, folderState, onlyThis, setAll, setFolder, stateNote, summary, ticksOf, toChoice, tree } from "./pick.js";

let serial = 0;

/**
 * The tree, its buttons and the count of what is ticked. `{ el, choice() }`: the node to
 * show, and the choice as the API takes it.
 */
function picker(files, { fresh, later }) {
  let ticks = ticksOf(files, fresh);
  const root = tree(files);
  const boxes = []; // [file, checkbox, note]
  const folders = []; // [dir, checkbox]
  const count = h("p", { class: "muted pick-count", role: "status" });
  const laterBox = h("input", { type: "checkbox" });
  laterBox.checked = later;

  function sync() {
    for (const [file, box, note] of boxes) {
      box.checked = ticks.get(file.path) === true;
      note.textContent = stateNote(file.state, box.checked) || "";
    }
    for (const [dir, box] of folders) {
      const state = folderState(dir, ticks);
      box.checked = state === "all";
      box.indeterminate = state === "some";
    }
    const s = summary(files, ticks);
    count.textContent = `Chosen: ${s.count} of ${s.total} files · ${bytes(s.size)} of ${bytes(s.totalSize)}`;
  }
  const set = (next) => {
    ticks = next;
    sync();
  };

  function fileRow(file) {
    const box = h("input", { type: "checkbox", onchange: (ev) => set(new Map(ticks).set(file.path, ev.currentTarget.checked)) });
    const note = h("small", { class: "muted block" });
    boxes.push([file, box, note]);
    return h(
      "li",
      { class: "pick-row" },
      h("label", { class: "pick-name" }, box, h("span", { class: "name" }, file.name, note)),
      h("span", { class: "num muted", text: bytes(file.size) }),
      files.length > 1
        ? h(
            "span",
            { class: "pick-quick" },
            h("button", { type: "button", class: "btn btn-small", "aria-label": `Download only ${file.name}`, onclick: () => set(onlyThis(ticks, file.path)) }, "Only this"),
            h("button", { type: "button", class: "btn btn-small", "aria-label": `Download all but ${file.name}`, onclick: () => set(allBut(ticks, file.path)) }, "All but this"),
          )
        : null,
    );
  }

  function folderRow(dir) {
    serial += 1;
    const id = `pick-folder-${serial}`;
    const box = h("input", { type: "checkbox", onchange: (ev) => set(setFolder(ticks, dir, ev.currentTarget.checked)) });
    folders.push([dir, box]);
    const inside = h("ul", { class: "pick-list", id, "aria-label": dir.name }, dir.dirs.map(folderRow), dir.files.map(fileRow));
    const arrow = h("button", { type: "button", class: "btn btn-icon pick-arrow", "aria-expanded": "true", "aria-controls": id, "aria-label": `Show or hide the files in ${dir.name}`, text: "▾" });
    arrow.addEventListener("click", () => {
      inside.hidden = !inside.hidden;
      arrow.setAttribute("aria-expanded", String(!inside.hidden));
      arrow.textContent = inside.hidden ? "▸" : "▾";
    });
    return h(
      "li",
      { class: "pick-folder" },
      h("div", { class: "pick-row" }, arrow, h("label", { class: "pick-name" }, box, h("strong", { class: "name", text: dir.name }))),
      inside,
    );
  }

  const el = h(
    "div",
    { class: "pick" },
    h(
      "div",
      { class: "toolbar pick-bar" },
      h("button", { type: "button", class: "btn btn-small", onclick: () => set(setAll(ticks, true)) }, "Select all"),
      h("button", { type: "button", class: "btn btn-small", onclick: () => set(setAll(ticks, false)) }, "Select none"),
    ),
    count,
    h("ul", { class: "pick-list pick-tree", "aria-label": "Files of this transfer" }, root.dirs.map(folderRow), root.files.map(fileRow)),
    h("label", { class: "pick-later" }, laterBox, h("span", { text: "Also download files that show up in this transfer later" })),
    h("p", { class: "muted", text: "Your choice is for this transfer only and wins over the folder rules and filters. Files you leave out stay on Put.io." }),
  );
  sync();
  return { el, choice: () => toChoice(files, ticks, laterBox.checked) };
}

/**
 * Show a dialog. `buttons` are `[label, value, primary]`; resolves with the value of the
 * one pressed, or "cancel" for Cancel and Escape.
 */
function show(name, body, buttons) {
  const dialog = h(
    "dialog",
    { class: "pick-panel", "aria-label": "Choose files" },
    h(
      "form",
      { method: "dialog" },
      h("h2", { text: "Choose files" }),
      h("p", { class: "name", text: name }),
      body,
      h(
        "div",
        { class: "dialog-actions" },
        buttons.map(([label, value, primary]) => h("button", { class: primary ? "btn btn-primary" : "btn", value }, label)),
      ),
    ),
  );
  dialog.returnValue = "cancel";
  document.body.append(dialog);
  return new Promise((resolve) => {
    dialog.addEventListener(
      "close",
      () => {
        resolve(dialog.returnValue || "cancel");
        dialog.remove();
      },
      { once: true },
    );
    dialog.showModal();
    const first = dialog.querySelector(".pick button, .dialog-actions button");
    if (first) first.focus();
  });
}

/**
 * Pick from a list of files that is not a transfer yet (a `.torrent`'s). Resolves with
 * the choice for `POST /putio/transfers`, or null when cancelled.
 */
export async function pickFromList({ name, files }) {
  const p = picker(files, { fresh: true, later: true });
  const answer = await show(name, p.el, [
    ["Cancel", "cancel"],
    ["Add transfer", "ok", true],
  ]);
  return answer === "ok" ? p.choice() : null;
}

/**
 * Choose the files of transfer `id`, save the choice and tell `onDone`. A transfer whose
 * files Put.io has not listed yet can be held until they are picked.
 */
export async function chooseFiles({ id, name, onDone = () => {} }) {
  let data;
  try {
    data = await get(`/putio/transfers/${id}/files`);
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toastError(e);
    return;
  }
  const title = data.name || name || "";
  const chosen = data.choice || null;
  const forget = chosen ? [["Let the rules decide", "forget"]] : [];
  const path = `/putio/transfers/${id}/choice`;
  try {
    if (!data.files || data.files.length === 0) {
      const held = Boolean(chosen && chosen.pending);
      const text = held
        ? "Put.io has not listed this transfer's files yet. Nothing of it downloads until you choose; open this again when Put.io has the files."
        : "Put.io has not listed this transfer's files yet. You can hold it, so nothing of it downloads until you choose.";
      const answer = await show(title, h("p", { text }), [...forget, ["Close", "cancel"], ...(held ? [] : [["Download nothing until I choose", "hold", true]])]);
      if (answer === "hold") {
        await put(path, { others: "skip", pending: true });
        toast("Held: nothing of this transfer downloads until you choose its files");
      } else if (answer === "forget") {
        await del(path);
        toast("The folder rules decide for this transfer again");
      } else return;
    } else {
      const fresh = !chosen || chosen.pending;
      const p = picker(data.files, { fresh, later: fresh || chosen.others === "download" });
      const answer = await show(title, p.el, [...forget, ["Cancel", "cancel"], ["Save", "ok", true]]);
      if (answer === "ok") {
        const saved = await put(path, p.choice());
        const n = (saved && saved.removed) || 0;
        toast("Saved your choice of files", "ok", n === 1 ? "Taken out of the queue: 1 file." : n > 1 ? `Taken out of the queue: ${n} files.` : null);
      } else if (answer === "forget") {
        await del(path);
        toast("The folder rules decide for this transfer again");
      } else return;
    }
  } catch (e) {
    if (e.status !== 401 && e.status !== 429) toastError(e);
  }
  onDone();
}

/** The "Choose files" button of a row; null when the row has no transfer */
export function chooseButton(id, name, onDone) {
  if (id == null) return null;
  return h("button", { type: "button", class: "btn btn-small", "aria-label": `Choose files of ${name}`, onclick: () => chooseFiles({ id, name, onDone }) }, "Choose files");
}
