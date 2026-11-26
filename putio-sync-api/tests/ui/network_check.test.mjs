// The network save's confirm text (ui/network_check.js). Run with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { checkText } from "../../ui/network_check.js";

test("under daemon the move happens once saved", () => {
  const c = { applies_now: true, this_page: "cut_off", listened_on: "127.0.0.1:15432", ui_at: ["http://127.0.0.1:15433/"] };
  assert.equal(checkText(c), "This page came in on 127.0.0.1:15432, which the new list doesn't listen on: once saved it won't reach putio-sync. Once saved, the UI is at http://127.0.0.1:15433/.");
  assert.equal(checkText({ applies_now: true, this_page: "unknown", ui_at: [] }), "putio-sync can't tell whether this page will still reach it once saved.");
  assert.equal(checkText({ applies_now: true, this_page: "kept", ui_at: ["http://nas:15432/"] }), "Once saved, the UI is at http://nas:15432/.");
});

test("under server it waits for a restart", () => {
  const c = { applies_now: false, this_page: "cut_off", listened_on: "0.0.0.0:15432", ui_at: [] };
  assert.equal(checkText(c), "This page came in on 0.0.0.0:15432, which the new list doesn't listen on: after a restart it won't reach putio-sync.");
  assert.equal(checkText({ this_page: "kept", ui_at: [] }), "");
});
