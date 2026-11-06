// Translation card (feature 026): how a wanted subtitle language that nothing provides is
// translated, by a chat model or by your own API, with a Test button for the chat server.

import { post } from "../api.js";
import { h } from "../app.js";
import { CHAT_FIELDS, chatLines, readChat } from "../translation.js";
import { groupCard, quiet, reason } from "./form.js";

const TRANSLATION = {
  id: "translation",
  key: "translation",
  title: "Translation",
  fields: [
    ["kind", "Translator", null, [
      ["none", "Off", "Nothing is translated"],
      ["chat", "Chat model", "An OpenAI-compatible server such as Ollama or LM Studio"],
    ]],
    ["excluded_categories", "Skip these categories", "One per line: the first folder of a download, such as radarr", "l"],
    ["max_in_flight", "Requests at once", "1 to 8, across all translations", "n", 1, 8],
    ["timeout_secs", "Time limit per request (s)", "10 to 1800", "n", 10, 1800],
    ["chunk_cues", "Subtitle lines per request", "5 to 200", "n", 5, 200],
    ["instruction", "Extra instruction", "Added to the prompt, for example a tone or a glossary (500 characters at most)", "s?"],
  ],
  advanced: ["max_in_flight", "timeout_secs", "chunk_cues"],
  extra: translationExtra,
};

export const cards = (ctx) => [groupCard(TRANSLATION, ctx)];

function translationExtra(data) {
  const chat = data.chat || {};
  const inputs = {};
  const rows = CHAT_FIELDS.map(([name, label, hint]) => {
    const id = `set-translation-chat-${name}`;
    const input = h("input", { class: "input", id, type: name === "api_key" ? "password" : "text", autocomplete: "off", spellcheck: "false" });
    input.value = chat[name] == null ? "" : String(chat[name]);
    inputs[name] = () => input.value;
    return h("label", { class: "field", for: id }, h("span", { text: label }), input, h("small", { text: hint }));
  });
  const read = () => ({ chat: readChat(Object.fromEntries(Object.entries(inputs).map(([k, f]) => [k, f()]))) });

  const out = h("div", { class: "spaced", role: "status", "aria-live": "polite" });
  const test = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    out.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const answer = await post("/settings/translation/test", { translation: { kind: "chat", ...read() } });
      out.replaceChildren(...chatLines(answer).map((l) => h("p", { class: l.ok ? "ok-text" : "error-text", text: l.text })));
    } catch (e) {
      if (!quiet(e)) out.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    }
    if (button.isConnected) button.disabled = false;
  };

  const notes = [];
  if (data.resolved_endpoint) notes.push(h("p", { class: "muted small", text: `Requests go to ${data.resolved_endpoint}` }));
  for (const w of data.warnings || []) notes.push(h("p", { class: "muted small", text: w }));
  const nodes = h(
    "div",
    { class: "spaced" },
    h("p", { class: "muted small", text: "Translates into each wanted language (Subtitles) that no subtitle covers, from a subtitle you already have. Uses your own server; nothing is sent while the translator is off." }),
    h("h3", { text: "Chat model" }),
    ...rows,
    h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn", onclick: test }, "Test server")),
    out,
    ...notes,
  );
  return { nodes, read };
}
