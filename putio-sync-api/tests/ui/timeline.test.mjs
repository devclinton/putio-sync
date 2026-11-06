// Feature 038: the Active page's timeline wording and grouping in ui/timeline.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { attemptLabel, seconds, skipReason, splitStages, stepActions, stepLine, stepSeconds, stepTitle } from "../../ui/timeline.js";

test("the row line names the stage, or says the item needs you", () => {
  assert.equal(stepLine(null, "running"), null);
  assert.equal(stepLine({ stage: "post_processing", name: "tidy", state: "running" }, "running"), "Post-processing: tidy");
  assert.equal(stepLine({ stage: "post_processing", name: "tidy", state: "failed" }, "waiting_for_operator"), "Needs you: tidy failed");
});

test("times read SQLite and RFC 3339 text as UTC", () => {
  assert.equal(seconds("2026-11-05 10:00:00", "2026-11-05 10:00:07"), 7);
  assert.equal(seconds("2026-11-05T10:00:00Z", "2026-11-05T10:01:00Z"), 60);
  assert.equal(seconds("2026-11-05 10:00:00", null), null);
  assert.equal(seconds("nope", "2026-11-05 10:00:00"), null);
  const step = { attempts: [{ started_at: "2026-11-05 10:00:00", ended_at: "2026-11-05 10:00:02" }, { started_at: "2026-11-05 10:01:00", ended_at: "2026-11-05 10:01:05" }] };
  assert.equal(stepSeconds(step), 65);
  assert.equal(stepSeconds({ attempts: [] }), null);
});

test("attempts say how they ended", () => {
  assert.equal(attemptLabel({ n: 2, result: "failed", exit_code: 3 }), "#2 failed (exit 3)");
  assert.equal(attemptLabel({ n: 1, result: "done", exit_code: 0 }), "#1 done");
  assert.equal(attemptLabel({ n: 1, result: "timeout", exit_code: null }), "#1 timeout");
  assert.equal(attemptLabel({ n: 1, result: null }), "#1 running");
});

test("skipped stages are set apart, the download stage never is", () => {
  const stages = [
    { stage: "pre_download", steps: [] },
    { stage: "downloading", steps: [{ state: "done" }] },
    { stage: "pre_processing", steps: [{ state: "done" }] },
    { stage: "translating", steps: [], branches: {}, skipped: "not_configured" },
    { stage: "post_processing", steps: [{ state: "skipped", skip_reason: "category" }] },
    { stage: "notifications", steps: [], branches: { a: [{ state: "waiting" }] } },
  ];
  const { shown, skipped } = splitStages(stages);
  assert.deepEqual(shown.map((s) => s.stage), ["downloading", "pre_processing", "notifications"]);
  assert.deepEqual(skipped.map((s) => s.stage), ["pre_download", "translating", "post_processing"]);
});

test("skip reasons and the buttons a step gets", () => {
  assert.equal(skipReason({ state: "skipped", skip_reason: "category" }), "not for this category");
  assert.equal(skipReason({ state: "skipped", skip_reason: "operator" }), "skipped by you");
  assert.equal(skipReason({ state: "done" }), "");
  assert.deepEqual(stepActions({ state: "failed" }), { retry: true, skip: true });
  assert.deepEqual(stepActions({ state: "interrupted" }), { retry: true, skip: false });
  assert.deepEqual(stepActions({ state: "running" }), { retry: false, skip: false });
});

test("subtitle steps are named after what they work on", () => {
  const scan = { stage: "subtitles", kind: "subtitle_scan", name: "videos", state: "running" };
  const video = { stage: "subtitles", kind: "subtitles", name: "Movie (2024)/Movie.2024.1080p.mkv", state: "running" };
  assert.equal(stepTitle(scan), "Find videos");
  assert.equal(stepTitle(video), "Subtitles for Movie.2024.1080p.mkv");
  assert.equal(stepTitle({ kind: "hook", name: "tidy" }), "tidy");
  assert.equal(stepLine(video, "running"), "Subtitles for Movie.2024.1080p.mkv");
  assert.equal(stepLine({ ...video, state: "waiting" }, "running"), "Waiting for subtitles");
  assert.equal(stepLine({ ...video, state: "failed" }, "running"), "Needs you: Subtitles for Movie.2024.1080p.mkv failed");
  assert.equal(skipReason({ state: "skipped", skip_reason: "no_video" }), "no video in this item");
});

test("translate steps name the file and the language, and a parked one says so (026)", () => {
  const scan = { stage: "translating", kind: "translate_scan", name: "videos", state: "running" };
  const step = { stage: "translating", kind: "translate", branch: "es", name: "Movie (2024)/Movie.2024.mkv", state: "running" };
  assert.equal(stepTitle(scan), "Find subtitles to translate");
  assert.equal(stepTitle(step), "Movie.2024.mkv (es)");
  assert.equal(stepLine(step, "running"), "Translating: Movie.2024.mkv (es)");
  const parked = { ...step, waiting: { for: "callback", since: "2026-11-05T10:00:00Z", expires_at: "2026-11-05T11:00:00Z" } };
  assert.equal(stepLine(parked, "running"), "Translating: Movie.2024.mkv (es), waiting for callback");
  assert.equal(stepLine({ ...step, state: "failed" }, "running"), "Needs you: Movie.2024.mkv (es) failed");
});
