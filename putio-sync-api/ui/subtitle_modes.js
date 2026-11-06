// Pure helpers for the Subtitles card's source modes and OpenSubtitles fields (feature 050).

/** [value, label] for the global mode; the empty value is the 025 behaviour */
export const MODES = [
  ["", "Download, then transcribe if missing (default)"],
  ["download_only", "Download only"],
  ["whisper_only", "Transcribe only"],
  ["both", "Download and transcribe"],
];

/**
 * "radarr = download_only" lines as an object. A line with a wrong or missing mode is kept as
 * typed, so the server refuses the save and names the mode it did not know
 */
export function parseModes(text) {
  const out = {};
  for (const line of String(text).split("\n")) {
    const [key, mode] = line.trim().split(/\s*[=:]\s*|\s+/, 2);
    if (key) out[key] = (mode || "").toLowerCase();
  }
  return out;
}

/** The lines parseModes reads, from a saved object */
export const modesText = (obj) => Object.entries(obj || {}).map(([k, m]) => `${k} = ${m}`).join("\n");

/** The mode fields of the saved/edited group; an empty global mode is left out so a 025 file stays as it was */
export function readModes({ mode, categories, languages }) {
  return { mode: mode || undefined, category_modes: parseModes(categories), language_modes: parseModes(languages) };
}

/** [name, label, hint, secret] for the OpenSubtitles fields */
export const OS_FIELDS = [
  ["api_key", "OpenSubtitles API key", "A free consumer key from opensubtitles.com. Empty turns this source off", true],
  ["username", "Account name (optional)", "A login raises the daily download limit", false],
  ["password", "Account password (optional)", "Needs the account name", true],
];

/** The OpenSubtitles body: the typed fields, the unchanged User-Agent, and the threshold when one is typed */
export function readOpenSubtitles(v, stored) {
  const min = String(v.min_confidence ?? "").trim();
  return {
    opensubtitles: { api_key: String(v.api_key).trim(), username: String(v.username).trim(), password: String(v.password), user_agent: (stored || {}).user_agent || "" },
    min_confidence: min === "" ? undefined : Number(min),
  };
}
