// The docs book's guides (feature 016), for the command palette and the setup link.
// tests/ui/guides.test.mjs keeps it in step with the book. Rows: name, title (the
// chapter's heading), kind, extra English search words.

export const KIND_LABELS = { index: "Guides", setup: "Setup guide", task: "Guide", switch: "Switching guide" };

export const GUIDES = [
  ["index", "Guides", "index", "start help"],
  ["unraid", "Set up on Unraid", "setup", "install"],
  ["truenas", "Set up on TrueNAS SCALE", "setup", "install"],
  ["raspberry-pi", "Set up on a Raspberry Pi", "setup", "install pi"],
  ["docker", "Set up with Docker and Compose", "setup", "install casaos umbrel"],
  ["macos", "Set up on macOS", "setup", "install mac brew"],
  ["windows", "Set up on Windows", "setup", "install winget"],
  ["linux", "Set up on Linux", "setup", "install snap deb"],
  ["first-sync", "Your first sync", "setup", "wizard"],
  ["radarr-sonarr", "Connect Radarr and Sonarr", "task", "arr prowlarr"],
  ["autobrr", "Connect autobrr", "task", "qbittorrent irc announce"],
  ["cross-seed", "Connect cross-seed", "task", "qbittorrent"],
  ["qbittorrent-apps", "Use a qBittorrent phone app", "task", "nzb360 remote android ios"],
  ["folder-rules", "Choose what downloads", "task", "filters presets"],
  ["watch-folders", "Send torrents from a watch folder", "task", "watcher magnet"],
  ["media-server", "Play downloads on your TV", "task", "dlna upnp"],
  ["metrics-grafana", "Watch it with Prometheus and Grafana", "task", "metrics"],
  ["troubleshooting", "When something isn't working", "task", "doctor logs error"],
  ["from-qbittorrent", "Switch from qBittorrent", "switch", "migrate"],
  ["from-transmission", "Switch from Transmission", "switch", "migrate"],
  ["from-deluge", "Switch from Deluge", "switch", "migrate"],
  ["from-usenet", "Use Put.io next to SABnzbd or NZBGet", "switch", "usenet nzb"],
].map(([name, title, kind, words]) => ({ slug: `guides/${name}`, title, kind, words }));

const SETUP = { unraid: "unraid", truenas: "truenas", container: "docker", casaos: "docker", umbrel: "docker", snap: "linux", deb: "linux", flatpak: "linux", homebrew: "macos", pkg: "macos", winget: "windows", setup: "windows" };

/** The setup guide for an install channel kind; the index for any other */
export const guideFor = (kind) => `guides/${Object.hasOwn(SETUP, kind) ? SETUP[kind] : "index"}`;
