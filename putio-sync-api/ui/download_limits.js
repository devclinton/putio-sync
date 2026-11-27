// Feature 064: the wording and small rules behind download hours, the disk pause, the
// storage budget and the forecast. Plain functions with no DOM, so
// tests/ui/download_limits.test.mjs can run them under `node --test`.

import { bytes } from "./format.js";

/** A `download.windows` entry as an editor row, and back (trimmed, as typed otherwise) */
export const windowRow = (entry) => ({ days: entry.days, start: entry.start, end: entry.end });
export const windowEntry = (row) => ({ days: String(row.days).trim(), start: String(row.start).trim(), end: String(row.end).trim() });

/** "mon-fri 23:00–07:00" */
export const describeWindow = (entry) => `${entry.days} ${entry.start}–${entry.end}`;

/**
 * Whether the budget may not be on for `percent`: it needs a preview of that percent,
 * unless it is already saved on with that same percent.
 */
export function budgetLocked(saved, percent, previewed) {
  if (!Number.isInteger(percent) || percent < 10 || percent > 99) return true;
  if (saved && saved.budget_enabled && saved.budget_used_percent === percent) return false;
  return previewed !== percent;
}

/** What a budget preview found: one sentence per entry */
export function budgetPreviewLines(plan, percent) {
  const n = plan.files.length;
  if (plan.needed_bytes <= 0) return [`Put.io is under ${percent}% used now, so nothing would be deleted.`];
  if (!n) return [`Put.io is ${bytes(plan.needed_bytes)} over ${percent}% used, but no file is confirmed downloaded, so nothing would be deleted.`];
  const lines = [`Put.io is ${bytes(plan.needed_bytes)} over ${percent}% used.`, n === 1 ? `Would be deleted from Put.io now: 1 file, ${bytes(plan.total_bytes)}.` : `Would be deleted from Put.io now: ${n} files, ${bytes(plan.total_bytes)}.`];
  if (plan.shortfall_bytes > 0) lines.push(`That leaves it ${bytes(plan.shortfall_bytes)} over: nothing else is confirmed downloaded.`);
  return lines;
}

/** Who started a storage run, from its recorded `trigger` */
export const runBy = (trigger) => (trigger === "schedule" ? "automatic" : trigger === "budget" ? "storage budget" : "manual");

const day = (iso) => new Date(iso).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });

/**
 * One disk of GET /stats/forecast as `{ use, text, tone }`: how full it is (null before
 * the first reading), when it fills or why that can't be said, and "warn" under 14 days.
 */
export function forecastLine(d) {
  if (!d) return null;
  const use = d.total_bytes != null && d.used_bytes != null ? `${bytes(d.used_bytes)} used of ${bytes(d.total_bytes)}` : null;
  const out = (text, tone = "muted") => ({ use, text, tone });
  switch (d.state) {
    case "full":
      return out("Full now.", "warn");
    case "filling": {
      const rate = bytes(d.rate_bytes_per_day);
      const tone = d.days_left < 14 ? "warn" : "muted";
      if (d.days_left < 1) return out(`Full in less than a day at the rate of the last week (${rate} a day).`, tone);
      const days = Math.round(d.days_left);
      return out(days === 1 ? `Full in about 1 day, around ${day(d.full_at)}, at the rate of the last week (${rate} a day).` : `Full in about ${days} days, around ${day(d.full_at)}, at the rate of the last week (${rate} a day).`, tone);
    }
    case "not_growing":
      return out("Not filling up: over the last week its use was level or falling.");
    case "not_enough_data":
      return out(d.samples ? `Not enough data yet: a forecast needs a day of hourly readings, and there are ${d.span_hours} h so far.` : "Not enough data yet: no reading has been taken.");
    default:
      return null;
  }
}
