// Feature 014: page tours and settings help. tour.js is the logic (which tours were seen,
// moving between steps, where the card sits); tour_content.js is the words. The coverage
// tests keep the words in step with the navigation, the settings cards and the guides.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { GUIDES } from "../../ui/guides.js";
import { SECTIONS } from "../../ui/settings/sections.js";
import { TOUR_KEY, due, markSeen, move, place, readSeen, resetSeen } from "../../ui/tour.js";
import { HELP, TOURS } from "../../ui/tour_content.js";

const ui = join(dirname(fileURLToPath(import.meta.url)), "../../ui");
const index = readFileSync(join(ui, "index.html"), "utf8");

function sources(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "i18n" ? [] : sources(path);
    return name.endsWith(".js") && name !== "tour_content.js" ? [readFileSync(path, "utf8")] : [];
  });
}

/** A Storage stand-in; `broken` throws on every call, as a blocked localStorage does */
function storage(broken = false) {
  const data = new Map();
  const guard = (f) => (...a) => {
    if (broken) throw new Error("blocked");
    return f(...a);
  };
  return { data, getItem: guard((k) => (data.has(k) ? data.get(k) : null)), setItem: guard((k, v) => void data.set(k, String(v))), removeItem: guard((k) => void data.delete(k)) };
}

test("every page in the navigation has a tour, and no other page does", () => {
  const pages = [...index.matchAll(/<a href="#\/[a-z]+" data-page="([a-z]+)"/g)].map((m) => m[1]);
  assert.ok(pages.length >= 11, `found ${pages.length} pages in index.html`);
  assert.deepEqual(Object.keys(TOURS).sort(), [...pages].sort());
  for (const page of ["login", "setup", "signin"]) assert.equal(TOURS[page], undefined, page);
});

