// Pure logic for "Fetch subtitles" on the Local files page (feature 025, US3).

const VIDEO_EXTS = ["mkv", "mp4", "m4v", "avi", "mov", "wmv", "ts", "m2ts", "webm", "mpg", "mpeg"];

/** Whether a name looks like a video the page can fetch subtitles for */
export const isVideoName = (name) => VIDEO_EXTS.includes(String(name).split(".").pop().toLowerCase()) && String(name).includes(".");

/** Whether a listing entry gets the action at all, and why not when it does not */
export function fetchState(entry) {
  if (entry.kind !== "file") return { show: false };
  if (!isVideoName(entry.name)) return { show: false };
  if (!entry.valid_name) return { show: true, disabled: true, why: "The name is not valid on this system" };
  if (entry.partial || entry.in_use) return { show: true, disabled: true, why: "A download is writing here" };
  return { show: true, disabled: false, why: "" };
}

/**
 * What to do with a preflight answer: `{go: false, message}` to stop, or
 * `{go: true, confirm: null | {title, text, verb}}`. Nothing is asked when Put.io can supply
 * subtitles and none are saved yet; a confirmation comes before transcription (it uses the
 * CPU of another machine for minutes) and when files already exist (they are kept).
 */
export function fetchPlan(name, pre) {
  if (!pre.eligible) return { go: false, message: pre.reason || "Subtitles cannot be fetched for this file" };
  const parts = [];
  const only = pre.mode === "whisper_only";
  const transcribes = only || pre.mode === "both" || (pre.mode !== "download_only" && pre.putio !== "available");
  if (only || (transcribes && pre.putio === "available")) {
    parts.push("The audio will be sent to your transcription server. That can take several minutes.");
  } else if (transcribes) {
    parts.push(pre.putio === "gone" ? "Put.io no longer has this file," : "Put.io cannot be asked about this file,");
    parts.push("so the audio will be sent to your transcription server. That can take several minutes.");
  }
  const have = (pre.existing || []).map((e) => e.name);
  if (have.length) parts.push(`Already saved and kept: ${have.join(", ")}.`);
  if (!parts.length) return { go: true, confirm: null };
  const langs = (pre.languages || []).join(", ");
  return { go: true, confirm: { title: `Fetch subtitles for ${name}`, text: `${parts.join(" ")} Languages: ${langs}.`, verb: "Fetch" } };
}

/** What the Translate action does for a preflight answer: {go, targets, confirm} or {go: false, message}. */
export function translatePlan(name, pre) {
  if (!pre.eligible) return { go: false, message: pre.reason || `${name} cannot be translated` };
  if (!pre.missing.length) return { go: false, message: `${name} already has a subtitle in every wanted language` };
  return { go: true, targets: pre.missing, confirm: `Translate the subtitle of ${name} into ${pre.missing.join(", ")}?` };
}
