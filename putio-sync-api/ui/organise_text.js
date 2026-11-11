// Organise on Put.io (feature 023): the pure parts of ui/organise.js, tested under node.

/** Which actions a move in `status` takes */
export const ACTIONS = {
  planned: ["apply", "dismiss"],
  failed: ["apply", "dismiss"],
  undone: ["apply", "dismiss"],
  applied: ["undo"],
};

/** `folder/name`, with `/` for the root */
export const pathOf = (folder, name) => (folder ? `${folder}/${name}` : name);

/** The ids of `moves` in `picked` that `action` takes */
export function idsFor(moves, picked, action) {
  return moves.filter((m) => picked.has(m.file_id) && (ACTIONS[m.status] || []).includes(action)).map((m) => m.file_id);
}

/** One line for a batch's results: how many worked and the first error */
export function outcomeText(results, done) {
  const ok = results.filter((r) => r.status === done).length;
  const failed = results.filter((r) => r.status === "failed");
  const parts = [];
  if (ok) parts.push(`${ok} ${done}`);
  if (failed.length) parts.push(`${failed.length} failed: ${failed[0].error || "unknown error"}`);
  return parts.join(" · ") || "Nothing to do";
}

/** Moves by state, planned first */
export function sortMoves(moves) {
  const order = { planned: 0, failed: 1, applied: 2, undone: 3 };
  return [...moves].sort((a, b) => (order[a.status] ?? 9) - (order[b.status] ?? 9) || (b.updated_at || "").localeCompare(a.updated_at || ""));
}
