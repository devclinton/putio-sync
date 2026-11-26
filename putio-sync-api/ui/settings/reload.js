// Hot reload status for the Settings page: a hand edit of the config file that was refused,
// and saved settings that wait for a restart. GET /settings/status is the source; the
// server's list of restart-only settings is putio_sync_core::restart::RESTART_ONLY.

/** What the banner shows for a status, or null when there is nothing to say */
export function noticeParts(status) {
  if (!status || typeof status !== "object") return null;
  const error = typeof status.last_error === "string" && status.last_error.trim() ? status.last_error.trim() : null;
  const pending = Array.isArray(status.pending_restart) ? status.pending_restart.filter((k) => typeof k === "string" && k) : [];
  return error || pending.length ? { error, pending } : null;
}

/** How often the page looks again, so a hand edit shows without a reload */
export const POLL_MS = 15000;
