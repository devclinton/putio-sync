// Feature 026: the Translation card's pure helpers in ui/translation.js.

import assert from "node:assert/strict";
import { test } from "node:test";
import { chatLines, readChat } from "../../ui/translation.js";

test("a working server shows the first sample", () => {
  const lines = chatLines({ ok: true, took_ms: 2310, sample: [{ number: 1, source: "Good morning.", translation: "Buenos días." }, { number: 2, source: "x", translation: "y" }] });
  assert.equal(lines.length, 1);
  assert.equal(lines[0].ok, true);
  assert.match(lines[0].text, /2 sample lines in 2310 ms: "Good morning\." became "Buenos días\."/);
});

test("each problem has its own words", () => {
  const key = chatLines({ ok: false, problem: "key", error: "the server rejected the API key" });
  assert.match(key[0].text, /rejected the API key/);
  const model = chatLines({ ok: false, problem: "model", error: "x", models: ["llama3", "qwen"] });
  assert.match(model[0].text, /does not know this model/);
  assert.equal(model[1].text, "Models on this server: llama3, qwen.");
  assert.match(chatLines({ ok: false, problem: "url" })[0].text, /server URL/);
  assert.match(chatLines({ ok: false, problem: "timeout" })[0].text, /in time/);
  assert.match(chatLines({ ok: false })[0].text, /test failed/);
});

test("a format problem shows the raw reply lines", () => {
  const lines = chatLines({ ok: false, problem: "format", error: "reply had 1 lines, expected 2", sample: [{ number: 1, source: "a", translation: "uno" }] });
  assert.equal(lines.length, 2);
  assert.equal(lines[1].text, "Reply line 1: uno");
});

test("the chat object is trimmed text", () => {
  assert.deepEqual(readChat({ base_url: " http://x ", model: "m ", api_key: undefined }), { base_url: "http://x", model: "m", api_key: "" });
});

import { callbackLines, headerText, parseHeaders } from "../../ui/translation.js";

test("header lines round trip and a line without a colon is refused", () => {
  const h = parseHeaders("X-Api-Key: abc\n\n Authorization : Bearer a:b ");
  assert.deepEqual(h, { "X-Api-Key": "abc", Authorization: "Bearer a:b" });
  assert.equal(headerText({ A: "1", B: "2" }), "A: 1\nB: 2");
  assert.throws(() => parseHeaders("nonsense"), /not "Name: value"/);
  assert.deepEqual(parseHeaders(""), {});
});

test("callback test lines name the result and the callback address", () => {
  const ok = callbackLines({ ok: true, result: "will_call_back", service: { took_ms: 12 }, base_url_reachable_from_here: { checked: true, status: 200 } });
  assert.equal(ok[0].ok, true);
  assert.match(ok[0].text, /will call back/);
  assert.equal(ok[1].ok, true);
  const bad = callbackLines({ ok: false, problem: "headers", error: "HTTP 401: no", service: { took_ms: 3 }, base_url_reachable_from_here: { checked: true, status: null, error: "refused" } });
  assert.match(bad[0].text, /rejected the headers/);
  assert.equal(bad[1].ok, false);
});
