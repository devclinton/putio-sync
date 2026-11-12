// Feature 051: fill the web UI's catalogs from en.json with atranslate-cli. Only strings a
// catalog lacks are sent, so a new UI string costs one string per language; strings no
// longer in en.json are dropped. Needs `atranslate-cli` on PATH and ATRANSLATE_URL set to
// the translation service (ATRANSLATE_TOKEN too, if it wants one).
//
//   node scripts/i18n-translate.mjs           every language in ui/i18n.js
//   node scripts/i18n-translate.mjs de,fr     just these

import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { CATALOG, UI_DIR } from "./i18n-extract.mjs";

// Catalog code → the service's code, where its base-language model is missing
const SERVICE = { ar: "ar-AE" };
// Product names, kept as they are when they are the whole string
const NAMES = new Set(["Put.io", "putio-sync", "Radarr", "Sonarr", "Plex", "Jellyfin", "Emby", "Tailscale", "OpenSubtitles", "Whisper", "Transmission", "DLNA", "NAS"]);
// Languages not written in Latin script: any English word left in them is untranslated
const NON_LATIN = new Set(["ar", "he", "hi", "ja", "ko", "ru", "th", "uk", "zh", "zh-TW"]);
// English left as it was: the service sent the source back (several words, or any word in a
// language not written in Latin script)
const untranslated = (lang, en, out) => out === en && (NON_LATIN.has(lang) ? /[a-z]{3}/ : /[a-z]{2,} [a-z]{2,}/).test(en);

const read = (path) => (existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : {});
const write = (path, catalog) => writeFileSync(path, JSON.stringify(catalog, null, 1) + "\n");

const all = readFileSync(join(UI_DIR, "i18n.js"), "utf8").match(/LANGUAGES = \[([^\]]*)\]/)[1].match(/"([^"]+)"/g).map((s) => s.slice(1, -1));
const langs = process.argv[2] ? process.argv[2].split(",") : all;
if (!process.env.ATRANSLATE_URL) {
  console.error("Set ATRANSLATE_URL to the atranslate service, e.g. http://<host>:8400");
  process.exit(2);
}

const en = read(CATALOG);
const keys = Object.keys(en);
const path = (lang) => join(UI_DIR, "i18n", `${lang}.json`);
const missing = new Set();
const todo = langs.filter((lang) => {
  const have = read(path(lang));
  const gaps = keys.filter((k) => !NAMES.has(k) && (typeof have[k] !== "string" || untranslated(lang, k, have[k])));
  gaps.forEach((k) => missing.add(k));
  return gaps.length > 0;
});

const dir = mkdtempSync(join(tmpdir(), "putio-sync-i18n-"));
try {
  if (todo.length) {
    console.log(`${missing.size} strings to ${todo.join(",")}`);
    write(join(dir, "en.json"), Object.fromEntries([...missing].map((k) => [k, k])));
    const args = ["json", join(dir, "en.json"), "--to", todo.map((l) => SERVICE[l] || l).join(","), "--out-dir", dir, "--timeout", process.env.ATRANSLATE_TIMEOUT || "21600", "--server", process.env.ATRANSLATE_URL];
    if (process.env.ATRANSLATE_TOKEN) args.push("--token", process.env.ATRANSLATE_TOKEN);
    try {
      execFileSync("atranslate-cli", args, { stdio: "inherit" });
    } catch {
      console.error("atranslate-cli failed; keeping what it wrote");
    }
  }
  for (const lang of langs) {
    const have = read(path(lang));
    const fresh = read(join(dir, `${SERVICE[lang] || lang}.json`));
    const pick = (k) => (NAMES.has(k) ? k : typeof fresh[k] === "string" && !untranslated(lang, k, fresh[k]) ? fresh[k] : have[k]);
    // Keep en.json's order and only its keys
    write(path(lang), Object.fromEntries(keys.filter((k) => typeof pick(k) === "string").map((k) => [k, pick(k)])));
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}
