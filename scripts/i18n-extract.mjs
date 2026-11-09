// Feature 051: build the web UI's English catalog, putio-sync-api/ui/i18n/en.json, from the
// strings in the UI's code. English stays in the code as the lookup key (gettext style); the
// catalog lists every string that may reach the page so the other languages can be made from it.
//
//   node scripts/i18n-extract.mjs          write en.json
//   node scripts/i18n-extract.mjs --check  exit 1 when en.json is out of date
//
// Kept: string literals and template literals that read as UI text (they start with a capital
// letter, or are several words that aren't CSS classes). A template's `${...}` parts become
// {0}, {1}... in order; a template whose value is glued to a letter (`file${s}`) is skipped,
// since its wording depends on the value. index.html contributes its text and labels.

import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
export const UI_DIR = join(ROOT, "putio-sync-api", "ui");
export const CATALOG = join(UI_DIR, "i18n", "en.json");

const ESCAPES = { n: "\n", t: "\t", r: "\r", b: "\b", f: "\f", v: "\v", 0: "\0" };

/** The value of a quoted string or template chunk, escapes resolved */
function unescape(raw) {
  return raw.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|\r?\n|.)/g, (_, e) => {
    if (e[0] === "u" && e.length > 1) return String.fromCodePoint(parseInt(e.replace(/[u{}]/g, ""), 16));
    if (e[0] === "x" && e.length === 3) return String.fromCharCode(parseInt(e.slice(1), 16));
    if (e.endsWith("\n")) return "";
    return ESCAPES[e] ?? e;
  });
}

// After one of these a `/` starts a regular expression, not a division
const REGEX_AFTER = new Set(["(", ",", "=", ":", "[", "!", "&", "|", "?", "{", "}", ";", "+", "-", "*", "%", "<", ">", "~", "^", ""]);
const REGEX_WORDS = /(?:^|[^\w$])(return|typeof|case|do|else|in|of|void|yield|await)$/;

/**
 * The strings in JavaScript `src`: `{ kind: "string", value }` for quoted strings and
 * `{ kind: "template", parts, exprs }` for templates (`parts` cooked, one more than `exprs`).
 * Templates inside `${...}` are found too.
 */
export function scan(src) {
  const out = [];
  let i = 0;
  let last = ""; // last significant character, for the regex rule
  let lastWord = "";
  const n = src.length;

  function quoted(q) {
    let j = i + 1;
    while (j < n && src[j] !== q) j += src[j] === "\\" ? 2 : 1;
    out.push({ kind: "string", value: unescape(src.slice(i + 1, j)) });
    i = j + 1;
  }

  // From the opening backtick at `i`; leaves `i` after the closing one
  function template() {
    const parts = [];
    const exprs = [];
    let chunk = "";
    let j = i + 1;
    while (j < n && src[j] !== "`") {
      if (src[j] === "\\") {
        chunk += src.slice(j, j + 2);
        j += 2;
      } else if (src[j] === "$" && src[j + 1] === "{") {
        parts.push(unescape(chunk));
        chunk = "";
        // The expression runs to its matching brace, past nested strings and templates
        let depth = 1;
        let k = j + 2;
        while (k < n && depth > 0) {
          const c = src[k];
          if (c === "{") depth++;
          else if (c === "}") depth--;
          else if (c === '"' || c === "'" || c === "`") {
            const inner = scan(src.slice(k, skipString(src, k)));
            out.push(...inner);
            k = skipString(src, k) - 1;
          }
          k++;
        }
        exprs.push(src.slice(j + 2, k - 1));
        j = k;
      } else {
        chunk += src[j++];
      }
    }
    parts.push(unescape(chunk));
    out.push({ kind: "template", parts, exprs });
    i = j + 1;
  }

  while (i < n) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      while (i < n && src[i] !== "\n") i++;
    } else if (c === "/" && src[i + 1] === "*") {
      i = src.indexOf("*/", i + 2);
      i = i < 0 ? n : i + 2;
    } else if (c === '"' || c === "'") {
      quoted(c);
      last = "a";
    } else if (c === "`") {
      template();
      last = "a";
    } else if (c === "/" && (REGEX_AFTER.has(last) || REGEX_WORDS.test(lastWord))) {
      // A regular expression literal: skip it, classes and escapes included
      let j = i + 1;
      let inClass = false;
      while (j < n && src[j] !== "\n" && (inClass || src[j] !== "/")) {
        if (src[j] === "\\") j++;
        else if (src[j] === "[") inClass = true;
        else if (src[j] === "]") inClass = false;
        j++;
      }
      i = j + 1;
      while (i < n && /[a-z]/.test(src[i])) i++;
      last = "a";
    } else {
      if (!/\s/.test(c)) {
        last = c;
        lastWord = /[\w$]/.test(c) ? lastWord + c : "";
        if (!/[\w$]/.test(c)) lastWord = "";
      } else if (lastWord) {
        lastWord += " ";
      }
      if (lastWord.length > 40) lastWord = lastWord.slice(-20);
      i++;
    }
  }
  return out;
}

