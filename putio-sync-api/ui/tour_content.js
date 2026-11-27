// The words of the page tours and the settings help (feature 014). Data only: tour_ui.js
// shows a page's steps, pages/settings.js shows a card's help. tests/ui/tour.test.mjs checks
// that every page in the navigation has a tour, every settings card has help, and every
// guide named here is in guides.js.

/** Page → its steps. `target` is a CSS selector on that page; a step whose target is
 * missing is still shown, without a highlight. */
export const TOURS = {
  status: [
    { target: ".page-head", title: "Welcome to putio-sync", text: "This page shows what putio-sync is doing right now: its connection to Put.io, the last sync and what is downloading." },
    { target: "#nav", title: "Pages", text: "Each page shows one part of the work: files waiting in the queue, your Put.io files, the files already on this machine, and the history of what was downloaded." },
    { target: "#find", title: "Search everything", text: "Search finds pages, settings, queue items and files by name from anywhere. Type a few letters and press Enter." },
  ],
  active: [
    { target: ".page-head", title: "Active downloads", text: "Everything that is on its way: transfers still on Put.io and files downloading now, with their speed and the time left. The list updates by itself." },
    { target: ".card", title: "Follow one item", text: "Each item shows its stage and the steps it has passed. A step that failed can be retried or skipped here." },
  ],
  queue: [
    { target: ".page-head", title: "The queue", text: "Every file putio-sync found on Put.io and has not finished yet: waiting, downloading or failed." },
    { target: ".toolbar", title: "Act on the whole queue", text: "Pause all downloads, retry the files that failed, or ask for a sync now instead of waiting for the next check." },
    { target: ".card", title: "One file at a time", text: "Each row has its own buttons: retry a failed file, remove it, or move it up or down the queue." },
  ],
  putio: [
    { target: ".page-head", title: "Your Put.io files", text: "Browse the files in your Put.io account, folder by folder. This is what Put.io holds, not what is on this machine." },
    { target: ".toolbar", title: "Choose and act", text: "Tick files or folders, then download them now or delete them from Put.io. Deleting always asks first." },
  ],
  files: [
    { target: ".page-head", title: "Files on this machine", text: "The folders putio-sync downloads into. Use this page to check where a file landed." },
    { target: ".toolbar", title: "Tidy up", text: "Rename or delete downloaded files, check a file again, or fetch subtitles for a video. Deleting a local file does not delete it from Put.io." },
  ],
  transfers: [
    { target: ".page-head", title: "Put.io transfers", text: "Torrents and links that Put.io is fetching for you. When a transfer finishes, its files join the queue and are downloaded here." },
    { target: ".toolbar", title: "Add a transfer", text: "Paste a magnet link or a web address, or upload a .torrent file. Finished and failed transfers can be cleared from this page." },
  ],
  history: [
    { target: ".page-head", title: "History", text: "Everything putio-sync has downloaded or removed, and the releases it turned away with the reason for each." },
    { target: ".toolbar", title: "Find an old download", text: "Filter by name or by action to answer questions such as: did this file ever arrive, and when?" },
  ],
  stats: [
    { target: ".page-head", title: "Stats", text: "How much was downloaded over time and how fast, by folder, file type and source. Pick a range to look at a shorter or a longer time." },
    { target: ".card", title: "Read the charts", text: "Point at or tap a chart to see the numbers behind it. Filters narrow every chart on the page at once." },
  ],
  doctor: [
    { target: ".page-head", title: "Doctor", text: "Doctor checks the things that most often go wrong: the connection, your Put.io account, syncing and this machine." },
    { target: ".toolbar", title: "Run the checks", text: "Run it whenever something seems off. Each failed check says what is wrong and how to fix it." },
  ],
  logs: [
    { target: ".page-head", title: "Logs", text: "The messages putio-sync writes as it works. They are the detail behind the other pages." },
    { target: ".toolbar", title: "Narrow it down", text: "Filter by level, module or words to find one problem. Copy or download the lines you need when you ask for help." },
  ],
  settings: [
    { target: ".page-head", title: "Settings", text: "Every setting, saved to the config file. Most changes apply at once; a save tells you when one needs a restart." },
    { target: ".settings-nav", title: "Sections", text: "Settings are grouped into sections. A badge marks a section with unsaved changes or a problem." },
    { target: ".settings-search", title: "Search settings", text: "Type a few letters of a setting's name to jump straight to it." },
    { target: ".help-btn", title: "Help for each group", text: "The ? next to a heading explains that group of settings in plain words, with a link to the guide that covers it." },
  ],
};