test("every tour is short and every step has a title, text and target", () => {
  for (const [page, steps] of Object.entries(TOURS)) {
    assert.ok(steps.length >= 1 && steps.length <= 4, `${page}: ${steps.length} steps`);
    for (const s of steps) {
      assert.match(s.title, /^[A-Z]/, `${page} title`);
      assert.ok(s.text.length > 30 && s.text.length <= 220, `${page}: "${s.title}" is ${s.text.length} characters`);
      assert.match(s.target, /^[.#][a-z-]+$/, `${page} target ${s.target}`);
    }
    assert.equal(new Set(steps.map((s) => s.title)).size, steps.length, `${page}: duplicate title`);
  }
});

test("every tour target is a class or id the UI really uses", () => {
  const code = [index, ...sources(ui)].join("\n");
  for (const [page, steps] of Object.entries(TOURS)) {
    for (const s of steps) {
      const name = s.target.slice(1);
      const used = s.target[0] === "#" ? new RegExp(`id(?:=|: )"${name}"`) : new RegExp(`class(?:=|: |Name = )"(?:[^"]* )?${name}(?: [^"]*)?"`);
      assert.match(code, used, `${page}: nothing has ${s.target}`);
    }
  }
});

test("every settings card has help, and no help is left without a card", () => {
  // "stream" is a card on the Downloads section that the section list does not name.
  const cards = [...SECTIONS.flatMap((s) => s.cards.map(([id]) => id)), "stream"];
  assert.deepEqual(Object.keys(HELP).sort(), [...cards].sort());
  for (const [id, help] of Object.entries(HELP)) {
    assert.match(help.text, /^[A-Z]/, id);
    assert.ok(help.text.length > 40 && help.text.length <= 240, `${id}: ${help.text.length} characters`);
  }
});

test("every help guide link is a real guide", () => {
  const slugs = GUIDES.map((g) => g.slug);
  for (const [id, help] of Object.entries(HELP)) assert.ok(slugs.includes(help.guide), `${id}: ${help.guide} is not in guides.js`);
  // Help that names a subject with its own guide links to it, not to the index
  assert.equal(HELP.arr.guide, "guides/radarr-sonarr");
  assert.equal(HELP["folder-rules"].guide, "guides/folder-rules");
  assert.equal(HELP.watcher.guide, "guides/watch-folders");
  assert.equal(HELP["media-server"].guide, "guides/media-server");
});

test("a tour is due once per page and never while signing in or setting up", () => {
  assert.equal(due("status", {}), true);
  assert.equal(due("status", { status: 1 }), false);
  assert.equal(due("queue", { status: 1 }), true);
  for (const page of ["login", "setup", "signin", "", undefined]) assert.equal(due(page, {}), false, String(page));
  assert.equal(due("status", {}, { setup: true }), false);
});

test("seen pages are saved per browser, and reset shows every tour again", () => {
  const s = storage();
  assert.deepEqual(readSeen(s), {});
  assert.equal(markSeen(s, ["status"]), true);
  assert.equal(markSeen(s, ["queue", "logs"]), true);
  assert.deepEqual(JSON.parse(s.data.get(TOUR_KEY)), { status: 1, queue: 1, logs: 1 });
  assert.equal(due("status", readSeen(s)), false);
  assert.equal(due("history", readSeen(s)), true);
  resetSeen(s);
  assert.equal(s.data.has(TOUR_KEY), false);
  assert.deepEqual(readSeen(s), {});
});

test("storage that throws or holds rubbish never breaks a tour", () => {
  const broken = storage(true);
  assert.deepEqual(readSeen(broken), {});
  assert.equal(markSeen(broken, ["status"]), false);
  // Not saved, but not shown twice in this page load either
  assert.equal(due("status", readSeen(broken)), false);
  resetSeen(broken);
  assert.deepEqual(readSeen(broken), {});
  assert.deepEqual(readSeen(null), {});
  assert.equal(markSeen(null, ["status"]), false);
  resetSeen(null);
  for (const junk of ["not json", "[1,2]", "7", "null", '"x"']) {
    const s = storage();
    s.data.set(TOUR_KEY, junk);
    assert.deepEqual(readSeen(s), {}, junk);
  }
});

test("Back stops at the first step and Next past the last step ends the tour", () => {
  assert.equal(move(0, 3, 1), 1);
  assert.equal(move(1, 3, -1), 0);
  assert.equal(move(0, 3, -1), 0);
  assert.equal(move(2, 3, 1), null);
  assert.equal(move(0, 1, 1), null);
});

test("the card sits by its target and stays on screen", () => {
  const card = { width: 360, height: 200 };
  const view = { width: 1200, height: 800 };
  const inView = (at) => at.top >= 12 && at.left >= 12 && at.top + card.height <= view.height - 12 && at.left + card.width <= view.width - 12;
  // Below when there is room
  const below = place({ top: 80, bottom: 140, left: 240, right: 900 }, card, view);
  assert.deepEqual(below, { sheet: false, top: 152, left: 240 });
  // Above a target near the bottom
  const above = place({ top: 700, bottom: 780, left: 240, right: 900 }, card, view);
  assert.equal(above.top, 700 - 12 - 200);
  // Beside a target as tall as the window (the side navigation)
  const beside = place({ top: 0, bottom: 800, left: 0, right: 224 }, card, view);
  assert.equal(beside.left, 236);
  assert.ok(inView(beside));
  // A target at the right edge: pulled back in
  const edge = place({ top: 10, bottom: 50, left: 1100, right: 1190 }, card, view);
  assert.ok(inView(edge));
  // A target that fills the window: a corner, still on screen
  assert.ok(inView(place({ top: 0, bottom: 800, left: 0, right: 1200 }, card, view)));
  // No target on the page: bottom centre
  const none = place(null, card, view);
  assert.equal(none.left, 420);
  assert.ok(inView(none));
});

test("at phone width the card is a sheet", () => {
  assert.deepEqual(place({ top: 80, bottom: 140, left: 0, right: 375 }, { width: 351, height: 220 }, { width: 375, height: 812 }), { sheet: true });
  assert.deepEqual(place(null, { width: 351, height: 220 }, { width: 600, height: 812 }), { sheet: true });
  assert.equal(place(null, { width: 360, height: 220 }, { width: 601, height: 812 }).sheet, false);
});
