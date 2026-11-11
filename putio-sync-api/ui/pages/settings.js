// Settings (features 015, 046, 048): every settings card, in sections shown one at a time.
// `#/settings/<section>?field=<address>` opens a section and jumps to a field. All sections
// load at once, so search covers every field and the section list marks unsaved changes and
// problems anywhere; Notifications (030) loads when opened. Advanced fields are hidden per
// section until Show advanced is on.

import { h, navigate, toastError } from "../app.js";
import { SECTIONS, fieldId, sectionOf, sectionOfKey, settingsHref } from "../settings/sections.js";
import { assistant, riskList } from "../settings/form.js";
import * as general from "../settings/general.js";
import * as putio from "../settings/putio.js";
import * as downloads from "../settings/downloads.js";
import * as filters from "../settings/filters.js";
import * as integrations from "../settings/integrations.js";
import * as plugins from "../settings/plugins.js";
import * as media from "../settings/media.js";
import { advancedCards, networkCards } from "../settings/advanced.js";

const BUILD = { general: general.cards, putio: putio.cards, downloads: downloads.cards, rules: filters.cards, integrations: integrations.cards, plugins: plugins.cards, "media-server": media.cards, network: networkCards, advanced: advancedCards };
const LAZY = { notifications: () => import("../settings/notifications.js") };
const ADV_KEY = "putio-sync.settings.advanced";

let seq = 0;
let page = null;

const stored = () => {
  try {
    return JSON.parse(localStorage.getItem(ADV_KEY)) || {};
  } catch {
    return {};
  }
};

