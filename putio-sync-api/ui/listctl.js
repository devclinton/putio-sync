// Filter box, Sort menu and sortable column headings for a list page (feature 045). The
// filter and sort are kept in the address as `q` and `sort` (replaced, not pushed, so
// typing doesn't add history or remount the page). The language is in query.js.

import { h } from "./app.js";
import { filterMatcher, formatSort, parseSort, sortRows } from "./query.js";

/**
 * `columns`: `{ key: { label, value: (row) => sortable } }`, with a `name` column (what
 * the filter matches). `onChange` runs after the filter or sort changes.
 */
export function listControls({ query, columns, onChange, label = "Filter by name" }) {
  const keys = Object.keys(columns);
  let text = (query && query.get("q")) || "";
  let sort = parseSort(query && query.get("sort"), keys);
  let match = filterMatcher(text);
  const values = Object.fromEntries(keys.map((k) => [k, columns[k].value]));

  const filter = h("input", { type: "search", class: "input", placeholder: "Filter: *.mkv -sample", title: "Spaces: all must match. OR or |: either. -word or NOT: leave out. * and ? are wildcards.", "aria-label": label, value: text, oninput: () => ((text = filter.value), (match = filterMatcher(text)), changed()) });
  const menu = h(
    "select",
    { class: "input sort-menu", "aria-label": "Sort", onchange: () => ((sort = parseSort(menu.value, keys)), changed()) },
    h("option", { value: "", text: "Default order" }),
    keys.flatMap((k) => [false, true].map((desc) => h("option", { value: formatSort({ key: k, desc }), text: `${columns[k].label} ${desc ? "↓" : "↑"}` }))),
  );
  menu.value = sort ? formatSort(sort) : "";

  function changed() {
    menu.value = sort ? formatSort(sort) : "";
    const [route, search = ""] = window.location.hash.split("?", 2);
    const params = new URLSearchParams(search);
    for (const [k, v] of [["q", text.trim()], ["sort", sort ? formatSort(sort) : ""]]) v ? params.set(k, v) : params.delete(k);
    const qs = params.toString();
    history.replaceState(null, "", `${route}${qs ? `?${qs}` : ""}`);
    onChange();
  }

  /** A heading that sorts by `key` on click, and again the other way */
  function th(key, attrs = {}) {
    const on = sort && sort.key === key;
    const flip = () => ((sort = on && !sort.desc ? { key, desc: true } : { key, desc: false }), changed());
    return h(
      "th",
      { ...attrs, "aria-sort": on ? (sort.desc ? "descending" : "ascending") : null },
      h("button", { type: "button", class: `sort-head${on ? " on" : ""}`, onclick: flip }, columns[key].label, h("span", { class: "sort-arrow", "aria-hidden": "true", text: on ? (sort.desc ? " ↓" : " ↑") : "" })),
    );
  }

  return {
    filter,
    menu,
    th,
    /** True while the filter hides anything it could */
    get filtering() {
      return text.trim() !== "";
    },
    /** The rows the filter shows, sorted; `folder(row)` keeps folders first */
    apply: (rows, { folder = null } = {}) => sortRows(rows.filter((r) => match(values.name(r))), sort, values, { folder }),
    /** The `sort=…` pair for links into another folder (they drop the filter) */
    carry: () => (sort ? `sort=${formatSort(sort)}` : ""),
  };
}
