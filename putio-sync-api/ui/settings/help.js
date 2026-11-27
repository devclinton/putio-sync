// Help for each settings card (feature 014): a ? button next to the card's heading that
// opens a short panel in plain words, with a link to the guide that covers it. The words
// are in tour_content.js. Cards build and rebuild themselves as they load, so the buttons
// are added by watching the page, not by each card.

import { docs, h } from "../app.js";
import { GUIDES } from "../guides.js";
import { HELP } from "../tour_content.js";

function panel(id, help) {
  const guide = GUIDES.find((g) => g.slug === help.guide);
  return h(
    "div",
    { class: "help-panel", id, role: "note" },
    h("p", { text: help.text }),
    guide ? h("p", { class: "small" }, docs(guide.slug, "", guide.kind === "index" ? guide.title : `Guide: ${guide.title}`)) : null,
  );
}

function decorate(card) {
  const id = card.id.slice(5);
  const help = HELP[id];
  const heading = card.querySelector("h2");
  if (!help || !heading || heading.querySelector(".help-btn")) return;
  const panelId = `help-${id}`;
  const button = h("button", { type: "button", class: "help-btn", "aria-expanded": "false", "aria-controls": panelId, "aria-label": `Help: ${heading.textContent}`, text: "?" });
  let open = null;
  const set = (on) => {
    if (on === Boolean(open)) return;
    if (open) open.remove();
    open = on ? panel(panelId, help) : null;
    if (open) heading.after(open);
    button.setAttribute("aria-expanded", String(on));
  };
  button.addEventListener("click", () => set(!open));
  button.addEventListener("keydown", (ev) => {
    if (ev.key !== "Escape" || !open) return;
    ev.stopPropagation();
    set(false);
  });
  heading.append(button);
}

/** Add a help button to every card under `root`, now and as cards load; returns a stop function */
export function attachHelp(root) {
  const all = () => root.querySelectorAll("section.card[id^=card-]").forEach(decorate);
  const observer = new MutationObserver(all);
  observer.observe(root, { childList: true, subtree: true });
  all();
  return () => observer.disconnect();
}
