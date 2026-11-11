// Feature 023: the pure parts of the Organise panel in ui/organise_text.js.

import assert from "node:assert/strict";
import { test } from "node:test";
import { idsFor, outcomeText, pathOf, sortMoves } from "../../ui/organise_text.js";

const moves = [
  { file_id: 1, status: "planned", updated_at: "2026-11-01" },
  { file_id: 2, status: "applied", updated_at: "2026-11-02" },
  { file_id: 3, status: "failed", updated_at: "2026-11-03" },
  { file_id: 4, status: "undone", updated_at: "2026-11-04" },
];

test("each action takes only the ticked moves in a state it applies to", () => {
  const all = new Set([1, 2, 3, 4]);
  assert.deepEqual(idsFor(moves, all, "apply"), [1, 3, 4]);
  assert.deepEqual(idsFor(moves, all, "undo"), [2]);
  assert.deepEqual(idsFor(moves, all, "dismiss"), [1, 3, 4]);
  assert.deepEqual(idsFor(moves, new Set([2]), "apply"), []);
});

test("a batch result counts what worked and shows the first error", () => {
  const results = [
    { file_id: 1, status: "applied" },
    { file_id: 3, status: "failed", error: "\"x.mkv\" already exists there" },
    { file_id: 5, status: "failed" },
  ];
  assert.equal(outcomeText(results, "applied"), "1 applied · 2 failed: \"x.mkv\" already exists there");
  assert.equal(outcomeText([{ file_id: 9, status: "missing" }], "applied"), "Nothing to do");
});

test("planned moves come first, newest first within a state", () => {
  const sorted = sortMoves([...moves, { file_id: 5, status: "planned", updated_at: "2026-11-05" }]);
  assert.deepEqual(sorted.map((m) => m.file_id), [5, 1, 3, 2, 4]);
});

test("a path at the root has no leading slash", () => {
  assert.equal(pathOf("", "a.mkv"), "a.mkv");
  assert.equal(pathOf("TV Shows/Show/Season 01", "a.mkv"), "TV Shows/Show/Season 01/a.mkv");
});
