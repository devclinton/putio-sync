// Feature 025 US3: the Fetch subtitles action on Local files (ui/subtitles.js).

import assert from "node:assert/strict";
import { test } from "node:test";
import { fetchPlan, fetchState, isVideoName } from "../../ui/subtitles.js";

test("only video files get the action, and some are disabled", () => {
  assert.equal(isVideoName("Movie.MKV"), true);
  assert.equal(isVideoName("Movie.srt"), false);
  assert.equal(isVideoName("mkv"), false);
  const file = (o) => ({ kind: "file", name: "a.mkv", valid_name: true, partial: null, in_use: false, ...o });
  assert.deepEqual(fetchState(file({})), { show: true, disabled: false, why: "" });
  assert.equal(fetchState(file({ kind: "dir" })).show, false);
  assert.equal(fetchState(file({ name: "a.nfo" })).show, false);
  assert.equal(fetchState(file({ in_use: true })).disabled, true);
  assert.equal(fetchState(file({ valid_name: false })).disabled, true);
});

test("nothing is asked when Put.io can help and nothing is saved", () => {
  const plan = fetchPlan("a.mkv", { eligible: true, putio: "available", languages: ["en"], existing: [] });
  assert.deepEqual(plan, { go: true, confirm: null });
});

test("transcription and existing files are confirmed first", () => {
  const gone = fetchPlan("a.mkv", { eligible: true, putio: "gone", languages: ["en", "es"], existing: [{ name: "a.en.srt" }] });
  assert.equal(gone.go, true);
  assert.match(gone.confirm.text, /no longer has this file/);
  assert.match(gone.confirm.text, /transcription server/);
  assert.match(gone.confirm.text, /a\.en\.srt/);
  assert.match(gone.confirm.text, /Languages: en, es/);
  assert.equal(gone.confirm.verb, "Fetch");
  const unknown = fetchPlan("a.mkv", { eligible: true, putio: "unknown", languages: ["en"], existing: [] });
  assert.match(unknown.confirm.text, /cannot be asked/);
  const kept = fetchPlan("a.mkv", { eligible: true, putio: "available", languages: ["en"], existing: [{ name: "a.en.srt" }] });
  assert.doesNotMatch(kept.confirm.text, /transcription/);
});

test("an ineligible file stops with the reason", () => {
  assert.deepEqual(fetchPlan("a.mkv", { eligible: false, reason: "No subtitle languages are set" }), { go: false, message: "No subtitle languages are set" });
  assert.match(fetchPlan("a.mkv", { eligible: false }).message, /cannot be fetched/);
});

import { translatePlan } from "../../ui/subtitles.js";

test("translate: needs an eligible video with a missing language, and names the languages", () => {
  assert.equal(translatePlan("a.mkv", { eligible: false, reason: "No translator" }).message, "No translator");
  assert.equal(translatePlan("a.mkv", { eligible: true, missing: [] }).go, false);
  const p = translatePlan("a.mkv", { eligible: true, missing: ["es", "fr"] });
  assert.deepEqual(p.targets, ["es", "fr"]);
  assert.match(p.confirm, /into es, fr\?/);
});

test("a Whisper mode confirms transcription even when Put.io has the file", () => {
  for (const mode of ["whisper_only", "both"]) {
    const plan = fetchPlan("a.mkv", { eligible: true, putio: mode === "both" ? "available" : "unknown", mode, languages: ["en"], existing: [] });
    assert.match(plan.confirm.text, /transcription server/);
  }
  assert.equal(fetchPlan("a.mkv", { eligible: true, putio: "available", mode: "download_only", languages: ["en"], existing: [] }).confirm, null);
});
