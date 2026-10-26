// Row selection for a table (feature 008): a checkbox per row, a "Select all" box above
// the table (mixed while only some rows are picked; it works at phone width too, where
// the table header is hidden), and action buttons enabled while anything is selected.
// Only rows passed to `reset` can be selected, so actions never reach a hidden row.

import { h } from "./app.js";

/**
 * `actions`: buttons for the bar, enabled only while something is selected.
 * Render rows with `box(id, ...)` after `reset(selectableIds)`, then call `update()`.
 */
export function selection({ actions = [] } = {}) {
  const ids = new Set();
  const boxes = new Map();
  let selectable = new Set();
  let busy = false;

  const all = h("input", { type: "checkbox", onchange: () => (all.checked ? selectable.forEach((id) => ids.add(id)) : ids.clear(), update()) });
  const count = h("span", { class: "selection-count muted", "aria-live": "polite" });
  const clear = h("button", { type: "button", class: "btn btn-small btn-quiet", onclick: () => (ids.clear(), update()) }, "Clear selection");
  const bar = h("div", { class: "selection-bar" }, h("label", { class: "select-all" }, all, "Select all"), count, h("div", { class: "row-actions" }, ...actions, clear));

  /** Keep only `next` selectable; selected ids not in it are dropped. */
  function reset(next) {
    selectable = new Set(next);
    for (const id of ids) if (!selectable.has(id)) ids.delete(id);
    boxes.clear();
  }

  /** The checkbox for row `id`; `disabled` rows can't be picked (`title` says why). */
  function box(id, { label, disabled = false, title = null }) {
    const input = h("input", {
      type: "checkbox",
      "aria-label": label,
      title,
      disabled: disabled || !selectable.has(id),
      onchange: () => (input.checked ? ids.add(id) : ids.delete(id), update()),
    });
    input.checked = ids.has(id);
    boxes.set(id, input);
    return h("td", { class: "select-cell" }, input);
  }

  function update() {
    const n = ids.size;
    all.checked = n > 0 && n === selectable.size;
    all.indeterminate = n > 0 && n < selectable.size;
    all.disabled = selectable.size === 0;
    count.textContent = n ? `${n} selected` : "";
    for (const b of [...actions, clear]) b.disabled = busy || n === 0;
    for (const [id, input] of boxes) {
      input.checked = ids.has(id);
      const row = input.closest("tr");
      if (row) row.classList.toggle("selected", input.checked);
    }
  }

  return {
    bar,
    box,
    reset,
    update,
    /** Selected ids, in selection order. */
    get ids() {
      return [...ids];
    },
    has: (id) => ids.has(id),
    /** Disable the bar's buttons while an action runs. */
    busy(flag) {
      busy = flag;
      update();
    },
    clear() {
      ids.clear();
      update();
    },
  };
}
