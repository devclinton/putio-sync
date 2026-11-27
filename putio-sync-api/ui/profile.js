// Usage profile (feature 056): the words for the setup wizard's "How will you use
// putio-sync?" screen and its review, shared with Settings › Usage profile. The daemon plans
// the settings and gives reason codes; this file turns them into text. No DOM, so it is
// tested with node.

/** The kinds of download, in the order the screen lists them */
export const KINDS = [
  ["movies", "Movies"],
  ["tv", "TV shows"],
  ["music", "Music"],
  ["books", "Books and audiobooks"],
  ["software", "Software and games"],
  ["other", "Other files"],
];

/** The *arr apps that can send downloads (Radarr for movies, Sonarr for TV) */
export const ARR = [
  ["radarr", "Radarr"],
  ["sonarr", "Sonarr"],
];

/** The setup code in a link's fragment (`#code=…`), or null */
export function codeFromHash(hash) {
  const m = /^#(?:.*[&?])?code=([^&]+)/.exec(hash || "");
  if (!m) return null;
  try {
    const code = decodeURIComponent(m[1]).trim();
    return code || null;
  } catch {
    return null;
  }
}

/** The kinds to tick first: those whose folders are in the Put.io account */
export function startKinds(found) {
  const have = Object.keys(found || {}).filter((k) => (found[k] || []).length);
  return KINDS.map(([k]) => k).filter((k) => have.includes(k));
}

/** The answers to show first: the saved ones, else what Put.io's folders suggest */
export function startAnswers(saved, found, watchDir) {
  if (saved && saved.kinds && saved.kinds.length) return { ...saved, watch: saved.watch || null };
  return { kinds: startKinds(found), arr: [], by_hand: true, watch: null, language: "", suggestedWatch: watchDir || "" };
}

/** The review line's title */
export function title(rec) {
  const folder = (rec.params && rec.params.folder) || "";
  if (rec.id.startsWith("rule:")) return `Folder ${folder}`;
  return (
    {
      watch: "Watch folder",
      precheck_watch: "Release check for the watch folder",
      delete: "Files on Put.io after download",
      subtitles: "Subtitles",
    }[rec.id] || rec.id
  );
}

/** Why the daemon recommends it, in words */
export function reason(rec) {
  const folder = (rec.params && rec.params.folder) || "";
  switch (rec.reason) {
    case "found":
      return `A folder named ${folder} is in your Put.io account.`;
    case "arr_category":
      return rec.kind === "tv" ? `Sonarr sends its downloads to ${folder}.` : `Radarr sends its downloads to ${folder}.`;
    case "new_folder":
      return `Put files you add by hand in ${folder}; it is created when needed.`;
    case "detected":
      return `Torrent and magnet files dropped in ${folder} are sent to Put.io.`;
    case "smallest_size":
      return "Releases smaller than this are usually fakes, so they are rejected before upload.";
    case "software_off":
      return "Off, because software and games can be any size.";
    case "arr_imports":
      return "Radarr and Sonarr move files into your library, so the copy on Put.io isn't needed.";
    case "library":
      return "Files stay on Put.io after they are downloaded.";
    case "video":
      return "Subtitles in your language, and English, are saved next to videos.";
    default:
      return "";
  }
}

/** A note under the review */
export function note(code) {
  return (
    {
      dlna: "Play downloads on a TV or player on your home network: turn on Settings › Media server › Media server.",
      tailscale: "Tailscale is running on this machine: you can sign in with it in Settings › Network and access › Tailscale.",
    }[code] || ""
  );
}

/** A setting key from the plan (`download.rules[Movies].preset`), in words */
export function keyLabel(key) {
  let m = /^download\.rules\[(.*)\]\.preset$/.exec(key);
  if (m) return `Folder rule for ${m[1]}`;
  m = /^precheck\.watch\[(.*)\]\.(min_size_mb|enabled)$/.exec(key);
  if (m) return m[2] === "enabled" ? `Release check for ${m[1]}` : `Smallest release for ${m[1]} (MB)`;
  return (
    {
      "download.delete_after_download": "Delete after download",
      "cleanup.enabled": "Put.io cleanup",
      "subtitles.languages": "Subtitle languages",
      "subtitles.mode": "Subtitle mode",
      "subtitles.excluded_categories": "Folders without subtitles",
      "watch.enabled": "Torrent watcher",
      "watch.paths": "Watched folders",
    }[key] || key
  );
}

/** A setting value, in words; `null` is the default */
export function valueText(v) {
  if (v === null || v === undefined) return "default";
  if (v === true) return "on";
  if (v === false) return "off";
  if (Array.isArray(v)) return v.length ? v.join(", ") : "none";
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

/** Subtitle languages typed as `de, en` */
export const languagesFrom = (text) =>
  String(text || "")
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean);

/** The answers from what the screen holds, for POST /setup/profile */
export function answersFrom({ kinds, arr, byHand, watchOn, watch, language }) {
  return {
    kinds: KINDS.map(([k]) => k).filter((k) => kinds.includes(k)),
    arr: ARR.map(([a]) => a).filter((a) => arr.includes(a)),
    by_hand: Boolean(byHand),
    watch: watchOn && String(watch || "").trim() ? String(watch).trim() : null,
    language: language || "",
  };
}
