// Feature 038: wording and grouping for the Active page's pipeline timeline. Plain
// functions with no DOM, so tests/ui/timeline.test.mjs can run them under `node --test`.

export const STAGE_LABEL = {
  pre_download: "Pre-download",
  downloading: "Downloading",
  pre_processing: "Pre-processing",
  subtitles: "Subtitles",
  translating: "Translating",
  post_processing: "Post-processing",
  notifications: "Notifications",
};

export const STEP_STATE_LABEL = {
  waiting: "Waiting",
  running: "Running",
  done: "Done",
  failed: "Failed",
  skipped: "Skipped",
  interrupted: "Interrupted",
  not_reached: "Not reached",
};

export const SKIP_LABEL = {
  not_configured: "not configured",
  category: "not for this category",
  operator: "skipped by you",
  hook: "the hook asked to skip",
  no_video: "no video in this item",
};

/**
 * What a step is called (feature 025): the subtitle steps are named after what they work on,
 * the scan "videos" and each video by its path; show the file name, not the whole path.
 */
export function stepTitle(step) {
  if (step.kind === "subtitle_scan") return "Find videos";
  if (step.kind === "subtitles") return `Subtitles for ${String(step.name).split("/").pop()}`;
  if (step.kind === "translate_scan") return "Find subtitles to translate";
  if (step.kind === "translate") return `${String(step.name).split("/").pop()}${step.branch ? ` (${step.branch})` : ""}`;
  return step.name;
}

/**
 * The line on an Active row: "Post-processing: tidy", or "Needs you: tidy failed". While a
 * subtitle step has not started the item is waiting for subtitles; its Put.io files are not
 * deleted until the stage ends.
 */
export function stepLine(step, itemState) {
  if (!step) return null;
  const title = stepTitle(step);
  if (itemState === "waiting_for_operator" || step.state === "failed") return `Needs you: ${title} failed`;
  const stage = STAGE_LABEL[step.stage] || step.stage;
  if (step.stage === "subtitles" && step.state === "waiting") return "Waiting for subtitles";
  const waiting = step.waiting && step.waiting.for === "callback" ? ", waiting for callback" : "";
  return step.stage === "subtitles" ? title : `${stage}: ${title}${waiting}`;
}

/** Whole seconds from `from` to `to` (SQLite or RFC 3339 UTC text); null when either is missing. */
export function seconds(from, to) {
  if (!from || !to) return null;
  const parse = (t) => Date.parse(/[zZ]|[+-]\d\d:?\d\d$/.test(t) ? t : `${t.replace(" ", "T")}Z`);
  const a = parse(from);
  const b = parse(to);
  return Number.isNaN(a) || Number.isNaN(b) ? null : Math.max(0, Math.round((b - a) / 1000));
}

/** Time a step took: first attempt start to last attempt end; null while it has not run or still runs. */
export function stepSeconds(step) {
  const tries = step.attempts || [];
  if (tries.length === 0) return null;
  const last = tries[tries.length - 1];
  return seconds(tries[0].started_at, last.ended_at);
}

/** What an attempt reads like: "#2 failed (exit 3)", "#1 timeout", "#1 done". */
export function attemptLabel(a) {
  const result = a.result || "running";
  return `#${a.n} ${result}${a.exit_code != null && result !== "done" ? ` (exit ${a.exit_code})` : ""}`;
}

/** Split stages into those worth showing and the skipped ones that go under "show skipped". */
export function splitStages(stages) {
  const shown = [];
  const skipped = [];
  for (const stage of stages || []) {
    const steps = [...(stage.steps || []), ...Object.values(stage.branches || {}).flat()];
    const idle = stage.skipped || steps.length === 0 || steps.every((s) => s.state === "skipped" || s.state === "not_reached");
    (idle && stage.stage !== "downloading" ? skipped : shown).push(stage);
  }
  return { shown, skipped };
}

/** Why a skipped step did not run, in words; "" when it was not skipped. */
export function skipReason(step) {
  return step.state === "skipped" ? SKIP_LABEL[step.skip_reason] || "skipped" : "";
}

/** Retry and Skip are offered on a failed step; Retry also on an interrupted one. */
export function stepActions(step) {
  return { retry: step.state === "failed" || step.state === "interrupted", skip: step.state === "failed" };
}
