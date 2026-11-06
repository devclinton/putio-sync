// Pure helpers for the Translation card and its Test button (feature 026).

/** The chat fields: [name, label, hint, kind] in the card's field notation */
export const CHAT_FIELDS = [
  ["base_url", "Server URL", "An OpenAI-compatible server, such as http://ollama.lan:11434/v1 or http://pc:1234", "s"],
  ["model", "Model", "The model name the server knows", "s"],
  ["api_key", "API key", "Sent as a Bearer token when set; leave ******** to keep the saved key", "s?"],
];

const PROBLEMS = {
  url: "Cannot use this address. Check the server URL (the path /v1 is added when missing).",
  key: "The server rejected the API key.",
  model: "The server does not know this model.",
  timeout: "The server did not answer in time.",
  format: "The server answered, but not in the numbered line format the translator needs. Try a larger or instruction-following model.",
  server: "The server returned an error.",
};

/** The chat object a value map reads as: optional text as text */
export function readChat(values) {
  return Object.fromEntries(CHAT_FIELDS.map(([name]) => [name, String(values[name] ?? "").trim()]));
}

/** The lines the Test button shows for a chat POST /settings/translation/test answer */
export function chatLines(a) {
  if (a.ok) {
    const first = (a.sample || [])[0];
    const shown = first ? `: "${first.source}" became "${first.translation}"` : "";
    return [{ ok: true, text: `Translated ${(a.sample || []).length} sample lines in ${a.took_ms} ms${shown}.` }];
  }
  const lines = [{ ok: false, text: `${PROBLEMS[a.problem] || "The test failed."}${a.error ? ` (${a.error})` : ""}` }];
  if (a.models && a.models.length) lines.push({ ok: false, text: `Models on this server: ${a.models.join(", ")}.` });
  for (const s of a.problem === "format" ? a.sample || [] : []) lines.push({ ok: false, text: `Reply line ${s.number}: ${s.translation}` });
  return lines;
}
