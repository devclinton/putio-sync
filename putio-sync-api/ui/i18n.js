// Languages (feature 051). The UI's code keeps its English text, which is the key into a
// catalog: /assets/i18n/<code>.json maps each English string to its translation, with {0},
// {1}... where a template put a value. translate.js and the catalog load only for another
// language, so English costs this file alone. The choice is saved in this browser only.

const KEY = "putio-sync.language";
export const LANGUAGES = ["ar", "da", "de", "es", "fi", "fr", "he", "hi", "id", "it", "ja", "ko", "ms", "nb", "nl", "pl", "pt", "pt-PT", "ru", "sv", "th", "tr", "uk", "vi", "zh", "zh-TW"];
const RTL = new Set(["ar", "he"]);

let lang = "en";
let translator = null;

/** The catalog language for a browser tag such as "pt-BR" or "zh-Hant-TW", or null */
export function match(tag) {
  const t = String(tag || "").toLowerCase();
  if (/^zh-(tw|hk|mo|hant)/.test(t)) return "zh-TW";
  const exactCode = LANGUAGES.find((c) => c.toLowerCase() === t);
  if (exactCode) return exactCode;
  const base = { iw: "he", no: "nb", nn: "nb", in: "id" }[t.split("-")[0]] || t.split("-")[0];
  return base === "en" || LANGUAGES.includes(base) ? base : null;
}

/** The saved choice: a language code, or "" for the browser's */
export function choice() {
  try {
    const saved = window.localStorage.getItem(KEY) || "";
    return saved === "en" || LANGUAGES.includes(saved) ? saved : "";
  } catch {
    return "";
  }
}

/** The language in use */
export const current = () => lang;

/** `text` in the language in use */
export const tr = (text) => (translator ? translator.tr(text) : text);

/** `code`'s name in its own language, such as "Deutsch" */
export function nativeName(code) {
  try {
    const name = new Intl.DisplayNames([code], { type: "language" }).of(code);
    return name[0].toLocaleUpperCase(code) + name.slice(1);
  } catch {
    return code;
  }
}

/** Show the UI in `code` ("" follows the browser) and save it. Resolves false when the catalog can't load. */
export async function setLanguage(code, save = true) {
  if (save) {
    try {
      if (code) window.localStorage.setItem(KEY, code);
      else window.localStorage.removeItem(KEY);
    } catch {
      // not remembered, still applied
    }
  }
  const want = code || (navigator.languages || [navigator.language]).map(match).find(Boolean) || "en";
  let catalog = {};
  let ok = true;
  if (want !== "en") {
    try {
      const res = await fetch(`/assets/i18n/${want}.json`);
      if (!res.ok) throw new Error(res.status);
      catalog = await res.json();
    } catch {
      ok = false;
    }
  }
  lang = ok ? want : "en";
  // English with nothing translated yet needs no translator
  if (lang !== "en" || translator) {
    try {
      translator ||= await import("./translate.js");
      translator.apply(catalog);
    } catch {
      lang = "en";
      ok = false;
    }
  }
  const root = document.documentElement;
  root.lang = lang;
  root.dir = RTL.has(lang) ? "rtl" : "ltr";
  document.dispatchEvent(new CustomEvent("languagechange", { detail: lang }));
  return ok;
}
