// General section (feature 046): appearance in this browser (feature 012) and which
// credentials are set.

import { get } from "../api.js";
import { h, toast } from "../app.js";
import { LANGUAGES, choice, nativeName, setLanguage } from "../i18n.js";

const PUTIO_AUTH = { token: "OAuth token set", app_password: "app password set", none: "not set" };
const MODE_NAMES = { system: "System", light: "Light", dark: "Dark" };

export const cards = (ctx) => [appearance(ctx), language(), credentials(ctx)];

/** Mode and accent radios; a change applies at once and is saved in this browser. */
function appearance(ctx) {
  const { MODES, ACCENTS } = window.appearance;
  const radio = (name, value, label, attrs = {}) =>
    h("label", attrs, h("input", { type: "radio", name, value, onchange: () => window.appearance.set({ [name]: value }) }), h("span", { text: label }));
  const form = h(
    "form",
    { class: "appearance-grid", onsubmit: (ev) => ev.preventDefault() },
    h("fieldset", { class: "choice" }, h("legend", { text: "Mode" }), h("div", { class: "seg-control" }, MODES.map((m) => radio("mode", m, MODE_NAMES[m])))),
    h("fieldset", { class: "choice swatches" }, h("legend", { text: "Accent" }), ACCENTS.map((a) => radio("accent", a, a[0].toUpperCase() + a.slice(1), { dataset: { accent: a } }))),
  );
  const sync = () => {
    const now = window.appearance.get();
    for (const input of form.querySelectorAll("input")) input.checked = now[input.name] === input.value;
  };
  sync();
  document.addEventListener("appearancechange", sync);
  ctx.cleanup.push(() => document.removeEventListener("appearancechange", sync));
  return h("section", { class: "card", id: "card-appearance" }, h("h2", { text: "Appearance" }), form, h("p", { class: "muted spaced", text: "Saved in this browser only." }));
}

/** The UI's language (feature 051): the browser's by default; a change applies at once */
function language() {
  const option = (code) => h("option", { value: code, translate: "no", lang: code, text: nativeName(code) });
  const select = h(
    "select",
    { id: "ui-language", onchange: async () => (await setLanguage(select.value)) || toast("Couldn't load that language", "error", "The page stays in English.") },
    h("option", { value: "", text: "Browser default" }),
    ["en", ...LANGUAGES].map(option),
  );
  select.value = choice();
  return h(
    "section",
    { class: "card", id: "card-language" },
    h("h2", { text: "Language" }),
    h("label", { class: "field" }, h("span", { text: "Language" }), select),
    h("p", { class: "muted spaced", text: "Saved in this browser only. Machine-translated; names, paths and messages from the daemon stay as they are." }),
  );
}

/** Which credentials are set; their values are never shown */
function credentials(ctx) {
  const list = h("dl", { class: "kv" }, h("dt", { text: "Put.io" }), h("dd", { class: "muted", text: "…" }));
  get("/status")
    .then((s) => {
      if (!ctx.alive()) return;
      const auth = (s && s.auth) || {};
      list.replaceChildren(
        h("dt", { text: "Put.io" }),
        h("dd", { text: PUTIO_AUTH[auth.putio] || "unknown" }),
        h("dt", { text: "API token" }),
        h("dd", { text: auth.api_token ? "set" : "not set" }),
      );
    })
    .catch(() => {
      if (ctx.alive()) list.lastChild.textContent = "unknown";
    });
  return h(
    "section",
    { class: "card", id: "card-credentials" },
    h("h2", { text: "Credentials" }),
    list,
    h("p", { class: "muted spaced", text: "Tokens and passwords are never shown here. Change them in the config file or environment." }),
  );
}
