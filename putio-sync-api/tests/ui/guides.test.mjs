// Feature 016: the web UI's links to the guides stay in step with the book.
// guides.js lists every guide; this checks it against site/book/src/guides, SUMMARY.md and
// the docs("guides/...") calls in the UI sources, and checks the install channel map.

import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { GUIDES, guideFor, KIND_LABELS } from "../../ui/guides.js";

const here = dirname(fileURLToPath(import.meta.url));
const ui = join(here, "../../ui");
const book = join(here, "../../../site/book/src");

const summary = readFileSync(join(book, "SUMMARY.md"), "utf8");
const inSummary = [...summary.matchAll(/\]\((guides\/[a-z0-9-]+)\.md\)/g)].map((m) => m[1]);

function sources(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    if (name === "i18n") continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...sources(path));
    else if (name.endsWith(".js")) out.push(path);
  }
  return out;
}

test("every listed guide has a chapter, a SUMMARY entry and its H1 as the title", () => {
  for (const g of GUIDES) {
    assert.match(g.slug, /^guides\/[a-z0-9-]+$/);
    assert.ok(inSummary.includes(g.slug), `${g.slug} is not in SUMMARY.md`);
    const page = readFileSync(join(book, `${g.slug}.md`), "utf8");
    const h1 = page.split("\n").find((l) => l.startsWith("# "));
    assert.equal(g.title, h1.slice(2).trim(), `${g.slug} title`);
    assert.ok(["index", "setup", "task", "switch"].includes(g.kind), `${g.slug} kind`);
    assert.ok(KIND_LABELS[g.kind], `${g.kind} label`);
  }
  assert.equal(new Set(GUIDES.map((g) => g.slug)).size, GUIDES.length, "duplicate slug");
});

test("every guide in SUMMARY is in the list", () => {
  const listed = GUIDES.map((g) => g.slug);
  for (const slug of inSummary) assert.ok(listed.includes(slug), `${slug} is not in guides.js`);
});

test("every docs(\"guides/...\") call in the UI names a listed guide", () => {
  const listed = GUIDES.map((g) => g.slug);
  let found = 0;
  for (const file of sources(ui)) {
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(/(?:docs\(\s*|guide:\s*)"(guides\/[^"]*)"/g)) {
      found++;
      assert.ok(listed.includes(m[1]), `${file}: ${m[1]} is not a guide`);
    }
  }
  assert.ok(found >= 7, `expected the settings, doctor and logs links, found ${found}`);
});

test("guideFor follows the channel table", () => {
  const table = {
    unraid: "guides/unraid",
    truenas: "guides/truenas",
    container: "guides/docker",
    casaos: "guides/docker",
    umbrel: "guides/docker",
    snap: "guides/linux",
    deb: "guides/linux",
    flatpak: "guides/linux",
    homebrew: "guides/macos",
    pkg: "guides/macos",
    winget: "guides/windows",
    setup: "guides/windows",
    archive: "guides/index",
    unknown: "guides/index",
  };
  for (const [kind, slug] of Object.entries(table)) assert.equal(guideFor(kind), slug, kind);
  for (const kind of [undefined, null, "", "nix"]) assert.equal(guideFor(kind), "guides/index");
  const listed = GUIDES.map((g) => g.slug);
  for (const slug of Object.values(table)) assert.ok(listed.includes(slug), slug);
});
