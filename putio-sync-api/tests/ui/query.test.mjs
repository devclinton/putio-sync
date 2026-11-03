// Feature 045: the filter language and sort helper in ui/query.js
// (contracts/filter-grammar.md). Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { filterMatcher, formatSort, parseFilter, parseSort, sortRows } from "../../ui/query.js";

/** The names in `names` that `filter` shows */
const shown = (filter, names) => names.filter(filterMatcher(filter));

const NAMES = ["Photoshop.exe", "setup.exe", "movie.mkv", "Movie.2024.sample.mkv", "info.nfo", "x-men [YTS].mp4", "abc", "The Office S01"];

test("the user's example: contains exe and not photoshop", () => {
  assert.deepEqual(shown("-*photoshop* *exe*", NAMES), ["setup.exe"]);
});

test("a plain term matches anywhere, in any case", () => {
  assert.deepEqual(shown("movie", NAMES), ["movie.mkv", "Movie.2024.sample.mkv"]);
  assert.deepEqual(shown("MKV", NAMES), ["movie.mkv", "Movie.2024.sample.mkv"]);
});

test("a glob term must match the whole name", () => {
  assert.deepEqual(shown("*.mkv", NAMES), ["movie.mkv", "Movie.2024.sample.mkv"]);
  assert.deepEqual(shown("mkv*", NAMES), []);
  assert.deepEqual(shown("a?c", NAMES), ["abc"]);
  assert.deepEqual(shown("?bc", ["abc", "aabc"]), ["abc"]);
});

test("spaces AND the terms", () => {
  assert.deepEqual(shown("movie sample", NAMES), ["Movie.2024.sample.mkv"]);
  assert.deepEqual(shown("movie AND sample", NAMES), ["Movie.2024.sample.mkv"]);
  assert.deepEqual(shown("movie and sample", NAMES), ["Movie.2024.sample.mkv"]);
});

test("OR, or and | make alternatives", () => {
  const want = ["movie.mkv", "Movie.2024.sample.mkv", "info.nfo"];
  assert.deepEqual(shown("*.mkv OR *.nfo", NAMES), want);
  assert.deepEqual(shown("*.mkv or *.nfo", NAMES), want);
  assert.deepEqual(shown("*.mkv | *.nfo", NAMES), want);
  assert.deepEqual(shown("*.mkv|*.nfo", NAMES), want);
});

test("OR binds tighter than AND", () => {
  assert.deepEqual(shown("*.mkv OR *.nfo -sample", NAMES), ["movie.mkv", "info.nfo"]);
  assert.deepEqual(shown("-sample *.mkv OR *.nfo", NAMES), ["movie.mkv", "info.nfo"]);
});

test("- and NOT negate a term", () => {
  assert.deepEqual(shown("*.exe -photoshop", NAMES), ["setup.exe"]);
  assert.deepEqual(shown("*.exe NOT photoshop", NAMES), ["setup.exe"]);
  assert.deepEqual(shown("*.exe not photoshop", NAMES), ["setup.exe"]);
});

test("a dash on its own negates nothing", () => {
  assert.deepEqual(shown("- movie", NAMES), ["movie.mkv", "Movie.2024.sample.mkv"]);
});

test("a dash inside a term is literal", () => {
  assert.deepEqual(shown("x-men", NAMES), ["x-men [YTS].mp4"]);
});

test("quotes keep spaces and make operators literal", () => {
  assert.deepEqual(shown('"the office"', NAMES), ["The Office S01"]);
  assert.deepEqual(shown('"or"', ["or.txt", "b.txt"]), ["or.txt"]);
  assert.deepEqual(shown('"-x"', ["a-x", "ab"]), ["a-x"]);
  assert.deepEqual(shown('"the office', NAMES), ["The Office S01"]); // unclosed runs to the end
  assert.deepEqual(shown('-"the office"', NAMES).length, NAMES.length - 1);
});

test("brackets and regex characters are literal", () => {
  assert.deepEqual(shown("[YTS]", NAMES), ["x-men [YTS].mp4"]);
  assert.deepEqual(shown("*[yts]*", NAMES), ["x-men [YTS].mp4"]);
  assert.deepEqual(shown(".", ["a.b", "ab"]), ["a.b"]);
  assert.deepEqual(shown("a+b", ["a+b", "aab"]), ["a+b"]);
  assert.deepEqual(shown("(1)", ["x (1)", "x 1"]), ["x (1)"]);
  assert.deepEqual(shown("$^", ["$^", "x"]), ["$^"]);
});

