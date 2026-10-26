// Display formatting for sizes, durations and times.

const DASH = "—";
const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** 1536 → "1.5 KB" (binary units, one decimal). */
export function bytes(n) {
  if (n == null || !Number.isFinite(Number(n))) return DASH;
  let value = Number(n);
  let unit = 0;
  while (Math.abs(value) >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${Math.round(value)} B` : `${value.toFixed(1)} ${UNITS[unit]}`;
}

const pad = (n) => String(n).padStart(2, "0");

/** 45 → "45s", 185 → "3m 05s", 3720 → "1h 02m", 93600 → "1d 02h". */
export function duration(seconds) {
  if (seconds == null || !Number.isFinite(Number(seconds))) return DASH;
  const s = Math.max(0, Math.round(Number(seconds)));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${pad(s % 60)}s`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ${pad(Math.floor((s % 3600) / 60))}m`;
  return `${Math.floor(s / 86400)}d ${pad(Math.floor((s % 86400) / 3600))}h`;
}

/** An ISO time relative to now: "3 s ago", "in 42 s", "5 min ago", "2 h ago". */
export function relative(iso) {
  if (!iso) return DASH;
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return DASH;
  const diff = Math.round((t - Date.now()) / 1000);
  const abs = Math.abs(diff);
  let text;
  if (abs < 60) text = `${abs} s`;
  else if (abs < 3600) text = `${Math.round(abs / 60)} min`;
  else if (abs < 86400) text = `${Math.round(abs / 3600)} h`;
  else text = `${Math.round(abs / 86400)} d`;
  if (abs < 1) return "now";
  return diff < 0 ? `${text} ago` : `in ${text}`;
}

/** 50 → "50.0%". */
export function percent(x) {
  if (x == null || !Number.isFinite(Number(x))) return DASH;
  return `${Number(x).toFixed(1)}%`;
}

/** An ISO time as local date and time. */
export function dateTime(iso) {
  if (!iso) return DASH;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return DASH;
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** Bytes per second: "1.2 MB/s". */
export function speed(bps) {
  if (bps == null || !Number.isFinite(Number(bps))) return DASH;
  return `${bytes(bps)}/s`;
}
