// Feature 061: loose matching for the command palette in ui/fuzzy.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { match, rank } from "../../ui/fuzzy.js";

test("letters in order match, ignoring case", () => {
  assert.notEqual(match("subt", "Subtitles"), null);
  assert.notEqual(match("SUBT", "subtitles"), null);
  assert.notEqual(match("stl", "Subtitles"), null);
  assert.notEqual(match("s01e02", "Some.Show.S01E02.mkv"), null);
});

test("letters out of order or missing do not match", () => {
  assert.equal(match("tsub", "Subtitles"), null);
  assert.equal(match("queuex", "Queue"), null);
});

test("a letter enters another word only at its start", () => {
  assert.equal(match("show", "Skip these categories Downloads › Subtitles"), null);
  assert.notEqual(match("show", "The.Show.S01E01.mkv"), null);
  assert.notEqual(match("sd", "Sync downloads"), null);
  // A later word start is found even when an earlier letter would block it
  assert.notEqual(match("sett", "Set settings"), null);
});

test("an empty query matches everything", () => {
  assert.equal(match("", "Anything"), 0);
  assert.equal(match("  ", "Anything"), 0);
});

test("word starts and adjacent letters rank higher", () => {
  assert.ok(match("bw", "Bandwidth Window") > match("bw", "Bandwidth"));
  assert.ok(match("que", "Queue") > match("que", "Unique entries"));
  assert.ok(match("queue", "Queue") > match("queue", "Clear the queue"));
});

test("rank keeps the best matches, in order, up to the limit", () => {
  const items = ["Clear completed", "Queue", "Unique", "Questions", "Status"];
  assert.deepEqual(rank("qu", items, (x) => x, 2), ["Queue", "Questions"]);
  assert.deepEqual(rank("zzz", items, (x) => x), []);
  assert.equal(rank("", items, (x) => x).length, 5);
});
