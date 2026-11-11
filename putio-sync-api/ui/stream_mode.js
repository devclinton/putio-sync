// Stream mode (feature 022): whether the download settings keep files on Put.io, and
// whether the media server can show them. Pure, so tests/ui can check it without a page.

/** Stream mode is on for the account or for any folder rule */
export function streamUsed(download) {
  if (!download) return false;
  return download.mode === "stream" || (download.rules || []).some((r) => r.mode === "stream");
}

/** What keeps streamed files off the TV: "off", "no_putio", or null when nothing does */
export function mediaServerGap(dlna) {
  if (!dlna || !dlna.enabled) return "off";
  if (!(dlna.roots || []).includes("putio")) return "no_putio";
  return null;
}

/** The warning for a gap */
export function gapText(gap) {
  if (gap === "off") return "Stream mode keeps files on Put.io, but the media server is off, so no player on your network can see them.";
  if (gap === "no_putio") return "Stream mode keeps files on Put.io, but the media server doesn't share Put.io, so no player on your network can see them.";
  return "";
}

/** The media server settings with the server on and Put.io shared, for one PUT */
export function fixMediaServer(dlna) {
  const { available_roots: _, ...settings } = dlna || {};
  const roots = [...(settings.roots || [])];
  if (!roots.includes("putio")) roots.push("putio");
  return { ...settings, enabled: true, roots };
}
