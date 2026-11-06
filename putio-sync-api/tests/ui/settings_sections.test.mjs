// Feature 046: the Settings sections, addresses and text links in ui/settings/sections.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { SECTIONS, fieldId, linkParts, sectionOf, sectionOfKey, settingsHref } from "../../ui/settings/sections.js";

test("every card is in exactly one section", () => {
  const ids = SECTIONS.flatMap((s) => s.cards.map(([id]) => id));
  assert.equal(new Set(ids).size, ids.length);
  assert.deepEqual(SECTIONS.map((s) => s.id), ["general", "putio", "downloads", "rules", "integrations", "plugins", "media-server", "network", "advanced"]);
  assert.ok(SECTIONS.every((s) => s.help.length > 20));
});

test("addresses name a section and maybe a field", () => {
  assert.equal(settingsHref("downloads"), "#/settings/downloads");
  assert.equal(settingsHref("downloads", "download.verify"), "#/settings/downloads?field=download.verify");
  assert.equal(sectionOf("rules").title, "Rules and filters");
  assert.equal(sectionOf("nope").id, "general");
  assert.equal(sectionOf(undefined).id, "general");
});

test("field addresses map to input ids", () => {
  assert.equal(fieldId("download.verify"), "set-download-verify");
  assert.equal(fieldId("watch.paths"), "set-watcher-paths");
  assert.equal(fieldId("arr-enabled"), "arr-enabled");
  assert.equal(fieldId("card-plugins"), "card-plugins");
});

test("config keys map to sections", () => {
  assert.equal(sectionOfKey("download.workers"), "downloads");
  assert.equal(sectionOfKey("download.verify"), "downloads");
  assert.equal(sectionOfKey("download.rules"), "rules");
  assert.equal(sectionOfKey("download.whitelist_patterns"), "rules");
  assert.equal(sectionOfKey("watch.paths"), "putio");
  assert.equal(sectionOfKey("cleanup.enabled"), "putio");
  assert.equal(sectionOfKey("hooks"), "integrations");
  assert.equal(sectionOfKey("plugins"), "plugins");
  assert.equal(sectionOfKey("logging.level"), "advanced");
  assert.equal(sectionOfKey("notifications.enabled"), "general");
  assert.equal(sectionOfKey("tailscale.socket"), "network");
  assert.equal(sectionOfKey("dlna.port"), "media-server");
  assert.equal(sectionOfKey("precheck.enabled"), "rules");
  assert.equal(sectionOfKey("mystery.key"), "general");
});

test("Settings names in text become links", () => {
  assert.deepEqual(linkParts("nothing here"), ["nothing here"]);
  assert.deepEqual(linkParts("Open Settings › Integrations › Plugins: check it."), [
    "Open ",
    { text: "Settings › Integrations › Plugins", href: "#/settings/plugins?field=card-plugins" },
    ": check it.",
  ]);
  assert.deepEqual(linkParts("Turn it off in Settings › Plugins."), ["Turn it off in ", { text: "Settings › Plugins", href: "#/settings/plugins" }, "."]);
  assert.deepEqual(linkParts("Use Send test event in Settings › Integrations › Event hooks."), [
    "Use Send test event in ",
    { text: "Settings › Integrations › Event hooks", href: "#/settings/integrations?field=card-hooks" },
    ".",
  ]);
  assert.deepEqual(linkParts("See Settings › Downloads, then Settings › Put.io › Put.io storage"), [
    "See ",
    { text: "Settings › Downloads", href: "#/settings/downloads" },
    ", then ",
    { text: "Settings › Put.io › Put.io storage", href: "#/settings/putio?field=card-storage" },
  ]);
});
