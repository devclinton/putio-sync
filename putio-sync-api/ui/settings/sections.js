// Settings sections (feature 046): which card is in which section, the help for each, the
// `#/settings/<section>?field=<address>` addresses, and links from "Settings › Section ›
// Card" in text. No DOM, so it is tested with node.

export const SECTIONS = [
  {
    id: "general",
    title: "General",
    help: "How this page looks in this browser, which credentials are set, and how finished downloads are announced.",
    cards: [["appearance", "Appearance"], ["credentials", "Credentials"], ["notifications", "Notifications"]],
  },
  {
    id: "putio",
    title: "Put.io",
    help: "Your Put.io account: linking with a code, keeping space free, removing empty folders, and uploading .torrent files from watched folders.",
    cards: [["putio", "Put.io"], ["storage", "Put.io storage"], ["cleanup", "Put.io cleanup"], ["watcher", "Torrent watcher"]],
  },
  {
    id: "downloads",
    title: "Downloads",
    help: "Where files go and how they are downloaded: how many at once, how fast and when, and how each file is checked once it arrives.",
    cards: [["download", "Downloads"], ["bandwidth", "Bandwidth"], ["verify", "Verification"]],
  },
  {
    id: "rules",
    title: "Rules and filters",
    help: "Which files are downloaded at all: name patterns for every folder, rules for one folder, and the release check that rejects fakes before they reach the NAS.",
    cards: [["filters", "File filters"], ["folder-rules", "Folder rules"], ["presets", "Presets"], ["blocked", "Recently blocked"], ["precheck", "Release check"]],
  },
  {
    id: "integrations",
    title: "Integrations",
    help: "Other programs putio-sync works with: Radarr and Sonarr, and hooks that run a command or call a webhook on each event or as a step after a download.",
    cards: [["arr", "Radarr / Sonarr"], ["hooks", "Event hooks"], ["pipeline", "Processing pipeline"]],
  },
  {
    id: "plugins",
    title: "Plugins",
    help: "Lua plugins that send notifications or tell Plex, Jellyfin or Emby to refresh. Each plugin that is on has a tab; turn others on from the list.",
    cards: [["plugins", "Plugins"]],
  },
  {
    id: "network",
    title: "Network and access",
    help: "Which addresses the web UI, API and metrics listen on, and signing in with Tailscale.",
    cards: [["network", "Network"], ["tailscale", "Tailscale"]],
  },
  {
    id: "advanced",
    title: "Advanced",
    help: "Logging and how many lines the Logs page keeps, and saving or restoring every setting as one file.",
    cards: [["logging", "Logging"], ["backup", "Backup and restore"]],
  },
];

/** The section with this id; General for anything else */
export const sectionOf = (id) => SECTIONS.find((s) => s.id === id) || SECTIONS[0];

export const settingsHref = (section, field) => `#/settings/${section}${field ? `?field=${field}` : ""}`;

// Config section → UI group, where they differ; keys edited in a card rather than a field
const GROUP = { watch: "watcher" };
const ALIAS = { "download.rules": "card-folder-rules", hooks: "card-hooks", plugins: "card-plugins", arr: "card-arr" };

/** `group.name` → the input's id `set-group-name`; anything else is an element id already */
export function fieldId(address) {
  if (ALIAS[address]) return ALIAS[address];
  const dot = address.indexOf(".");
  if (dot < 0) return address;
  const group = address.slice(0, dot);
  return `set-${GROUP[group] || group}-${address.slice(dot + 1)}`;
}

const RULE_FIELDS = ["whitelist_patterns", "blacklist_patterns", "blocked_extensions", "rules"];
const KEY_SECTION = {
  download: "downloads",
  watch: "putio",
  watcher: "putio",
  cleanup: "putio",
  storage: "putio",
  putio: "putio",
  hooks: "integrations",
  pipeline: "integrations",
  plugins: "plugins",
  arr: "integrations",
  network: "network",
  tailscale: "network",
  api: "network",
  logging: "advanced",
  notifications: "general",
  precheck: "rules",
};

/** The section a dotted config key (`download.verify`, `hooks`) is edited in */
export function sectionOfKey(key) {
  const [group, name] = key.split(".");
  if (group === "download" && RULE_FIELDS.includes(name)) return "rules";
  return KEY_SECTION[group] || "general";
}

// "Settings › Section › Card" and "Settings › Section", longest first so a card wins
const NAMES = [
  ...SECTIONS.flatMap((s) => [
    ...s.cards.map(([id, title]) => [`Settings › ${s.title} › ${title}`, settingsHref(s.id, `card-${id}`)]),
    [`Settings › ${s.title}`, settingsHref(s.id)],
  ]),
  ["Settings › Integrations › Plugins", settingsHref("plugins", "card-plugins")],
].sort((a, b) => b[0].length - a[0].length);
const NAME_RE = new RegExp(NAMES.map(([n]) => n.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")).join("|"), "g");

/** Text → strings and `{text, href}` for each Settings name in it */
export function linkParts(text) {
  const out = [];
  let at = 0;
  for (const m of text.matchAll(NAME_RE)) {
    if (m.index > at) out.push(text.slice(at, m.index));
    out.push({ text: m[0], href: NAMES.find(([n]) => n === m[0])[1] });
    at = m.index + m[0].length;
  }
  if (at < text.length || !out.length) out.push(text.slice(at));
  return out;
}
