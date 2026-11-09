// Feature 051: the language matcher in ui/i18n.js, translation in ui/translate.js, and the
// string extractor in scripts/i18n-extract.mjs, which must agree with the committed en.json.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { match } from "../../ui/i18n.js";
import { compile, tr } from "../../ui/translate.js";
import { CATALOG, catalogText, extract, isText, scan, templateKey } from "../../../scripts/i18n-extract.mjs";

const de = compile({
  "Run now": "Jetzt ausführen",
  System: "System (de)",
  "Flagged {0} as fake": "{0} als Fälschung markiert",
  "Appearance: {0}": "Darstellung: {0}",
  "{0} of {1} files": "{1} Dateien, davon {0}",
  "Removed {0}": "Entfernt",
  Empty: "",
});
const t = (text) => tr(text, de.exact, de.patterns);

test("browser tags match a catalog by region, then by base language", () => {
  assert.equal(match("de-DE"), "de");
  assert.equal(match("pt-BR"), "pt");
  assert.equal(match("pt-PT"), "pt-PT");
  assert.equal(match("zh-CN"), "zh");
  assert.equal(match("zh-Hant"), "zh-TW");
  assert.equal(match("zh-HK"), "zh-TW");
  assert.equal(match("no"), "nb");
  assert.equal(match("en-GB"), "en");
  assert.equal(match("ga"), null);
  assert.equal(match(undefined), null);
});

test("exact strings are translated and keep their surrounding spaces", () => {
  assert.equal(t("Run now"), "Jetzt ausführen");
  assert.equal(t("  Run now "), "  Jetzt ausführen ");
  assert.equal(t("Run later"), "Run later");
  assert.equal(t("movie.2024.mkv"), "movie.2024.mkv");
  assert.equal(t("42"), "42");
});

test("patterns put each value where the translation wants it", () => {
  assert.equal(t("Flagged Some.Movie.mkv as fake"), "Some.Movie.mkv als Fälschung markiert");
  assert.equal(t("3 of 10 files"), "10 Dateien, davon 3");
});

test("a value that is itself a UI string is translated too", () => {
  assert.equal(t("Appearance: System"), "Darstellung: System (de)");
});

test("a translation that lost a value, or is empty, falls back to English", () => {
  assert.equal(t("Removed x.mkv"), "Removed x.mkv");
  assert.equal(t("Empty"), "Empty");
});

test("no catalog: text is untouched", () => {
  const none = compile({});
  assert.equal(tr("Run now", none.exact, none.patterns), "Run now");
});

test("the scanner finds strings and templates, not comments or regexes", () => {
  const found = scan(`
    // "Not this"
    /* "Nor this" */
    const re = /"[a-z]"/g;
    h("p", { text: "Saved" }, \`Copied \${n} files to \${dest}\`);
    toast(ok ? \`Done \${\`Nested \${x}\`}\` : 'It\\'s here');
  `);
  const values = found.map((s) => (s.kind === "string" ? s.value : templateKey(s)));
  assert.ok(values.includes("Saved"));
  assert.ok(values.includes("Copied {0} files to {1}"));
  assert.ok(values.includes("Nested {0}"));
  assert.ok(values.includes("It's here"));
  for (const v of values) assert.ok(!/Not this|Nor this|\[a-z\]/.test(v || ""), v);
});

test("text, not code, is kept", () => {
  for (const s of ["Run now", "Saved in this browser only.", "not on Put.io"]) assert.ok(isText(s), s);
  for (const s of ["btn btn-primary", "/local/queue", "page-head", "GET", "ByDate", "width:50%", "tr[data-path]", "a"]) assert.ok(!isText(s), s);
  assert.equal(templateKey({ parts: ["", " file", ""], exprs: ["n", "s"] }), null);
  assert.equal(templateKey({ parts: ["dot ", ""], exprs: ["kind"] }), null);
});

test("en.json is up to date with the UI's strings (make i18n)", () => {
  assert.equal(readFileSync(CATALOG, "utf8"), catalogText(extract()), "run `make i18n`");
});
