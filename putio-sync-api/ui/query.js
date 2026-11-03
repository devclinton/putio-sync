// List filter and sort (feature 045), shared by the Put.io, Transfers and Local files pages.
// No DOM, so tests/ui/query.test.mjs runs it under Node. The filter language is in
// specs/045-select-sort-filter/contracts/filter-grammar.md: spaces AND, OR or | between
// terms (tighter than AND), - or NOT negates, * and ? are globs that match the whole name,
// a plain term matches anywhere, quotes keep spaces; case never matters.

/** Words and operators: `{ text, quoted, neg }`, with `|` split out even inside a word */
function tokens(text) {
  const out = [];
  const re = /(-?)"([^"]*)"?|\|+|[^\s|"]+/g;
  for (const m of text.matchAll(re)) out.push(m[2] != null ? { text: m[2], quoted: true, neg: !!m[1] } : { text: m[0], quoted: false });
  return out;
}

const word = (t, w) => !t.quoted && t.text.toUpperCase() === w;
const isOr = (t) => word(t, "OR") || (!t.quoted && t.text[0] === "|");

/** A case-insensitive matcher for one pattern: anchored when it has * or ? */
function pattern(text) {
  const glob = /[*?]/.test(text);
  const body = [...text].map((c) => (c === "*" && glob ? ".*" : c === "?" && glob ? "." : c.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))).join("");
  return new RegExp(glob ? `^${body}$` : body, "is");
}

/** "a b|-c" → [[a], [b, -c]]: AND-ed groups of OR-ed `{ re, not }` terms */
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
    const term = { re: pattern(p), not: neg };
    if (joining && group) group.push(term);
    else groups.push((group = [term]));
    joining = false;
  }
  return groups;
}

/** `(name) => bool` for `text`; an empty filter matches every name */
export function filterMatcher(text) {
  const groups = parseFilter(text);
  return (name) => groups.every((g) => g.some((t) => t.re.test(String(name ?? "")) !== t.not));
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
