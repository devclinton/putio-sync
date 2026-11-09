// Translation (feature 051), loaded by i18n.js only when a language other than English is
// in use. English text is the key; text nodes and the title, placeholder and aria-label
// attributes are translated as they reach the page, so pages need no changes. Anything
// inside code, pre, textarea or [translate=no] is data and left alone.

const ATTRS = ["title", "placeholder", "aria-label"];
const SKIP = "script, style, code, pre, textarea, [translate=no]";

let exact = new Map();
let patterns = [];
// English as written by the page, per text node and per attribute, so a switch can redo it
const texts = new WeakMap();
const attrs = new WeakMap();

/** Compile a catalog: exact strings, and {n} keys as patterns. Entries that lose a {n} are dropped. */
export function compile(catalog) {
  const ex = new Map();
  const pats = [];
  for (const [en, out] of Object.entries(catalog || {})) {
    if (typeof out !== "string" || !out.trim()) continue;
    const holes = en.match(/\{\d+\}/g);
    if (!holes) {
      ex.set(en, out);
      continue;
    }
    if (holes.some((x) => !out.includes(x))) continue;
    const pieces = en.split(/\{\d+\}/);
    const re = new RegExp(`^${pieces.map((p) => p.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("(.+?)")}$`, "s");
    pats.push({ re, out, order: holes.map((x) => +x.slice(1, -1)), anchor: pieces.reduce((a, b) => (b.length > a.length ? b : a)) });
  }
  return { exact: ex, patterns: pats };
}

/** `text` in the current language; untouched when the catalog doesn't have it */
export function tr(text, ex = exact, pats = patterns) {
  if (!ex.size || !text) return text;
  const t = text.trim();
  if (!t || !/\p{L}/u.test(t)) return text;
  let out = ex.get(t);
  if (out == null) {
    for (const p of pats) {
      if (!t.includes(p.anchor)) continue;
      const m = p.re.exec(t);
      if (!m) continue;
      out = p.out;
      p.order.forEach((n, k) => (out = out.split(`{${n}}`).join(ex.get(m[k + 1]) ?? m[k + 1])));
      break;
    }
  }
  return out == null ? text : text.match(/^\s*/)[0] + out + text.match(/\s*$/)[0];
}

function textNode(node) {
  const seen = texts.get(node);
  const en = seen && node.data === seen.shown ? seen.en : node.data;
  const shown = tr(en);
  texts.set(node, { en, shown });
  if (shown !== node.data) node.data = shown;
}

function attribute(el, name) {
  const value = el.getAttribute(name);
  if (value == null) return;
  const all = attrs.get(el) || {};
  const seen = all[name];
  const en = seen && value === seen.shown ? seen.en : value;
  const shown = tr(en);
  all[name] = { en, shown };
  attrs.set(el, all);
  if (shown !== value) el.setAttribute(name, shown);
}

const skipped = (node) => {
  const el = node.nodeType === 1 ? node : node.parentElement;
  return !el || el.closest(SKIP);
};

function walk(root) {
  if (root.nodeType === 3) return skipped(root) || textNode(root);
  if (root.nodeType !== 1 || skipped(root)) return;
  for (const name of ATTRS) attribute(root, name);
  const it = document.createTreeWalker(root, 5, { acceptNode: (n) => (n.nodeType === 1 && n.matches(SKIP) ? 2 : 1) });
  for (let n = it.nextNode(); n; n = it.nextNode()) {
    if (n.nodeType === 3) textNode(n);
    else for (const name of ATTRS) attribute(n, name);
  }
}

const onMutations = (records) => {
  for (const r of records) {
    if (r.type === "childList") r.addedNodes.forEach(walk);
    else if (r.type === "characterData") skipped(r.target) || textNode(r.target);
    else if (!skipped(r.target)) attribute(r.target, r.attributeName);
  }
};
let observer = null;

/** Translate the page with `catalog` (empty for English) and keep translating what is added */
export function apply(catalog) {
  ({ exact, patterns } = compile(catalog));
  observer?.disconnect();
  observer ||= new MutationObserver(onMutations);
  walk(document.body);
  observer.observe(document.body, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ATTRS });
}
