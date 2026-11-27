// Feature 056: the usage profile's words and the setup link's code
import { test } from "node:test";
import assert from "node:assert/strict";
import { answersFrom, codeFromHash, keyLabel, languagesFrom, note, reason, startAnswers, startKinds, title, valueText } from "../../ui/profile.js";

test("the setup code comes from the link's fragment", () => {
  assert.equal(codeFromHash("#code=AB12-CD34"), "AB12-CD34");
  assert.equal(codeFromHash("#x=1&code=a%20b"), "a b");
  assert.equal(codeFromHash("#/status"), null);
  assert.equal(codeFromHash("#code="), null);
  assert.equal(codeFromHash(""), null);
});

test("found folders tick their kinds, in screen order", () => {
  assert.deepEqual(startKinds({ tv: ["TV"], movies: ["Movies"], music: [] }), ["movies", "tv"]);
  assert.deepEqual(startKinds(null), []);
});

test("saved answers win over what Put.io suggests", () => {
  const saved = { kinds: ["music"], arr: [], by_hand: true, watch: "/w", language: "de" };
  assert.deepEqual(startAnswers(saved, { movies: ["Movies"] }, "/x"), saved);
  const fresh = startAnswers(null, { movies: ["Movies"] }, "/x");
  assert.deepEqual(fresh.kinds, ["movies"]);
  assert.equal(fresh.by_hand, true);
  assert.equal(fresh.suggestedWatch, "/x");
});

test("each reason code has words", () => {
  for (const code of ["found", "arr_category", "new_folder", "detected", "smallest_size", "software_off", "arr_imports", "library", "video"]) {
    assert.ok(reason({ reason: code, params: { folder: "Movies" } }).length > 10, code);
  }
  assert.match(reason({ reason: "found", params: { folder: "Movies" } }), /Movies/);
  assert.equal(reason({ reason: "nope", params: {} }), "");
});

test("titles name the folder of a rule", () => {
  assert.equal(title({ id: "rule:Movies", params: { folder: "Movies" } }), "Folder Movies");
  assert.equal(title({ id: "delete", params: {} }), "Delete from Put.io after download");
});

test("setting keys read as words", () => {
  assert.equal(keyLabel("download.rules[TV Shows].preset"), "Folder rule for TV Shows");
  assert.equal(keyLabel("precheck.watch[/w].enabled"), "Release check for /w");
  assert.equal(keyLabel("precheck.watch[/w].min_size_mb"), "Smallest release for /w (MB)");
  assert.equal(keyLabel("cleanup.enabled"), "Put.io cleanup");
  assert.equal(keyLabel("unknown.key"), "unknown.key");
});

test("values and notes", () => {
  assert.equal(valueText(null), "default");
  assert.equal(valueText(true), "on");
  assert.equal(valueText(["de", "en"]), "de, en");
  assert.equal(valueText([]), "none");
  assert.equal(valueText(300), "300");
  assert.match(note("dlna"), /Media server/);
  assert.equal(note("x"), "");
});

test("answers keep only known choices and an empty watch folder is off", () => {
  const a = answersFrom({ kinds: ["tv", "movies", "bogus"], arr: ["sonarr", "lidarr"], byHand: 0, watchOn: true, watch: "  ", language: "de" });
  assert.deepEqual(a, { kinds: ["movies", "tv"], arr: ["sonarr"], by_hand: false, watch: null, language: "de" });
  assert.equal(answersFrom({ kinds: ["tv"], arr: [], byHand: true, watchOn: true, watch: " /w ", language: "" }).watch, "/w");
  assert.equal(answersFrom({ kinds: ["tv"], arr: [], byHand: true, watchOn: false, watch: "/w", language: "" }).watch, null);
  assert.deepEqual(languagesFrom("de, en  fr"), ["de", "en", "fr"]);
});
