// Bandwidth schedules (feature 032): limits for people, and the rows of the Settings
// schedule editor to and from `[download] bandwidth_schedule` entries. No DOM here, so
// tests/ui/bandwidth.test.mjs runs it in node.

const MIB = 1048576;

/** A limit for people: "Unlimited", "Paused", "5.0 MB/s", "512 KB/s" */
export function rate(limit) {
  if (limit == null || limit === "unlimited") return "Unlimited";
  if (limit === "paused" || limit === 0) return "Paused";
  const n = Number(limit);
  if (!Number.isFinite(n)) return String(limit);
  if (n >= MIB) return `${(n / MIB).toFixed(1)} MB/s`;
  if (n >= 1024) return `${Math.floor(n / 1024)} KB/s`;
  return `${n} B/s`;
}

/** Where the limit comes from, for people */
export function describeSource(source, entry) {
  switch (source) {
    case "schedule":
      return `Schedule entry ${entry}`;
    case "pause":
      return "Paused from the Status page";
    case "slow":
      return "Slowed from the Status page";
    default:
      return "Default limit";
  }
}

/** Bytes per second as MB/s for an input: 5242880 → "5", 1572864 → "1.5" */
export function toMBs(bytes) {
  return String(Number((Number(bytes) / MIB).toFixed(2)));
}

/** MB/s typed by a person as bytes per second; null when it is not a positive number */
export function fromMBs(text) {
  const n = Number(String(text).trim().replace(",", "."));
  if (String(text).trim() === "" || !Number.isFinite(n) || n <= 0) return null;
  return Math.max(1, Math.round(n * MIB));
}

/** A schedule entry for people: "mon-fri 18:00–23:00 Paused" */
export function describeEntry(entry) {
  return `${entry.days} ${entry.start}–${entry.end} ${rate(entry.limit)}`;
}

/** A schedule entry as an editor row: `kind` is "unlimited", "paused" or "rate" */
export function rowFromEntry(entry) {
  const limit = entry.limit;
  const kind = limit === "unlimited" || limit === "paused" ? limit : "rate";
  return { days: entry.days, start: entry.start, end: entry.end, kind, mbs: kind === "rate" ? toMBs(limit) : "" };
}

/**
 * An editor row as a schedule entry. A rate that is not a positive number is sent as
 * typed, so the save is refused with the entry and field named.
 */
export function entryFromRow(row) {
  const entry = { days: row.days.trim(), start: row.start.trim(), end: row.end.trim() };
  if (row.kind !== "rate") return { ...entry, limit: row.kind };
  const bytes = fromMBs(row.mbs);
  return { ...entry, limit: bytes ?? String(row.mbs).trim() };
}

/** Swap item `i` with its neighbour `i + by` (-1 up, 1 down); out of range does nothing */
export function move(list, i, by) {
  const j = i + by;
  if (i < 0 || j < 0 || i >= list.length || j >= list.length) return list;
  const out = list.slice();
  [out[i], out[j]] = [out[j], out[i]];
  return out;
}
