// Organise on Put.io (features 023 and 024): the settings and the planned, applied and failed moves
// of files stream mode keeps on Put.io. Loaded on demand from the Put.io page's Organise
// button, so it does not count towards the page's first load.

import { get, post, put } from "./api.js";
import { h, toast, toastError } from "./app.js";
import { ACTIONS, idsFor, outcomeText, pathOf, sortMoves } from "./organise_text.js";

const LABELS = { planned: "Planned", applied: "Moved", undone: "Undone", failed: "Failed" };
const KINDS = { planned: "", applied: "ok", undone: "", failed: "bad" };
const MEDIA = { tv: "TV", movie: "Movie" };

let card = null;

/** Show the panel above the Put.io page's cards, or hide it again */
export function toggle(button) {
  if (card?.isConnected) {
    card.remove();
    card = null;
    return;
  }
  const stack = button.closest(".page-head")?.nextElementSibling;
  if (!stack) return;
  card = h("section", { class: "card wide", id: "organise" }, h("p", { class: "muted", text: "Loading…" }));
  stack.prepend(card);
  load(card);
}

async function load(el) {
  let settings;
  let moves;
  try {
    [settings, moves] = await Promise.all([get("/settings/organise"), get("/organise")]);
  } catch (e) {
    if (e.status === 401 || e.status === 429) return;
    el.replaceChildren(h("h2", { text: "Organise on Put.io" }), h("p", { class: "error-text", text: e.message || String(e) }));
    return;
  }
  if (el !== card) return;
  el.replaceChildren(
    h("h2", { text: "Organise on Put.io" }),
    h("p", { class: "muted", text: "Files stream mode keeps on Put.io can be moved into Show/Season NN folders named Show - S01E02 - Title, and Movie (Year) folders named Movie (Year), with extras in their own folder and samples left out. Nothing moves until you apply it, unless you choose to move at once." }),
    form(settings, el),
    movesList(moves, el),
  );
}

function form(s, el) {
  const tv = h("input", { type: "checkbox", id: "org-tv", checked: s.tv });
  const folder = h("input", { type: "text", id: "org-folder", value: s.tv_folder, spellcheck: false });
  const movies = h("input", { type: "checkbox", id: "org-movies", checked: s.movies });
  const moviesFolder = h("input", { type: "text", id: "org-movies-folder", value: s.movies_folder, spellcheck: false });
  const auto = h("input", { type: "checkbox", id: "org-auto", checked: s.apply === "auto" });
  const plugins = h("input", { type: "checkbox", id: "org-plugins", checked: s.use_plugins });
  async function save(ev) {
    ev.preventDefault();
    try {
      const res = await put("/settings/organise", { tv: tv.checked, tv_folder: folder.value.trim(), movies: movies.checked, movies_folder: moviesFolder.value.trim(), apply: auto.checked ? "auto" : "preview", use_plugins: plugins.checked });
      toast(res.message);
      load(el);
    } catch (e) {
      toastError(e);
    }
  }
  return h(
    "form",
    { class: "fields", onsubmit: save },
    h("label", { class: "field check", for: "org-tv" }, tv, h("span", {}, "Organise TV shows")),
    h("label", { class: "field", for: "org-folder" }, h("span", { text: "TV folder on Put.io" }), folder),
    h("label", { class: "field check", for: "org-movies" }, movies, h("span", {}, "Organise movies")),
    h("label", { class: "field", for: "org-movies-folder" }, h("span", { text: "Movies folder on Put.io" }), moviesFolder),
    h("label", { class: "field check", for: "org-auto" }, auto, h("span", {}, "Move at once", h("small", { text: "Without a preview; every move can still be undone" }))),
    h("label", { class: "field check", for: "org-plugins" }, plugins, h("span", {}, "Let plugins choose folders and names", h("small", { text: "Their folder and rename answers win over the built-in names" }))),
    h("div", {}, h("button", { type: "submit", class: "btn btn-small" }, "Save")),
  );
}

function movesList(body, el) {
  const box = h("div", { class: "spaced" });
  if (!body.available) {
    box.append(h("p", { class: "muted", text: "Moves are made by the daemon; this engine only saves the settings." }));
    return box;
  }
  const moves = sortMoves(body.moves || []);
  const picked = new Set();
  const run = (action, done) => async () => {
    const ids = idsFor(moves, picked, action);
    if (!ids.length) return toast("Tick the moves first", "error");
    try {
      const res = await post(`/organise/${action}`, { file_ids: ids });
      const failed = res.results.some((r) => r.status === "failed");
      toast(outcomeText(res.results, done), failed ? "error" : "ok");
    } catch (e) {
      toastError(e);
    }
    load(el);
  };
  async function scan() {
    try {
      const res = await post("/organise/scan");
      toast(res.planned ? `${res.planned} moves planned` : "No new files found");
    } catch (e) {
      toastError(e);
    }
    load(el);
  }
  box.append(
    h(
      "div",
      { class: "toolbar" },
      h("button", { type: "button", class: "btn btn-small", onclick: run("apply", "applied") }, "Apply"),
      h("button", { type: "button", class: "btn btn-small", onclick: run("undo", "undone") }, "Undo"),
      h("button", { type: "button", class: "btn btn-small", onclick: run("dismiss", "dismissed") }, "Dismiss"),
      h("button", { type: "button", class: "btn btn-small", title: "Plan moves for files streamed before organising was on", onclick: scan }, "Find files"),
    ),
  );
  if (!moves.length) {
    box.append(h("p", { class: "muted", text: "No moves yet. New episodes and movies appear here after the next sync." }));
    return box;
  }
  const rows = moves.map((m) => {
    const tick = h("input", { type: "checkbox", "aria-label": m.from_name });
    tick.addEventListener("change", () => (tick.checked ? picked.add(m.file_id) : picked.delete(m.file_id)));
    tick.disabled = !ACTIONS[m.status];
    return h(
      "tr",
      {},
      h("td", {}, tick),
      h("td", {}, h("span", { class: `badge ${KINDS[m.status] || ""}`, text: LABELS[m.status] || m.status }), m.by_plugin ? h("small", { class: "muted", text: " plugin" }) : null),
      h("td", {}, MEDIA[m.kind] || m.kind),
      h("td", { class: "name" }, pathOf(m.from_folder, m.from_name)),
      h("td", { class: "name" }, pathOf(m.to_folder, m.to_name), m.error ? h("small", { class: "error-text", text: m.error }) : null),
    );
  });
  box.append(
    h(
      "div",
      { class: "table-wrap" },
      h("table", { class: "table" }, h("thead", {}, h("tr", {}, h("th", {}), h("th", { text: "State" }), h("th", { text: "Kind" }), h("th", { text: "Now" }), h("th", { text: "Moves to" }))), h("tbody", {}, rows)),
    ),
  );
  return box;
}
