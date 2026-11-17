// Loose matching for the command palette (feature 061): the typed letters appear in the text
// in order, ignoring case, and a letter only enters another word at that word's start. Matches
// at the start of a word and runs of adjacent letters score higher; gaps and a late first
// letter score lower. No DOM, so it is tested with node.

const WORD = /[\s\-_./()[\]›·:]/;

/** The score of `query` in `text`, higher is better, or null when it does not match */
export function match(query, text) {
  const q = String(query || "").toLowerCase().replace(/\s+/g, "");
  const t = String(text || "").toLowerCase();
  if (!q) return 0;
  const best = place(q, t, 0, -1, 0, new Map());
  if (best == null) return null;
  // Prefer shorter texts and an early first letter, so "Queue" beats "Clear the queue"
  return best - t.length * 0.05 - t.indexOf(q[0]) * 0.2;
}

const isStart = (t, i) => i === 0 || WORD.test(t[i - 1]);

/**
 * The best score for placing q[i..] after t[at], or null. A letter after the first goes next
 * to the one before it, at a word start, or later in the same word: never into the middle of
 * another word, so "show" does not find "Skip tHese categOries..."
 */
function place(q, t, i, at, run, memo) {
  if (i === q.length) return 0;
  const key = `${i},${at},${run}`;
  if (memo.has(key)) return memo.get(key);
  let best = null;
  let sameWord = true;
  for (let j = at + 1; j < t.length; j++) {
    if (j > at + 1 && WORD.test(t[j - 1])) sameWord = false;
    if (t[j] !== q[i]) continue;
    const start = isStart(t, j);
    const next = j === at + 1;
    if (i > 0 && !next && !start && !sameWord) continue;
    const r = next && i > 0 ? run + 1 : 0;
    const rest = place(q, t, i + 1, j, r, memo);
    if (rest == null) continue;
    const s = 1 + (start ? 8 : 0) + r * 4 - Math.min(j - at - 1, 10) * 0.5 + rest;
    if (best == null || s > best) best = s;
  }
  memo.set(key, best);
  return best;
}

/** The `limit` best of `items` for `query`; `text(item)` gives the words matched */
export function rank(query, items, text, limit = 8) {
  const scored = [];
  for (const item of items) {
    const s = match(query, text(item));
    if (s != null) scored.push([s, item]);
  }
  scored.sort((a, b) => b[0] - a[0]);
  return scored.slice(0, limit).map(([, item]) => item);
}
