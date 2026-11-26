// Hot reload status banner (ui/settings/reload.js). Run with `node --test`.

import assert from "node:assert/strict";
import { test } from "node:test";
import { noticeParts } from "../../ui/settings/reload.js";

test("nothing to say for a clean status", () => {
  assert.equal(noticeParts(null), null);
  assert.equal(noticeParts({ last_error: null, pending_restart: [] }), null);
  assert.equal(noticeParts({ last_error: "  ", pending_restart: [""] }), null);
});

test("a refused edit and pending restarts are shown", () => {
  assert.deepEqual(noticeParts({ last_error: " expected `=` (line 3, column 5) ", pending_restart: ["api.bind", 7] }), {
    error: "expected `=` (line 3, column 5)",
    pending: ["api.bind"],
  });
  assert.deepEqual(noticeParts({ pending_restart: ["metrics.enabled"] }), { error: null, pending: ["metrics.enabled"] });
});
