// Feature 061: the settings catalog in ui/settings/catalog.js that the command palette and
// Settings search share. Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { buildCatalog } from "../../ui/settings/catalog.js";
import { SECTIONS, fieldId } from "../../ui/settings/sections.js";

const GROUPS = [
  { id: "download", key: "download", title: "Downloads", fields: [["workers", "Workers", "Downloads that run at once", "n", 1]] },
  { id: "bandwidth", key: "download", title: "Bandwidth", fields: [["max_speed", "Speed limit", null, "n?"]] },
  { id: "stream", key: "download", title: "Streaming", fields: [["stream_mode", "Stream mode", null, "b"]] },
];

const field = (href) => new URLSearchParams(href.split("?")[1] || "").get("field");

test("every section and every card has an entry", () => {
  const all = buildCatalog([]);
  for (const s of SECTIONS) {
    assert.ok(all.some((e) => e.href === `#/settings/${s.id}` && e.label === s.title), s.id);
    for (const [id, title] of s.cards) {
      assert.ok(all.some((e) => e.card === id && e.label === title && field(e.href) === `card-${id}`), id);
    }
  }
});

test("each group field opens its input in its card's section", () => {
  const all = buildCatalog(GROUPS);
  const workers = all.find((e) => e.label === "Workers");
  assert.equal(workers.section, "downloads");
  assert.equal(workers.detail, "Downloads › Downloads");
  assert.equal(workers.hint, "Downloads that run at once");
  assert.equal(workers.href, "#/settings/downloads?field=set-download-workers");
  // A group whose key differs from its id: the input id uses the key, as form.js does
  const speed = all.find((e) => e.label === "Speed limit");
  assert.equal(speed.detail, "Downloads › Bandwidth");
  assert.equal(fieldId(field(speed.href)), "set-download-max_speed");
});

test("a card that no section lists goes in the section that edits its key", () => {
  const e = buildCatalog(GROUPS).find((x) => x.label === "Stream mode");
  assert.equal(e.section, "downloads");
  assert.equal(e.detail, "Downloads › Streaming");
  assert.equal(e.href, "#/settings/downloads?field=set-download-stream_mode");
});
