// Pure helpers for the Subtitles card's transcription section and Test button (feature 025).

/** The lines the Test button shows for a POST /settings/subtitles/test answer */
export function testLines(answer) {
  const lines = [];
  const s = answer.server || {};
  if (answer.ok) {
    const heard = s.text ? `heard "${s.text.length > 60 ? `${s.text.slice(0, 57)}…` : s.text}"` : "heard no speech in the test tone, which is expected";
    const lang = s.language ? `, language ${s.language}` : "";
    lines.push({ ok: true, text: `Server answered in ${s.took_ms} ms${lang}; ${heard}.` });
  } else {
    const code = s.status ? `HTTP ${s.status}: ` : "";
    lines.push({ ok: false, text: `Server test failed: ${s.error && s.error.startsWith("HTTP") ? s.error : code + (s.error || "no answer")}` });
  }
  const f = answer.ffmpeg || {};
  lines.push(f.found ? { ok: true, text: `ffmpeg found at ${f.path}${f.version ? ` (version ${f.version})` : ""}.` } : { ok: false, text: "ffmpeg not found: transcription will fail until it is installed or its path is set." });
  return lines;
}

/** The transcription fields: [name, label, hint, kind] in the card's field notation */
export const TRANSCRIPTION_FIELDS = [
  ["enabled", "Transcribe when nothing is found", "Make subtitles from the audio for a wanted language that no subtitle file covers", "b"],
  ["url", "Server URL", "An OpenAI-style speech to text server, such as http://nas:8000", "s"],
  ["api_key", "API key", "Sent as a Bearer token; leave ******** to keep the saved key", "s?"],
  ["model", "Model", "Sent as the model name when set", "s?"],
  ["language", "Language hint", "Leave empty to let the server detect it", "s?"],
  ["timeout_secs", "Time limit per request (s)", "10 to 7200", "n"],
  ["chunk_secs", "Audio part length (s)", "60 to 1800; longer videos are sent in parts", "n"],
  ["ffmpeg_path", "ffmpeg path", "Empty searches PATH", "s?"],
];

/** The transcription object a field map reads as: numbers as numbers, optional text as text */
export function readTranscription(values) {
  const out = {};
  for (const [name, , , kind] of TRANSCRIPTION_FIELDS) {
    const v = values[name];
    if (kind === "b") out[name] = Boolean(v);
    else if (kind === "n") out[name] = Number(v);
    else out[name] = String(v ?? "").trim();
  }
  return out;
}
