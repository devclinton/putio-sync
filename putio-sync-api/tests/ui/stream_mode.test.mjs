// Feature 022: the Settings warning when stream mode is used but the media server can't
// show the streamed files, from ui/stream_mode.js.

import assert from "node:assert/strict";
import { test } from "node:test";
import { fixMediaServer, gapText, mediaServerGap, streamUsed } from "../../ui/stream_mode.js";

test("stream mode is used by the account or by any rule", () => {
  assert.equal(streamUsed({ mode: "download", rules: [] }), false);
  assert.equal(streamUsed({ mode: "stream" }), true);
  assert.equal(streamUsed({ mode: "download", rules: [{ path: "Movies", mode: "stream" }] }), true);
  assert.equal(streamUsed({ rules: [{ path: "Movies", mode: "download" }] }), false);
  assert.equal(streamUsed(null), false);
});

test("the media server must be on and share Put.io", () => {
  assert.equal(mediaServerGap({ enabled: false, roots: ["putio"] }), "off");
  assert.equal(mediaServerGap({ enabled: true, roots: ["downloads"] }), "no_putio");
  assert.equal(mediaServerGap({ enabled: true, roots: ["downloads", "putio"] }), null);
  assert.equal(mediaServerGap(null), "off");
  assert.match(gapText("off"), /media server is off/);
  assert.match(gapText("no_putio"), /doesn't share Put\.io/);
  assert.equal(gapText(null), "");
});

test("the fix turns the server on and adds Put.io, keeping the rest", () => {
  const dlna = { enabled: false, name: "NAS", port: 8200, bind: [], roots: ["downloads"], putio_folder: null, available_roots: [{ key: "putio" }] };
  assert.deepEqual(fixMediaServer(dlna), { enabled: true, name: "NAS", port: 8200, bind: [], roots: ["downloads", "putio"], putio_folder: null });
  assert.deepEqual(fixMediaServer({ enabled: true, roots: ["putio"] }).roots, ["putio"]);
});