export function mount(el, params) {
  const mine = ++seq;
  const state = {};
  const cardSection = Object.fromEntries(SECTIONS.flatMap((s) => s.cards.map(([id]) => [id, s.id])));
  const riskBox = h("section", { class: "card", role: "status", hidden: true });
  let risks = [];
  let jumpTimer = 0;
  const ctx = {
    alive: () => mine === seq,
    saved: {},
    cleanup: [],
    report: (id, patch) => {
      state[id] = { ...state[id], ...patch };
      marks();
    },
    onSaved: () => showRisks(),
    remount: () => {
      unmount();
      el.replaceChildren();
      mount(el, page ? page.params : params);
    },
  };

  const adv = stored();
  const panels = {};
  const nav = SECTIONS.map((s) => {
    const mark = h("span", { class: "badge warn", hidden: true, text: "unsaved" });
    const count = h("span", { class: "badge bad", hidden: true });
    const toggle = h("input", { type: "checkbox", checked: Boolean(adv[s.id]) });
    const toggleLabel = h("span");
    const advRow = h("label", { class: "adv-toggle", hidden: true }, toggle, toggleLabel);
    const node = h(
      "section",
      { class: "settings-panel", dataset: { section: s.id }, hidden: true },
      h("div", { class: "panel-head" }, h("div", {}, h("h2", { text: s.title }), h("p", { class: "muted", text: s.help })), advRow),
      h("div", { class: "stack" }, LAZY[s.id] ? [] : BUILD[s.id](ctx)),
    );
    let lazy;
    const load = () => LAZY[s.id] && (lazy ||= LAZY[s.id]().then((m) => mine === seq && node.lastChild.append(...m.cards(ctx)), (e) => toastError(e, (lazy = null))));
    const showAdv = (on) => {
      node.classList.toggle("show-adv", on);
      toggle.checked = on;
    };
    toggle.addEventListener("change", () => {
      showAdv(toggle.checked);
      const now = stored();
      now[s.id] = toggle.checked;
      try {
        localStorage.setItem(ADV_KEY, JSON.stringify(now));
      } catch {
        // private mode: the choice lasts until the page is left
      }
    });
    showAdv(Boolean(adv[s.id]));
    const link = h("a", { href: settingsHref(s.id) }, h("span", { text: s.title }), mark, count);
    panels[s.id] = { node, link, mark, count, advRow, toggleLabel, showAdv, load };
    return link;
  });

  // Marks per section: unsaved cards, and cards that failed plus warnings about its settings
  const marks = () => {
    for (const s of SECTIONS) {
      const p = panels[s.id];
      const cards = Object.entries(state).filter(([id]) => cardSection[id] === s.id);
      const problems = cards.filter(([, v]) => v.problem).length + risks.filter((r) => sectionOfKey(r.keys[0] || "") === s.id).length;
      const dirty = cards.some(([, v]) => v.dirty);
      p.mark.hidden = !dirty;
      p.count.hidden = !problems;
      p.count.textContent = String(problems);
      p.count.title = `${problems} problem${problems === 1 ? "" : "s"}`;
      const n = p.node.querySelectorAll(".adv").length;
      p.advRow.hidden = !n;
      p.toggleLabel.textContent = `Show advanced (${n})`;
    }
    // A warning about an advanced field shows it
    for (const k of risks.flatMap((r) => r.keys)) {
      const f = document.getElementById(fieldId(k));
      if (f && f.closest(".adv")) panels[sectionOfKey(k)].showAdv(true);
    }
  };

  async function showRisks() {
    const a = await assistant(true);
    if (mine !== seq) return;
    risks = (a && a.risks) || [];
    riskBox.hidden = !risks.length;
    const nodes = riskList(risks);
    risks.forEach((r, i) => {
      if (r.keys[0]) nodes[i].append(" ", h("a", { href: settingsHref(sectionOfKey(r.keys[0]), r.keys[0]), text: "Show setting" }));
    });
    riskBox.replaceChildren(h("h2", { text: "Warnings" }), ...nodes);
    marks();
  }

  // Search the labelled fields and card titles on every section
  const results = h("ul", { class: "search-results", hidden: true });
  const search = h("input", { class: "input", type: "search", placeholder: "Search settings", "aria-label": "Search settings", autocomplete: "off" });
  let uid = 0;
  const find = (q) => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    const hits = [];
    const add = (text, target, card, section) => {
      if (words.every((w) => text.toLowerCase().includes(w))) hits.push({ target, card, section });
    };
    for (const s of SECTIONS) {
      for (const card of panels[s.id].node.querySelectorAll(".card")) {
        const title = card.querySelector("h2, h3")?.textContent || "";
        if (card.id) add(title, card.id, title, s);
        for (const label of card.querySelectorAll("label.field")) {
          const input = label.querySelector("input, select, textarea");
          if (!input) continue;
          input.id ||= `set-found-${++uid}`;
          add(label.textContent, input.id, `${title} › ${label.querySelector("span")?.firstChild?.textContent || label.textContent}`, s);
        }
      }
    }
    return hits.slice(0, 10);
  };
  const go = (hit) => {
    clear();
    navigate(settingsHref(hit.section.id, hit.target));
  };
  const clear = () => {
    search.value = "";
    results.hidden = true;
  };
  search.addEventListener("input", () => {
    const q = search.value.trim();
    if (q.length < 2) return void (results.hidden = true);
    const hits = find(q);
    results.replaceChildren(
      ...(hits.length
        ? hits.map((hit) => h("li", {}, h("button", { type: "button", class: "link", onclick: () => go(hit) }, h("span", { text: hit.card }), h("small", { class: "muted", text: ` · ${hit.section.title}` }))))
        : [h("li", { class: "muted", text: "Nothing matched." })]),
    );
    results.hidden = false;
  });
  search.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") clear();
    if (ev.key === "Enter") {
      const hits = find(search.value.trim());
      if (hits.length) go(hits[0]);
    }
  });

  // Open a section, then wait up to 5 s for the field's card to load and jump to it
  const show = ({ path, query }) => {
    const field = query.get("field");
    // A card's address opens the section that holds the card (048: plugins moved out of
    // Integrations, and old links name that section)
    const card = field && fieldId(field).startsWith("card-") ? cardSection[fieldId(field).slice(5)] : null;
    const s = sectionOf(card || path[0] || (field && field.includes(".") ? sectionOfKey(field) : ""));
    for (const x of SECTIONS) {
      const p = panels[x.id];
      p.node.hidden = x.id !== s.id;
      if (x.id === s.id) p.link.setAttribute("aria-current", "page");
      else p.link.removeAttribute("aria-current");
    }
    panels[s.id].load();
    // The pills scroll sideways at phone width; keep the current one in view
    const link = panels[s.id].link;
    const nav = link.parentElement;
    nav.scrollLeft += link.getBoundingClientRect().left - nav.getBoundingClientRect().left - 16;
    clearTimeout(jumpTimer);
    if (!field) return;
    const id = fieldId(field);
    const fallback = `card-${field.split(".")[0] === "watch" ? "watcher" : field.split(".")[0]}`;
    const started = Date.now();
    const jump = () => {
      if (mine !== seq) return;
      const late = Date.now() - started > 5000;
      const target = document.getElementById(id) || (late ? document.getElementById(fallback) : null);
      if (!target) return void (late || (jumpTimer = setTimeout(jump, 100)));
      if (target.closest(".adv")) panels[s.id].showAdv(true);
      // A field on a tab that is not shown (048: plugin tabs): show its tab first
      const tab = target.closest("[role=tabpanel][hidden]");
      if (tab) document.getElementById(tab.getAttribute("aria-labelledby"))?.click();
      target.scrollIntoView({ block: "center" });
      if (target.matches("input, select, textarea")) target.focus({ preventScroll: true });
      const mark = target.closest("label.field") || target;
      mark.classList.add("flash");
      setTimeout(() => mark.classList.remove("flash"), 2000);
    };
    jump();
  };

  const leave = (ev) => {
    if (Object.values(state).some((v) => v.dirty)) {
      ev.preventDefault();
      ev.returnValue = "";
    }
  };
  window.addEventListener("beforeunload", leave);
  ctx.cleanup.push(() => window.removeEventListener("beforeunload", leave), () => clearTimeout(jumpTimer));

  el.append(
    h(
      "div",
      { class: "page-head" },
      h("div", {}, h("h1", { text: "Settings" }), h("p", { class: "muted", text: "Saved to the config file. Each save says whether it applies now or after a restart." })),
      h("div", { class: "settings-search" }, search, results),
    ),
    riskBox,
    h("div", { class: "settings-layout" }, h("nav", { class: "settings-nav", "aria-label": "Settings sections" }, nav), h("div", {}, SECTIONS.map((s) => panels[s.id].node))),
  );
  page = { params, ctx, show };
  show(params);
  showRisks();
}

/** Another section or field of Settings: shown in place, so unsaved input stays */
export function update(params) {
  page.params = params;
  page.show(params);
}

export function unmount() {
  seq += 1;
  if (page) for (const f of page.ctx.cleanup) f();
  page = null;
}
