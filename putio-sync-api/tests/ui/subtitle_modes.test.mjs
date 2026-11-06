// Feature 050: the Subtitles card's mode helpers in ui/subtitle_modes.js.

import assert from "node:assert/strict";
import { test } from "node:test";
import { MODES, modesText, parseModes, readModes } from "../../ui/subtitle_modes.js";

test("lines become an object and back", () => {
  const got = parseModes("radarr = download_only\nSonarr: both\n\n  anime   whisper_only ");
  assert.deepEqual(got, { radarr: "download_only", Sonarr: "both", anime: "whisper_only" });
  assert.equal(modesText({ radarr: "both" }), "radarr = both");
  assert.equal(modesText(undefined), "");
});

test("a wrong or missing mode is kept for the server to refuse", () => {
  assert.deepEqual(parseModes("radarr = Whisper-Only"), { radarr: "whisper-only" });
  assert.deepEqual(parseModes("radarr"), { radarr: "" });
});

test("an empty global mode is left out of the body", () => {
  const body = readModes({ mode: "", categories: "", languages: "es = both" });
  assert.equal(body.mode, undefined);
  assert.deepEqual(body.category_modes, {});
  assert.deepEqual(body.language_modes, { es: "both" });
  assert.ok(!JSON.stringify(body).includes("\"mode\""));
  assert.equal(readModes({ mode: "both", categories: "", languages: "" }).mode, "both");
});

test("the default is first and every other mode is offered", () => {
  assert.equal(MODES[0][0], "");
  assert.deepEqual(MODES.slice(1).map((m) => m[0]), ["download_only", "whisper_only", "both"]);
});

test("the OpenSubtitles fields are read with the stored User-Agent and an optional threshold", async () => {
  const { OS_FIELDS, readOpenSubtitles } = await import("../../ui/subtitle_modes.js");
  assert.deepEqual(OS_FIELDS.map((f) => f[0]), ["api_key", "username", "password"]);
  const got = readOpenSubtitles({ api_key: " KEY ", username: " me ", password: "********", min_confidence: "70" }, { user_agent: "mine/1" });
  assert.deepEqual(got, { opensubtitles: { api_key: "KEY", username: "me", password: "********", user_agent: "mine/1" }, min_confidence: 70 });
  assert.equal(readOpenSubtitles({ api_key: "", username: "", password: "", min_confidence: " " }).min_confidence, undefined);
  assert.equal(readOpenSubtitles({ api_key: "", username: "", password: "" }).opensubtitles.user_agent, "");
});
