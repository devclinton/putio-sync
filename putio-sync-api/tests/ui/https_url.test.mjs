// UI-01: update links are rendered only when they are https.

import assert from "node:assert/strict";
import { test } from "node:test";
import { httpsUrl } from "../../ui/format.js";

test("only https URLs are links", () => {
  assert.equal(httpsUrl("https://github.com/x/y/releases/v1"), "https://github.com/x/y/releases/v1");
  assert.equal(httpsUrl("HTTPS://github.com/x"), "HTTPS://github.com/x");
  for (const bad of ["javascript:alert(1)", "http://example.com", "data:text/html,x", "//evil.example", "https://", "", null, undefined, 5]) {
    assert.equal(httpsUrl(bad), null, String(bad));
  }
});
