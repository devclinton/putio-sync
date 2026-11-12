// Feature 038: the pipeline timeline of one item, shared by the Active page and History
// (FR-024). The wording and grouping come from timeline.js.

import { h } from "./app.js";
import { duration, percent, relative } from "./format.js";
import { STAGE_LABEL, STEP_STATE_LABEL, attemptLabel, seconds, skipReason, splitStages, stepActions, stepSeconds, stepTitle } from "./timeline.js";

/**
 * An item's stages left to right, steps with attempts. With `onAction(t, step, verb, button)`
 * a failed step offers Retry and Skip; without it the timeline is read-only (History).
 */
export function timelineView(t, onAction) {
  const { shown, skipped } = splitStages(t.stages);
  return h(
    "div",
    { class: "timeline", "data-key": t.key },
    h("div", { class: "timeline-stages" }, shown.map((stage) => stageView(t, stage, onAction))),
    skipped.length
      ? h(
          "details",
          { class: "timeline-skipped" },
          h("summary", { text: `Show skipped (${skipped.length})` }),
          skipped.map((stage) => stageView(t, stage, onAction)),
        )
      : null,
  );
}

function stageView(t, stage, onAction) {
  const lines = [{ title: null, steps: stage.steps || [] }, ...Object.entries(stage.branches || {}).map(([title, steps]) => ({ title, steps }))].filter((l) => l.steps.length > 0);
  return h(
    "section",
    { class: "timeline-stage" },
    h("h3", { text: STAGE_LABEL[stage.stage] || stage.stage }),
    stage.skipped || lines.length === 0
      ? h("p", { class: "muted", text: "Skipped: not configured" })
      : h("div", { class: "timeline-branches" }, lines.map((l) => h("div", { class: "timeline-branch" }, l.title ? h("small", { class: "muted block", text: l.title }) : null, l.steps.map((step) => stepView(t, step, onAction))))),
  );
}

function stepView(t, step, onAction) {
  const took = stepSeconds(step);
  const act = onAction ? stepActions(step) : {};
  const tone = { done: "ok", failed: "bad", running: "active", interrupted: "warn", skipped: "", waiting: "", not_reached: "" }[step.state] || "";
  const tries = step.attempts || [];
  return h(
    "div",
    { class: `timeline-step ${step.state}` },
    h(
      "div",
      { class: "badges" },
      h("strong", { text: stepTitle(step), title: step.name }),
      h("span", { class: `badge ${tone}`, text: STEP_STATE_LABEL[step.state] || step.state }),
      step.required === false ? h("span", { class: "badge", text: "Optional" }) : null,
      took != null ? h("small", { class: "muted", text: took < 1 ? "<1s" : duration(took) }) : null,
      step.progress != null && step.state === "running" ? h("small", { class: "muted", text: percent(step.progress) }) : null,
    ),
    skipReason(step) ? h("small", { class: "muted block", text: skipReason(step) }) : null,
    step.waiting ? h("small", { class: "muted block", text: `Waiting for the service to call back; gives up ${relative(step.waiting.expires_at)}` }) : null,
    step.next_attempt_at && step.state === "waiting" ? h("small", { class: "muted block", text: `Next try ${relative(step.next_attempt_at.replace(" ", "T") + "Z")}` }) : null,
    tries.map((a) =>
      h(
        "details",
        { class: "timeline-attempt" },
        h("summary", { text: `${attemptLabel(a)}${seconds(a.started_at, a.ended_at) != null ? ` · ${duration(seconds(a.started_at, a.ended_at))}` : ""}` }),
        h("pre", { class: "timeline-output", text: a.output || (onAction ? "(no output)" : "(no output; output is kept for a day)") }),
      ),
    ),
    act.retry || act.skip
      ? h(
          "div",
          { class: "row-actions" },
          act.retry ? h("button", { type: "button", class: "btn btn-small", onclick: (ev) => onAction(t, step, "retry", ev.currentTarget) }, "Retry") : null,
          act.skip ? h("button", { type: "button", class: "btn btn-small", onclick: (ev) => onAction(t, step, "skip", ev.currentTarget) }, "Skip") : null,
        )
      : null,
  );
}
