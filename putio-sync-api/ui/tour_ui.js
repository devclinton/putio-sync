// The tour card (feature 014): a small panel that walks through a page's steps, loaded the
// first time a page with an unseen tour opens. It never covers the page with a backdrop:
// everything under it stays usable, and Esc or Skip ends it. At phone width it is a sheet
// above the tab bar. tour.js decides when and where; tour_content.js holds the words.

import { h } from "./app.js";
import { TOURS } from "./tour_content.js";
import { markSeen, move, place, readSeen, store } from "./tour.js";

const WAIT_MS = 2000;
let live = null;
let seq = 0;

const blocked = () => document.body.classList.contains("signed-out") || Boolean(document.querySelector("dialog[open]"));
const visible = (el) => Boolean(el && el.getClientRects().length);
const find = (selector) => [...document.querySelectorAll(selector)].find(visible) || null;

/** End the tour that is showing, if any */
export function stop() {
  seq += 1;
  if (!live) return;
  const { card, off, target } = live;
  live = null;
  off();
  if (target) target.classList.remove("tour-target");
  const had = card.contains(document.activeElement);
  card.remove();
  if (had) document.getElementById("view").focus({ preventScroll: true });
}

/** Resolves once `selector` is on the page, or after WAIT_MS: pages fill in as data arrives */
function ready(selector, mine) {
  const started = Date.now();
  return new Promise((done) => {
    const look = () => (mine !== seq || find(selector) || Date.now() - started > WAIT_MS ? done() : setTimeout(look, 100));
    look();
  });
}

/**
 * Show `page`'s tour unless it was seen in this browser; `force` shows it anyway and moves
 * focus to it (the user asked). Resolves true when the tour is shown.
 */
export async function start(page, { force = false } = {}) {
  stop();
  const steps = TOURS[page];
  if (!steps || (!force && readSeen(store())[page])) return false;
  const mine = seq;
  await ready(steps[0].target, mine);
  if (mine !== seq || blocked()) return false;
  markSeen(store(), [page]);

  let index = 0;
  const count = h("p", { class: "muted small tour-count" });
  const title = h("h2", { id: "tour-title" });
  const text = h("p", { id: "tour-text" });
  const back = h("button", { type: "button", class: "btn btn-small", onclick: () => go(-1) }, "Back");
  const next = h("button", { type: "button", class: "btn btn-small btn-primary", onclick: () => go(1) });
  const skip = h("button", { type: "button", class: "btn btn-small btn-quiet", onclick: stop }, "Skip");
  const skipAll = h("button", { type: "button", class: "link small", onclick: () => (markSeen(store(), Object.keys(TOURS)), stop()) }, "Don't show tours");
  const card = h(
    "section",
    { class: "tour", role: "dialog", "aria-modal": "false", "aria-labelledby": "tour-title", "aria-describedby": "tour-text", tabindex: "-1" },
    count,
    title,
    h("div", { "aria-live": "polite" }, text),
    h("div", { class: "tour-actions" }, skip, back, next),
    skipAll,
  );

  const position = () => {
    if (!live) return;
    const t = live.target && visible(live.target) ? live.target.getBoundingClientRect() : null;
    const at = place(t, { width: card.offsetWidth, height: card.offsetHeight }, { width: window.innerWidth, height: window.innerHeight });
    card.classList.toggle("sheet", at.sheet);
    card.style.cssText = at.sheet ? "" : `top: ${Math.round(at.top)}px; left: ${Math.round(at.left)}px;`;
  };
  const render = () => {
    const step = steps[index];
    if (live.target) live.target.classList.remove("tour-target");
    live.target = find(step.target);
    if (live.target) {
      live.target.classList.add("tour-target");
      live.target.scrollIntoView({ block: "nearest" });
    }
    count.textContent = `Step ${index + 1} of ${steps.length}`;
    title.textContent = step.title;
    text.textContent = step.text;
    back.hidden = index === 0;
    next.textContent = index === steps.length - 1 ? "Done" : "Next";
    position();
  };
  function go(by) {
    const to = move(index, steps.length, by);
    if (to == null) return stop();
    index = to;
    render();
  }
  const key = (ev) => {
    if (ev.key !== "Escape" || ev.defaultPrevented || document.querySelector("dialog[open]")) return;
    stop();
  };
  document.addEventListener("keydown", key);
  window.addEventListener("resize", position);
  window.addEventListener("scroll", position, { capture: true, passive: true });
  const off = () => {
    document.removeEventListener("keydown", key);
    window.removeEventListener("resize", position);
    window.removeEventListener("scroll", position, { capture: true });
  };

  live = { card, off, target: null };
  document.body.append(card);
  render();
  // Announce it and put its buttons next in the tab order, unless the user is already typing
  const at = document.activeElement;
  if (force || !at || at === document.body || at.id === "view") card.focus({ preventScroll: true });
  return true;
}
