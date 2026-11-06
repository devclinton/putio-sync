// Feature 022: the media server status line and shared-folder list in ui/media_text.js.

import assert from "node:assert/strict";
import { test } from "node:test";
import { pickRoots, statusText } from "../../ui/media_text.js";
import { fieldId, sectionOfKey, sectionOf } from "../../ui/settings/sections.js";

test("a running server shows addresses, streams and players", () => {
  const s = { state: "running", addresses: ["http://192.168.1.10:8200/description.xml"], streams_active: 1, players_last_hour: 2, putio: { state: "ok" } };
  assert.equal(statusText(s), "http://192.168.1.10:8200/description.xml · 1 playing · 2 players in the last hour · Put.io: ok");
  assert.match(statusText({ ...s, players_last_hour: 1 }), /1 player in the last hour/);
});

test("an off or failed server shows no counts", () => {
  assert.equal(statusText({ state: "off", addresses: [], putio: { state: "off" } }), "");
  assert.equal(statusText({ state: "failed", addresses: [], putio: { state: "off" } }), "");
});

test("a Put.io problem is shown with its reason", () => {
  const s = { state: "running", addresses: [], streams_active: 0, players_last_hour: 0, putio: { state: "error", reason: "listing a folder: timed out" } };
  assert.match(statusText(s), /Put\.io: listing a folder: timed out/);
});

test("roots keep the list's order and keep ones no longer available", () => {
  const available = [{ key: "downloads" }, { key: "cat:radarr" }, { key: "putio" }];
  assert.deepEqual(pickRoots(available, new Set(["putio", "downloads"])), ["downloads", "putio"]);
  assert.deepEqual(pickRoots(available, new Set(["cat:gone", "cat:radarr"])), ["cat:radarr", "cat:gone"]);
});

test("the media server has its own section", () => {
  assert.equal(sectionOfKey("dlna.port"), "media-server");
  assert.equal(fieldId("dlna.port"), "set-media-server-port");
  assert.equal(sectionOf("media-server").title, "Media server");
});
