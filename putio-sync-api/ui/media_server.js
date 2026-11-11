// Media server settings (feature 022): the shared-folder checkboxes, the Put.io folder
// picker and a status line polled every 5 s while the section is open. The group's plain
// fields (on/off, name, port, addresses) are built by the form; this is its `extra`.

import { get } from "./api.js";
import { fill, h } from "./app.js";
import { pickRoots, statusText } from "./media_text.js";

const POLL_MS = 5000;
const reason = (e) => (e && (e.error || e.message)) || String(e);

const STATES = {
  off: ["", "Off"],
  starting: ["warn", "Starting"],
  running: ["ok", "Running"],
  failed: ["bad", "Failed"],
};

/** The group's `extra`: sends `roots` and `putio_folder` */
export function mediaServerExtra(data, { rebase } = {}) {
  const checked = new Set(data.roots || []);
  let folder = data.putio_folder ?? null;
  const box = h("div", { class: "media-server" });
  const status = h("p", { class: "muted", role: "status", text: "Asking the media server…" });
  const folders = h("div", { class: "fields" });
  const putioRow = h("div", { class: "spaced", hidden: !checked.has("putio") });
  const changed = () => box.dispatchEvent(new Event("change", { bubbles: true }));

  const available = data.available_roots || [];
  for (const r of available) {
    const id = `media-root-${r.key.replace(/[^a-z0-9]+/gi, "-")}`;
    const input = h("input", { type: "checkbox", id, checked: checked.has(r.key) });
    input.addEventListener("change", () => {
      if (input.checked) checked.add(r.key);
      else checked.delete(r.key);
      if (r.key === "putio") putioRow.hidden = !input.checked;
      changed();
    });
    folders.append(h("label", { class: "field check", for: id }, input, h("span", {}, r.title, r.path ? h("small", { text: r.path }) : null)));
  }

  // Put.io folder picker over the existing Put.io browse route
  const chosen = h("span", { class: "muted" });
  const showChosen = () => (chosen.textContent = folder == null ? "No folder chosen: the synced folder, all your files" : `Put.io folder ${folder}`);
  const picker = h("div", { class: "spaced", hidden: true });
  const open = h("button", { type: "button", class: "btn btn-small", onclick: () => browse(0, "Your files") }, "Choose folder");
  async function browse(id, name) {
    picker.hidden = false;
    picker.replaceChildren(h("p", { class: "muted", text: "Loading…" }));
    let list;
    try {
      list = await get(`/putio/files?parent_id=${id}`);
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      picker.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
      return;
    }
    const subs = (list.items || []).filter((f) => f.file_type === "FOLDER");
    picker.replaceChildren(
      h("p", { class: "muted small", text: name }),
      h(
        "ul",
        { class: "tailscale-urls" },
        subs.map((f) => h("li", {}, h("button", { type: "button", class: "btn btn-small", onclick: () => browse(f.id, f.name) }, f.name))),
      ),
      h(
        "button",
        {
          type: "button",
          class: "btn btn-small btn-primary",
          onclick: () => {
            folder = id;
            showChosen();
            picker.hidden = true;
            changed();
          },
        },
        id === 0 ? "Share everything" : `Share "${name}"`,
      ),
    );
  }
  showChosen();
  putioRow.append(h("div", { class: "toolbar" }, h("span", { text: "Put.io folder: " }), chosen, open), picker);

  fill(
    box,
    h("h3", { text: "Shared folders" }),
    folders,
    putioRow,
    h("p", { class: "warn-text", text: "Anyone on your home network can browse and play what you share here." }),
    status,
  );

  // Poll while this card is on the page
  let timer = 0;
  const poll = async () => {
    if (!box.isConnected) return void clearInterval(timer);
    try {
      const s = await get("/media-server/status");
      const [tone, label] = STATES[s.state] || ["", s.state];
      const line = statusText(s);
      status.replaceChildren(h("span", { class: `badge ${tone}`, text: label }), line ? ` ${line}` : "", s.reason ? h("small", { class: s.state === "failed" ? "error-text" : "muted", text: ` ${s.reason}` }) : null);
    } catch (e) {
      if (e.status !== 401 && e.status !== 429) status.textContent = `Status unavailable: ${reason(e)}`;
    }
  };
  poll();
  timer = setInterval(poll, POLL_MS);
  void rebase;

  return { nodes: box, read: () => ({ roots: pickRoots(available, checked), putio_folder: folder }) };
}
