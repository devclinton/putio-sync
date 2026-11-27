// Page tours (feature 014): which pages have been toured in this browser, moving between
// steps, and where the tour card sits. No DOM, so it is tested with node; tour_ui.js draws
// the card and tour_content.js holds the words.

export const TOUR_KEY = "putio-sync.tours";

// Private mode: storage can refuse writes, so a tour seen in this page load stays seen
const memory = new Set();

/** This browser's storage, or null where even asking for it throws */
export function store() {
  try {
    return globalThis.localStorage || null;
  } catch {
    return null;
  }
}

/** `{ page: 1 }` for every page whose tour has been shown here */
export function readSeen(storage) {
  let saved = null;
  try {
    saved = JSON.parse(storage.getItem(TOUR_KEY));
  } catch {
    // no storage, or not JSON: nothing saved
  }
  const seen = saved && typeof saved === "object" && !Array.isArray(saved) ? { ...saved } : {};
  for (const page of memory) seen[page] = 1;
  return seen;
}

/** Remember that these pages' tours were shown; false when it could not be saved */
export function markSeen(storage, pages) {
  for (const page of pages) memory.add(page);
  try {
    storage.setItem(TOUR_KEY, JSON.stringify(readSeen(storage)));
    return true;
  } catch {
    return false;
  }
}

/** Forget every tour, so each page shows its tour again */
export function resetSeen(storage) {
  memory.clear();
  try {
    storage.removeItem(TOUR_KEY);
  } catch {
    // nothing was saved
  }
}

/** True when `page` should offer its tour: never while signing in or setting up */
export function due(page, seen, { setup = false } = {}) {
  if (setup || !page || ["login", "setup", "signin"].includes(page)) return false;
  return !seen[page];
}

/** The step after moving `by` from `index`; null once the tour is past its last step */
export function move(index, count, by) {
  const next = index + by;
  if (next >= count) return null;
  return Math.max(0, next);
}

const MARGIN = 12;
const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/**
 * Where the card goes. `target` is the highlighted element's box (or null when the page
 * does not have it), `card` and `view` are sizes. At phone width the card is a sheet along
 * the bottom; otherwise it sits below, above or beside the target, and never off screen.
 */
export function place(target, card, view, gap = 12) {
  if (view.width <= 600) return { sheet: true };
  const maxLeft = view.width - card.width - MARGIN;
  const maxTop = view.height - card.height - MARGIN;
  if (!target) return { sheet: false, top: clamp(maxTop - MARGIN, MARGIN, maxTop), left: clamp((view.width - card.width) / 2, MARGIN, maxLeft) };
  const left = clamp(target.left, MARGIN, maxLeft);
  const top = clamp(target.top, MARGIN, maxTop);
  if (target.bottom + gap + card.height <= view.height - MARGIN) return { sheet: false, top: target.bottom + gap, left };
  if (target.top - gap - card.height >= MARGIN) return { sheet: false, top: target.top - gap - card.height, left };
  if (target.right + gap + card.width <= view.width - MARGIN) return { sheet: false, top, left: target.right + gap };
  if (target.left - gap - card.width >= MARGIN) return { sheet: false, top, left: target.left - gap - card.width };
  return { sheet: false, top: maxTop, left: clamp(maxLeft, MARGIN, maxLeft) };
}