/** The index just after the string or template that opens at `k` */
function skipString(src, k) {
  const q = src[k];
  let j = k + 1;
  let depth = 0;
  while (j < src.length) {
    const c = src[j];
    if (c === "\\") j += 2;
    else if (q === "`" && c === "$" && src[j + 1] === "{") {
      depth++;
      j += 2;
    } else if (q === "`" && depth > 0 && c === "}") {
      depth--;
      j++;
    } else if (depth === 0 && c === q) return j + 1;
    else j++;
  }
  return j;
}

const CLASSY = /^[a-z0-9_.:#/-]+$/;

/** True when `text` reads as words shown to a person, not code, a class list or a path */
export function isText(text) {
  const t = text.trim();
  if (t.length < 2 || !/[A-Za-z]{2}/.test(t)) return false;
  if (/\[[a-z-]+[=\]]|^[a-z-]+:\S|^[/#.?&=]|:\/\/|^\w+\.(js|css|json|html|srt|mkv)$|^[\w-]+=|^\$\{|^[A-Z][A-Z0-9_]+$|^[a-z][\w.]*\(|=>|^<\/?[a-z]/.test(t)) return false;
  // CamelCase values (ByDate) and long unbroken tokens are code, not words
  if (/^[A-Z][a-z]+[A-Z]\w*$/.test(t) || (!/\s/.test(t) && t.length > 30)) return false;
  if (/^[A-Z]/.test(t)) return true;
  // lower case: several words, not all of them class names or identifiers
  const words = t.split(/\s+/);
  return words.length > 1 && !words.every((w) => CLASSY.test(w.replace(/,$/, "")) || /^[a-z]+[A-Z]\w*$/.test(w));
}

/** The catalog key for a template, or null when it isn't UI text */
export function templateKey({ parts, exprs }) {
  if (!exprs.length) return isText(parts[0]) ? parts[0] : null;
  // A value glued to a letter (`${n} file${s}`, `${base}ing`) changes the wording itself
  for (let k = 0; k < exprs.length; k++) {
    if (/[A-Za-z]$/.test(parts[k]) || /^[A-Za-z]/.test(parts[k + 1])) return null;
  }
  const literal = parts.join(" ");
  if (!/[A-Za-z]{2}/.test(literal) || /^[/#?]|:\/\/|^[a-z0-9_-]*$/.test(parts[0])) return null;
  const key = parts.reduce((acc, part, k) => acc + (k ? `{${k - 1}}` : "") + part, "");
  // The words around the values must read as text on their own (`dot ${kind}` is a class)
  const words = parts.join(" ").replace(/\s+/g, " ").trim();
  if (!words || !isText(/^[A-Z]/.test(key) ? key.replace(/\{\d+\}/g, "X") : words)) return null;
  if (/^[\s{}\d·:,()/×%-]*$/.test(parts.join("")) || !isText(key.replace(/\{\d+\}/g, "0"))) return null;
  return key;
}

/** Text nodes and title/placeholder/aria-label values in index.html */
function htmlStrings(html) {
  const body = html.replace(/<script[\s\S]*?<\/script>|<style[\s\S]*?<\/style>|<!--[\s\S]*?-->/g, "");
  const found = [];
  for (const m of body.matchAll(/>([^<>]+)</g)) found.push(m[1].replace(/\s+/g, " "));
  for (const m of body.matchAll(/\s(?:title|placeholder|aria-label)="([^"]*)"/g)) found.push(m[1]);
  return found.map((s) => s.trim()).filter(isText);
}

function jsFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "i18n" ? [] : jsFiles(p);
    return e.name.endsWith(".js") && !["i18n.js", "translate.js"].includes(e.name) ? [p] : [];
  });
}

/** The sorted catalog keys for the UI under `dir` */
export function extract(dir = UI_DIR) {
  const keys = new Set(htmlStrings(readFileSync(join(dir, "index.html"), "utf8")));
  for (const file of jsFiles(dir)) {
    for (const s of scan(readFileSync(file, "utf8"))) {
      const key = s.kind === "string" ? (isText(s.value) ? s.value : null) : templateKey(s);
      if (key && key.trim()) keys.add(key.trim());
    }
  }
  return [...keys].sort();
}

/** en.json's text for `keys`: each English string maps to itself */
export function catalogText(keys) {
  return JSON.stringify(Object.fromEntries(keys.map((k) => [k, k])), null, 1) + "\n";
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const text = catalogText(extract());
  if (process.argv.includes("--check")) {
    let now = "";
    try {
      now = readFileSync(CATALOG, "utf8");
    } catch {
      // missing: out of date
    }
    if (now !== text) {
      console.error("putio-sync-api/ui/i18n/en.json is out of date: run `make i18n`");
      process.exit(1);
    }
  } else {
    writeFileSync(CATALOG, text);
    console.log(`${Object.keys(JSON.parse(text)).length} strings in ${CATALOG}`);
  }
}
