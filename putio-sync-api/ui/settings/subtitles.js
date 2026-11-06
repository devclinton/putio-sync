// Subtitles card (feature 025): which languages are saved next to each video, which download
// categories are left alone, and which language names and codes are known. The transcription
// server settings are in the same group and shown under the languages, with a Test button.

import { post } from "../api.js";
import { h } from "../app.js";
import { TRANSCRIPTION_FIELDS, readTranscription, testLines } from "../transcription.js";
import { groupCard, quiet, reason } from "./form.js";

const SUBTITLES = {
  id: "subtitles",
  key: "subtitles",
  title: "Subtitles",
  fields: [
    ["languages", "Languages", "One per line, most wanted first; a name or a code (English, en, spa). Empty turns subtitles off", "l"],
    ["excluded_categories", "Skip these categories", "One per line: the first folder of a download, such as radarr. Nothing is fetched for them and their files are deleted from Put.io as before", "l"],
  ],
  extra: languagesExtra,
};

export const cards = (ctx) => [groupCard(SUBTITLES, ctx)];

/** The known languages, a reminder of how files are named, and the transcription section */
function languagesExtra(data) {
  const known = data.known_languages || [];
  const list = known.map((l) => `${l.name} (${l.code})`).join(", ");
  const t = data.transcription || {};
  const values = {};
  const inputs = {};
  const rows = TRANSCRIPTION_FIELDS.map(([name, label, hint, kind]) => {
    const id = `set-subtitles-transcription-${name}`;
    values[name] = t[name];
    let input;
    if (kind === "b") {
      input = h("input", { type: "checkbox", id, checked: Boolean(t[name]) });
      inputs[name] = () => input.checked;
      return h("label", { class: "field check", for: id }, input, h("span", {}, label, h("small", { text: hint })));
    }
    const num = kind === "n";
    input = h("input", { class: "input", id, type: num ? "number" : name === "api_key" ? "password" : "text", autocomplete: "off", spellcheck: "false" });
    input.value = t[name] == null ? "" : String(t[name]);
    inputs[name] = () => input.value;
    return h("label", { class: "field", for: id }, h("span", { text: label }), input, h("small", { text: hint }));
  });
  const read = () => {
    const now = Object.fromEntries(Object.entries(inputs).map(([k, f]) => [k, f()]));
    return { transcription: readTranscription(now) };
  };

  const out = h("div", { class: "spaced", role: "status", "aria-live": "polite" });
  const test = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    out.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const answer = await post("/settings/subtitles/test", read());
      out.replaceChildren(...testLines(answer).map((l) => h("p", { class: l.ok ? "ok-text" : "error-text", text: l.text })));
    } catch (e) {
      if (!quiet(e)) out.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    }
    if (button.isConnected) button.disabled = false;
  };

  const nodes = h(
    "div",
    { class: "spaced" },
    h("p", { class: "muted small", text: "Each wanted language is saved as Movie.en.srt next to Movie.mkv. A file that is already there is kept." }),
    known.length ? h("details", {}, h("summary", { text: `Known languages (${known.length})` }), h("p", { class: "muted small", text: list })) : null,
    h("h3", { text: "Transcription" }),
    h("p", { class: "muted small", text: "When Put.io has no subtitle for a wanted language, the audio can be sent to a speech to text server. Uses your own server; nothing is sent while this is off." }),
    ...rows,
    h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn", onclick: test }, "Test server")),
    out,
  );
  return { nodes, read };
}
