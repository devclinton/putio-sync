// The network save's confirm text (feature 037, hot reload part 3), from the answer of
// POST /settings/network/check. `applies_now` is true under `daemon`, where the listeners
// move as soon as the save applies; `server` binds once, so there it is after a restart.

/** What the confirm says about this page, from the check's answer */
export function checkText(c) {
  const urls = c.ui_at && c.ui_at.length ? c.ui_at.join(", ") : "";
  if (c.applies_now) {
    const at = urls ? ` Once saved, the UI is at ${urls}.` : "";
    if (c.this_page === "cut_off") return `This page came in on ${c.listened_on}, which the new list doesn't listen on: once saved it won't reach putio-sync.${at}`;
    if (c.this_page === "unknown") return `putio-sync can't tell whether this page will still reach it once saved.${at}`;
    return at.trim();
  }
  const at = urls ? ` After a restart the UI is at ${urls}.` : "";
  if (c.this_page === "cut_off") return `This page came in on ${c.listened_on}, which the new list doesn't listen on: after a restart it won't reach putio-sync.${at}`;
  if (c.this_page === "unknown") return `putio-sync can't tell whether this page will still reach it after a restart.${at}`;
  return at.trim();
}