test("empty and stray operators show everything", () => {
  for (const f of ["", "   ", "-", "OR", "|", "AND", "NOT", "- OR |"]) assert.deepEqual(shown(f, NAMES), NAMES, JSON.stringify(f));
  assert.deepEqual(parseFilter(""), []);
});

test("leading, trailing and repeated ORs are ignored", () => {
  const want = ["movie.mkv", "Movie.2024.sample.mkv", "info.nfo"];
  assert.deepEqual(shown("OR *.mkv OR OR *.nfo OR", NAMES), want);
  assert.deepEqual(shown("| *.mkv || *.nfo |", NAMES), want);
});

test("negation applies per term inside an OR group", () => {
  // -mkv OR -movie: shown unless the name has both
  assert.deepEqual(shown("-mkv OR -movie", NAMES).includes("movie.mkv"), false);
  assert.deepEqual(shown("-mkv OR -movie", NAMES).includes("setup.exe"), true);
});

test("parseFilter returns AND groups of OR terms", () => {
  const g = parseFilter("a b|-c");
  assert.equal(g.length, 2);
  assert.equal(g[0].length, 1);
  assert.equal(g[1].length, 2);
  assert.equal(g[1][1].not, true);
});

// ---- Sort ---------------------------------------------------------------------

const KEYS = ["name", "size", "date"];

test("parseSort and formatSort round-trip", () => {
  assert.deepEqual(parseSort("size-desc", KEYS), { key: "size", desc: true });
  assert.deepEqual(parseSort("name-asc", KEYS), { key: "name", desc: false });
  assert.equal(formatSort({ key: "size", desc: true }), "size-desc");
  assert.equal(formatSort({ key: "date", desc: false }), "date-asc");
  for (const bad of [null, "", "colour-asc", "size", "size-up"]) assert.equal(parseSort(bad, KEYS), null, String(bad));
});

const ROWS = [
  { id: 1, name: "b.mkv", size: 30, date: "2026-01-02", dir: false },
  { id: 2, name: "c folder", size: 5, date: "2026-01-03", dir: true },
  { id: 3, name: "a.mkv", size: 30, date: null, dir: false },
  { id: 4, name: "file10", size: null, date: "2026-01-01", dir: false },
  { id: 5, name: "file9", size: 1, date: "2026-01-04", dir: false },
];
const COLUMNS = { name: (r) => r.name, size: (r) => r.size, date: (r) => (r.date ? Date.parse(r.date) : null) };
const ids = (rows) => rows.map((r) => r.id);

test("no sort keeps the given order", () => {
  assert.deepEqual(ids(sortRows(ROWS, null, COLUMNS)), [1, 2, 3, 4, 5]);
});

test("names sort case-insensitively and numerically", () => {
  assert.deepEqual(ids(sortRows(ROWS, { key: "name", desc: false }, COLUMNS)), [3, 1, 2, 5, 4]);
  assert.deepEqual(ids(sortRows(ROWS, { key: "name", desc: true }, COLUMNS)), [4, 5, 2, 1, 3]);
});

test("numbers sort numerically, missing lowest, ties by name ascending", () => {
  assert.deepEqual(ids(sortRows(ROWS, { key: "size", desc: false }, COLUMNS)), [4, 5, 2, 3, 1]);
  assert.deepEqual(ids(sortRows(ROWS, { key: "size", desc: true }, COLUMNS)), [3, 1, 2, 5, 4]);
});

test("dates sort by time, missing oldest", () => {
  assert.deepEqual(ids(sortRows(ROWS, { key: "date", desc: false }, COLUMNS)), [3, 4, 1, 2, 5]);
});

test("folders come first in both directions", () => {
  const folder = (r) => r.dir;
  assert.deepEqual(ids(sortRows(ROWS, { key: "size", desc: false }, COLUMNS, { folder })), [2, 4, 5, 3, 1]);
  assert.deepEqual(ids(sortRows(ROWS, { key: "size", desc: true }, COLUMNS, { folder })), [2, 3, 1, 5, 4]);
});

test("equal names keep their order, and the input is not changed", () => {
  const rows = [
    { id: 1, name: "x", size: 1 },
    { id: 2, name: "x", size: 1 },
  ];
  assert.deepEqual(ids(sortRows(rows, { key: "size", desc: true }, COLUMNS)), [1, 2]);
  const before = ids(ROWS);
  sortRows(ROWS, { key: "name", desc: true }, COLUMNS);
  assert.deepEqual(ids(ROWS), before);
});
