// Download verification (feature 034): labels and summaries for the History page's
// Verification panel and Verified column. No DOM here, so tests/ui/verify.test.mjs runs
// it in node.

/** How a download was checked, for the Verified column */
export function methodLabel(verify, redownloads) {
  const base = { crc32: "CRC32", size: "Size", none: "Not checked" }[verify] || "—";
  return redownloads > 0 ? `${base} · re-downloaded ${redownloads}×` : base;
}

/** A run or file result for people */
export function resultLabel(result) {
  return (
    {
      verified: "Verified",
      mismatch: "Does not match Put.io",
      missing: "Missing on disk",
      not_on_putio: "Not on Put.io",
      busy: "Being downloaded",
      unreadable: "Could not be read",
      not_checked: "Not checked",
      failed: "Gave up after re-downloads",
    }[result] || result
  );
}

/** Run state for people */
export function stateLabel(state) {
  return (
    {
      idle: "Idle",
      running: "Running",
      paused: "Paused",
      waiting: "Waiting",
      stopping: "Stopping",
      finished: "Finished",
      stopped: "Stopped",
      failed: "Failed",
    }[state] || state
  );
}

/** Percent of a run done, by bytes when there are any, else by files; 0..100 */
export function progressPercent(status) {
  if (!status) return 0;
  const [done, total] = status.bytes_total > 0 ? [status.bytes_done, status.bytes_total] : [status.files_done, status.files_total];
  if (!total) return 0;
  return Math.min(100, Math.max(0, (100 * done) / total));
}

// The summary's words, in its order
const SHORT = [
  ["verified", "verified"],
  ["mismatch", "mismatched"],
  ["missing", "missing"],
  ["not_on_putio", "not on Put.io"],
  ["busy", "busy"],
  ["unreadable", "unreadable"],
  ["not_checked", "not checked"],
];

/** "310 of 1200 files: 305 verified, 1 mismatched, 3 missing" */
export function summaryLine(run) {
  if (!run) return "No verify run yet.";
  const counts = run.counts || run;
  const parts = SHORT.filter(([key]) => counts[key] > 0).map(([key, word]) => `${counts[key]} ${word}`);
  const head = run.files_done === run.files_total ? `${run.files_total} ${run.files_total === 1 ? "file" : "files"}` : `${run.files_done} of ${run.files_total} files`;
  return parts.length ? `${head}: ${parts.join(", ")}` : head;
}

/** Whether the run may be paused, resumed or stopped */
export function controls(state) {
  const going = state === "running" || state === "waiting" || state === "paused";
  return { start: state === "idle", pause: state === "running" || state === "waiting", resume: state === "paused", stop: going };
}
