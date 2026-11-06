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

/** Header lines "Name: value" as an object; blank lines are skipped, a line with no colon is an error */
export function parseHeaders(text) {
  const out = {};
  for (const raw of String(text || "").split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const i = line.indexOf(":");
    if (i < 1) throw new Error(`"${line}" is not "Name: value"`);
    out[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  return out;
}

/** An object as header lines */
export const headerText = (headers) => Object.entries(headers || {}).map(([k, v]) => `${k}: ${v}`).join("\n");

const CALLBACK_PROBLEMS = {
  callback_base_url: "The service accepted the job, but this address does not answer from here.",
  url: "Cannot use the service address (or it answered 404/405).",
  headers: "The service rejected the headers (401/403).",
  timeout: "The service did not answer in time.",
  format: "The service answered, but not with a subtitle or 202.",
  server: "The service returned an error.",
};

/** The lines the Test button shows for a callback POST /settings/translation/test answer */
export function callbackLines(a) {
  const lines = [];
  if (a.result === "immediate") lines.push({ ok: true, text: `The service translated the sample straight away (${a.service.took_ms} ms).` });
  else if (a.result === "will_call_back") lines.push({ ok: true, text: `The service accepted the job and will call back (${a.service.took_ms} ms).` });
  else lines.push({ ok: false, text: `${CALLBACK_PROBLEMS[a.problem] || "The test failed."}${a.error ? ` (${a.error})` : ""}` });
  const r = a.base_url_reachable_from_here || {};
  if (r.checked) lines.push(r.status ? { ok: true, text: "The callback address answers from here. Check that it also answers from the service." } : { ok: false, text: `The callback address does not answer from here${r.error ? ` (${r.error})` : ""}.` });
  return lines;
}
