// Feature 056: the usage profile's words and the setup link's code
import { test } from "node:test";
import assert from "node:assert/strict";
import { afterStartApps, answersFrom, codeFromHash, keyLabel, languagesFrom, note, reason, startAnswers, startKinds, title, valueText } from "../../ui/profile.js";

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
  // The *arr folder names only the app that uses it
  assert.equal(reason({ reason: "arr_category", kind: "movies", params: { folder: "radarr" } }), "Radarr sends its downloads to radarr.");
  assert.equal(reason({ reason: "arr_category", kind: "tv", params: { folder: "tv-sonarr" } }), "Sonarr sends its downloads to tv-sonarr.");
});

test("titles name the folder of a rule", () => {
  assert.equal(title({ id: "rule:Movies", params: { folder: "Movies" } }), "Folder Movies");
  assert.equal(title({ id: "delete", params: {} }), "Files on Put.io after download");
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

test("after start, only the apps the wizard set up are checked, matched on their saved address", () => {
  const apps = [
    { index: 0, kind: "radarr", name: "Radarr", url: "http://radarr:7878", api_key_set: true },
    { index: 1, kind: "sonarr", name: null, url: "http://sonarr:8989", api_key_set: true },
    { index: 2, kind: "radarr", name: "Old", url: "http://old:7878", api_key_set: true },
    { index: 3, kind: "sonarr", name: "No key", url: "http://nokey:8989", api_key_set: false },
  ];
  const setUp = [
    { url: "HTTP://Radarr:7878/", host: "nas.lan" },
    { url: "http://sonarr:8989", host: null },
    { url: "http://nokey:8989", host: "x" },
  ];
  assert.deepEqual(afterStartApps(apps, setUp), [
    { index: 0, kind: "radarr", name: "Radarr", host: "nas.lan" },
    { index: 1, kind: "sonarr", name: "Sonarr", host: null },
  ]);
  assert.deepEqual(afterStartApps(apps, []), []);
  assert.deepEqual(afterStartApps(null, setUp), []);
});
