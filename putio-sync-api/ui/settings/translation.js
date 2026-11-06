// Translation card (feature 026): how a wanted subtitle language that nothing provides is
// translated, by a chat model or by your own API, with a Test button for the chat server.

import { post } from "../api.js";
import { h } from "../app.js";
import { CHAT_FIELDS, callbackLines, chatLines, headerText, parseHeaders, readChat } from "../translation.js";
import { groupCard, quiet, reason } from "./form.js";

const TRANSLATION = {
  id: "translation",
  key: "translation",
  title: "Translation",
  fields: [
    ["kind", "Translator", null, [
      ["none", "Off", "Nothing is translated"],
      ["chat", "Chat model", "An OpenAI-compatible server such as Ollama or LM Studio"],
      ["callback", "Your own service", "Your API gets the subtitle and answers now or calls back"],
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
  const cb = data.callback || {};
  const text = (id, value, type = "text") => {
    const input = h("input", { class: "input", id, type, autocomplete: "off", spellcheck: "false" });
    input.value = value == null ? "" : String(value);
    return input;
  };
  const cbUrl = text("set-translation-cb-url", cb.url);
  const cbBase = text("set-translation-cb-base", cb.callback_base_url);
  const cbWait = text("set-translation-cb-wait", cb.timeout_secs, "number");
  const cbHeaders = h("textarea", { class: "input", id: "set-translation-cb-headers", rows: "3", autocomplete: "off", spellcheck: "false" });
  cbHeaders.value = headerText(cb.headers);
  const useHere = () => { cbBase.value = location.origin; };
  const field = (id, label, input, hint) => h("label", { class: "field", for: id }, h("span", { text: label }), input, h("small", { text: hint }));
  const readCb = () => ({
    url: cbUrl.value.trim(),
    callback_base_url: cbBase.value.trim(),
    timeout_secs: Number(cbWait.value) || 0,
    headers: parseHeaders(cbHeaders.value),
  });
  const read = () => ({ chat: readChat(Object.fromEntries(Object.entries(inputs).map(([k, f]) => [k, f()]))), callback: readCb() });

  const out = h("div", { class: "spaced", role: "status", "aria-live": "polite" });
  const tester = (kind, lines) => async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    out.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const answer = await post("/settings/translation/test", { translation: { kind, ...read() } });
      out.replaceChildren(...lines(answer).map((l) => h("p", { class: l.ok ? "ok-text" : "error-text", text: l.text })));
    } catch (e) {
      if (!quiet(e)) out.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    }
    if (button.isConnected) button.disabled = false;
  };

  const notes = [];
  if (data.resolved_endpoint) notes.push(h("p", { class: "muted small", text: `Requests go to ${data.resolved_endpoint}` }));
  if (data.callback_url_form) notes.push(h("p", { class: "muted small", text: `The service is told to POST its result to ${data.callback_url_form}` }));
  for (const w of data.warnings || []) notes.push(h("p", { class: "muted small", text: w }));
  const nodes = h(
    "div",
    { class: "spaced" },
    h("p", { class: "muted small", text: "Translates into each wanted language (Subtitles) that no subtitle covers, from a subtitle you already have. Uses your own server; nothing is sent while the translator is off." }),
    h("h3", { text: "Chat model" }),
    ...rows,
    h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn", onclick: tester("chat", chatLines) }, "Test server")),
    h("h3", { text: "Your own service" }),
    field("set-translation-cb-url", "Service URL", cbUrl, "Gets a POST with the subtitle (JSON). Answer 200 with the translated subtitle, or 202 and POST it to the callback address."),
    field("set-translation-cb-headers", "Headers", cbHeaders, "One per line as Name: value, for example an API key; values are kept secret"),
    field("set-translation-cb-base", "This app's address", cbBase, "How the service reaches putio-sync, such as http://nas.lan:8088"),
    h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn", onclick: useHere }, "Use this address")),
    field("set-translation-cb-wait", "Wait for the callback (s)", cbWait, "60 to 86400"),
    h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn", onclick: tester("callback", callbackLines) }, "Test service")),
    out,
    ...notes,
  );
  return { nodes, read };
}