/** Settings card id (settings/sections.js) → plain-language help and the guide to read */
export const HELP = {
  appearance: { text: "Choose light or dark colours and an accent colour. This changes only how the page looks in this browser; other people and other devices keep their own choice.", guide: "guides/index" },
  language: { text: "The language of this page. Names of files, paths and messages from the daemon stay as they are. The choice is kept in this browser.", guide: "guides/index" },
  profile: { text: "Tell putio-sync how you use it, for example with a media server or with Radarr and Sonarr. It then suggests settings that fit, and shows each change before it is saved.", guide: "guides/first-sync" },
  credentials: { text: "Shows which sign-in details are set, never their values. To change one, edit the config file or the environment, or link Put.io again from the Put.io section.", guide: "guides/first-sync" },
  updates: { text: "Asks GitHub once a day whether a newer version exists. Nothing about your install is sent. Some installs can also update themselves when no transfer is running.", guide: "guides/index" },
  tours: { text: "Each page shows a short tour the first time you open it in this browser. Use the button to see every tour again, for example after an update or on a shared computer.", guide: "guides/index" },
  notifications: { text: "Sends a plain-text email about the events you choose. You need the address and sign-in details of a mail server.", guide: "guides/index" },
  notifiers: { text: "Sends the same events to your phone or a chat app through a plugin. Each plugin's account and token are set on its tab in the Plugins section.", guide: "guides/index" },
  putio: { text: "Links putio-sync to your Put.io account. Linking shows a short code to enter on put.io, so your password is never typed here.", guide: "guides/first-sync" },
  storage: { text: "Warns when your Put.io account is nearly full, and can free space by deleting the oldest files there. It lists the files first and deletes nothing until you confirm.", guide: "guides/first-sync" },
  cleanup: { text: "Removes folders on Put.io that are left empty after their files were downloaded and deleted. It never removes a folder that still holds a file.", guide: "guides/first-sync" },
  watcher: { text: "Watches folders on this machine for .torrent files and sends each one to Put.io. Other programs can drop files there to start a download.", guide: "guides/watch-folders" },
  download: { text: "Where downloaded files are saved, how many download at once, and what happens when a file already exists. Start with the defaults and change one thing at a time.", guide: "guides/first-sync" },
  stream: { text: "Choose between saving files on this machine and leaving them on Put.io to play through your media server. Stream mode needs the media server turned on.", guide: "guides/media-server" },
  bandwidth: { text: "Limits how fast putio-sync downloads, all the time or at set hours, so it does not slow down everyone else on your network.", guide: "guides/first-sync" },
  verify: { text: "Checks each file after it arrives by comparing it with what Put.io says it should be. A file that does not match is downloaded again.", guide: "guides/troubleshooting" },
  subtitles: { text: "Saves subtitles next to each video, in the languages you choose, so your player or media server finds them without extra steps.", guide: "guides/media-server" },
  translation: { text: "Makes a missing subtitle language from a subtitle the video already has. It is off until you pick a translator, and it never writes over an existing subtitle file.", guide: "guides/media-server" },
  filters: { text: "Name patterns that decide which files are downloaded at all. Use them to skip samples, extras or file types you never want.", guide: "guides/folder-rules" },
  "folder-rules": { text: "Rules for one Put.io folder: where its files go on this machine and which files are skipped. Use them to send films and series to different places.", guide: "guides/folder-rules" },
  presets: { text: "Ready-made sets of rules for common cases, such as films or series. Pick a preset for a folder instead of writing its rules by hand.", guide: "guides/folder-rules" },
  blocked: { text: "Files that a filter or the release check stopped lately, and which rule stopped each. Look here when a file you expected did not download.", guide: "guides/folder-rules" },
  precheck: { text: "Looks at a release before it is sent to Put.io and turns away fakes. Each rejected release is listed on the History page with the reason.", guide: "guides/folder-rules" },
  arr: { text: "Lets Radarr and Sonarr use putio-sync as their download client. They send it what to fetch, and pick up each file when it has arrived.", guide: "guides/radarr-sonarr" },
  hooks: { text: "Runs a command or calls a web address when something happens, such as a finished download. Use it to tell another program that a file is ready.", guide: "guides/index" },
  pipeline: { text: "Your own steps that a download passes through before it counts as finished. A step that fails can be retried or skipped on the Active page.", guide: "guides/index" },
  plugins: { text: "Small add-ons that send notifications or tell Plex, Jellyfin or Emby to look for new files. Turn on only the ones you use.", guide: "guides/media-server" },
  "media-server": { text: "Shows your downloaded files to TVs and players on your home network, with no extra program. Choose the folders to share and the name players see.", guide: "guides/media-server" },
  network: { text: "Which addresses this page, the API and the metrics answer on. Add an address to reach putio-sync from other devices, and check it before you save.", guide: "guides/troubleshooting" },
  tailscale: { text: "With Tailscale on this machine, people on your tailnet can sign in without a token. You can also limit putio-sync to answer only on the tailnet.", guide: "guides/troubleshooting" },
  "sign-in": { text: "How people sign in to this page: with a username and password, or with a Google, GitHub or Discord account. Only the people you list are let in.", guide: "guides/troubleshooting" },
  logging: { text: "How much detail putio-sync writes to its log and how many lines the Logs page keeps. Raise the level only while you look for a problem.", guide: "guides/troubleshooting" },
  "usage-stats": { text: "Usage stats are off until you turn them on here. The README lists every field that would be sent, under What leaves your machine.", guide: "guides/index" },
  backup: { text: "Saves every setting as one file, and restores settings from such a file. Make a backup before a big change or a move to another machine.", guide: "guides/index" },
};
