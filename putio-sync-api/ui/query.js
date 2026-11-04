// List filter and sort (feature 045), shared by the Put.io, Transfers and Local files pages,
// and the language of saved patterns (feature 020, mirrored by putio_sync_core::pattern).
// No DOM, so tests/ui/query.test.mjs runs it under Node, with the shared cases in
// putio-sync-core/tests/data/filter-cases.json. The language is in
// specs/020-glob-filters/contracts/pattern-grammar.md: spaces AND, OR, | or , between
// terms (tighter than AND), - or NOT negates, * and ? are globs that match the whole name,
// {a,b} is either, a plain term matches anywhere, a term with / is tested against the
// path, quotes keep spaces; case never matters.

/** Words and operators: `{ text, quoted, neg }`, with `|` and `,` split out even inside a word (but not inside `{…}`) */
function tokens(text) {
  const out = [];
  const re = /(-?)"([^"]*)"?|[|,]+|(?:\{[^}\s"|]*\}?|[^\s|",{])+/g;
  for (const m of text.matchAll(re)) out.push(m[2] != null ? { text: m[2], quoted: true, neg: !!m[1] } : { text: m[0], quoted: false });
  return out;
}

const word = (t, w) => !t.quoted && t.text.toUpperCase() === w;
const isOr = (t) => word(t, "OR") || (!t.quoted && (t.text[0] === "|" || t.text[0] === ","));

/** A case-insensitive matcher for one pattern: anchored when it has * or ?; `{a,b}` is a group */
function pattern(text) {
  const glob = /[*?]/.test(text);
  const part = (s) => [...s].map((c) => (c === "*" && glob ? ".*" : c === "?" && glob ? "." : c.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&"))).join("");
  const body = text
    .split(/(\{[^{}]*\})/)
    .map((s, i) => (i % 2 ? `(?:${s.slice(1, -1).split(",").map(part).join("|")})` : part(s)))
    .join("");
  return new RegExp(glob ? `^${body}$` : body, "isu");
}

/** The path and each tail of it after a `/` */
const tails = (path) => [path, ...[...path.matchAll(/\//g)].map((m) => path.slice(m.index + 1))];

/** "a b|-c" → [[a], [b, -c]]: AND-ed groups of OR-ed `{ re, not, path }` terms */
export function parseFilter(text) {
  const groups = [];
  let group = null;
  let joining = false; // the last token was OR
  let not = false;
  for (const t of tokens(String(text || ""))) {
    if (isOr(t)) {
      joining = group != null;
      continue;
    }
    if (word(t, "AND")) {
      joining = false;
      continue;
    }
    if (word(t, "NOT")) {
      not = true;
      continue;
    }
    let p = t.text;
    let neg = not || !!t.neg;
    not = false;
    if (!t.quoted && p[0] === "-") {
      neg = true;
      p = p.slice(1);
    }
    if (!p) continue;
    const term = { re: pattern(p), not: neg, path: p.includes("/") };
    if (joining && group) group.push(term);
    else groups.push((group = [term]));
    joining = false;
  }
  return groups;
}

/**
 * `(name, path = name) => bool` for `text`; an empty filter matches every name. `path` is
 * `Folder/Sub/name`; terms with `/` test it and its tails (a non-string, such as the index
 * `Array.filter` passes, counts as no path).
 */
export function filterMatcher(text) {
  const groups = parseFilter(text);
  return (name, path) => {
    const n = String(name ?? "");
    const p = typeof path === "string" ? path : n;
    return groups.every((g) => g.some((t) => (t.path ? tails(p).some((s) => t.re.test(s)) : t.re.test(n)) !== t.not));
  };
}

/** "size-desc" → { key: "size", desc: true }; null unless `key` is one of `keys` */
export function parseSort(text, keys) {
  const m = /^(\w+)-(asc|desc)$/.exec(text || "");
  return m && keys.includes(m[1]) ? { key: m[1], desc: m[2] === "desc" } : null;
}

export const formatSort = ({ key, desc }) => `${key}-${desc ? "desc" : "asc"}`;

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function compare(a, b) {
  if (a == null || b == null) return a == null ? (b == null ? 0 : -1) : 1;
  return typeof a === "string" || typeof b === "string" ? collator.compare(String(a), String(b)) : a - b;
}

/**
 * A sorted copy of `rows`. `columns`: `{ key: (row) => value }`; strings compare by
 * collation, numbers numerically, missing values lowest. Ties go by name (ascending),
 * then the given order. With `folder`, folders stay first in both directions.
 */
export function sortRows(rows, sort, columns, { folder = null } = {}) {
  if (!sort || !columns[sort.key]) return [...rows];
  const value = columns[sort.key];
  const name = columns.name || (() => null);
  const dir = sort.desc ? -1 : 1;
  return rows
    .map((row, i) => ({ row, i }))
    .sort((a, b) => (folder ? Number(!!folder(b.row)) - Number(!!folder(a.row)) : 0) || dir * compare(value(a.row), value(b.row)) || compare(name(a.row), name(b.row)) || a.i - b.i)
    .map((x) => x.row);
}
