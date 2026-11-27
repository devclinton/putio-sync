// Feature 070: the file picker's logic (ui/pick.js): the tree, the ticks and the choice
// sent to the API.

import assert from "node:assert/strict";
import { test } from "node:test";
import { allBut, folderState, onlyThis, setAll, setFolder, stateNote, summary, ticksOf, toChoice, tree, under } from "../../ui/pick.js";

const FILES = [
  { path: "Season 1/e10.mkv", size: 300, wanted: true, state: "none" },
  { path: "Season 1/e2.mkv", size: 200, wanted: true, state: "pending" },
  { path: "Season 1/Extras/making-of.mkv", size: 50, wanted: false, state: "none" },
  { path: "readme.txt", size: 1, wanted: false, state: "done" },
];

test("the tree nests folders and sorts names the way people count", () => {
  const root = tree(FILES);
  assert.deepEqual(root.files.map((f) => f.name), ["readme.txt"]);
  assert.deepEqual(root.dirs.map((d) => d.path), ["Season 1"]);
  const season = root.dirs[0];
  assert.deepEqual(season.files.map((f) => f.name), ["e2.mkv", "e10.mkv"]);
  assert.deepEqual(season.dirs.map((d) => d.path), ["Season 1/Extras"]);
  assert.equal(season.files[0].state, "pending");
  assert.deepEqual(under(season).sort(), ["Season 1/Extras/making-of.mkv", "Season 1/e10.mkv", "Season 1/e2.mkv"]);
  assert.deepEqual(tree([]), { name: "", path: "", dirs: [], files: [] });
});

test("ticks start from what the API says, or from everything when nothing was picked", () => {
  const ticks = ticksOf(FILES);
  assert.equal(ticks.get("Season 1/e2.mkv"), true);
  assert.equal(ticks.get("readme.txt"), false);
  assert.ok([...ticksOf(FILES, true).values()].every(Boolean));
});

test("a folder is all, none or some, and ticking it ticks everything under it", () => {
  const season = tree(FILES).dirs[0];
  let ticks = ticksOf(FILES);
  assert.equal(folderState(season, ticks), "some");
  assert.equal(folderState(season.dirs[0], ticks), "none");
  ticks = setFolder(ticks, season, true);
  assert.equal(folderState(season, ticks), "all");
  assert.equal(ticks.get("Season 1/Extras/making-of.mkv"), true);
  assert.equal(ticks.get("readme.txt"), false, "files outside the folder keep their tick");
  assert.equal(folderState(season, setFolder(ticks, season, false)), "none");
  assert.equal(folderState({ dirs: [], files: [] }, ticks), "none");
});

test("only this and all but this", () => {
  const ticks = ticksOf(FILES);
  const only = onlyThis(ticks, "Season 1/e2.mkv");
  assert.deepEqual([...only].filter(([, on]) => on).map(([p]) => p), ["Season 1/e2.mkv"]);
  const but = allBut(ticks, "Season 1/e2.mkv");
  assert.deepEqual([...but].filter(([, on]) => !on).map(([p]) => p), ["Season 1/e2.mkv"]);
  assert.ok([...setAll(ticks, true).values()].every(Boolean));
  assert.ok([...setAll(ticks, false).values()].every((on) => !on));
  assert.equal(ticks.get("readme.txt"), false, "the ticks passed in are not changed");
});

test("the summary counts the ticked files and their size", () => {
  assert.deepEqual(summary(FILES, ticksOf(FILES)), { count: 2, total: 4, size: 500, totalSize: 551 });
  assert.deepEqual(summary([], new Map()), { count: 0, total: 0, size: 0, totalSize: 0 });
});

test("the choice names every listed file and says what happens to later ones", () => {
  const choice = toChoice(FILES, onlyThis(ticksOf(FILES), "readme.txt"), false);
  assert.equal(choice.others, "skip");
  assert.equal(choice.files.length, 4);
  assert.deepEqual(choice.files.filter((f) => f.wanted), [{ path: "readme.txt", wanted: true }]);
  assert.equal(toChoice(FILES, ticksOf(FILES), true).others, "download");
});

test("a file that is here or on its way says what unticking does", () => {
  assert.equal(stateNote("none", false), null);
  assert.equal(stateNote("done", true), "Downloaded");
  assert.match(stateNote("done", false), /stays/);
  assert.match(stateNote("downloading", false), /finish/);
  for (const state of ["pending", "paused", "failed"]) {
    assert.equal(stateNote(state, true), "In the queue");
    assert.match(stateNote(state, false), /takes it out/);
  }
});
