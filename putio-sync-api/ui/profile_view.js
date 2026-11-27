// Usage profile (feature 056): the "How will you use putio-sync?" questions and the review
// of what they set up, used by the setup wizard and by Settings › Usage profile.

import { h } from "./app.js";
import { current } from "./i18n.js";
import { ARR, KINDS, answersFrom, keyLabel, languagesFrom, note, reason, title, valueText } from "./profile.js";

const check = (checked, onchange) => h("input", { type: "checkbox", checked: Boolean(checked), onchange });

/**
 * The questions. `start` is the answers to show (see `startAnswers`), `found` the kinds whose
 * folders are in Put.io. `onArr(kinds)` runs when Radarr or Sonarr is ticked or cleared.
 * Returns `{ node, read }`; `read()` gives the answers for the daemon.
 */
export function usageForm({ start, found = {}, onArr = () => {} }) {
  const kinds = KINDS.map(([id, label]) => {
    const box = check(start.kinds.includes(id));
    const where = (found[id] || []).join(", ");
    return { id, box, node: h("label", { class: "field check" }, box, h("span", {}, label, where ? h("small", {}, "In Put.io: ", h("span", { translate: "no", text: where })) : null)) };
  });
  const arrChanged = () => onArr(arr.filter((a) => a.box.checked).map((a) => a.id));
  const arr = ARR.map(([id, label]) => {
    const box = check(start.arr.includes(id), arrChanged);
    return { id, box, node: h("label", { class: "field check" }, box, h("span", { text: label })) };
  });
  const byHand = check(start.by_hand);
  const watchPath = h("input", { class: "input", type: "text", value: start.watch || start.suggestedWatch || "", spellcheck: "false", "aria-label": "Watch folder" });
  const watchOn = check(Boolean(start.watch), () => (watchPath.disabled = !watchOn.checked));
  watchPath.disabled = !watchOn.checked;
  queueMicrotask(arrChanged);

  const node = h(
    "div",
    { class: "profile-questions" },
    h("fieldset", { class: "profile-group" }, h("legend", { text: "What do you download?" }), kinds.map((k) => k.node)),
    h(
      "fieldset",
      { class: "profile-group" },
      h("legend", { text: "How do downloads reach Put.io?" }),
      arr.map((a) => a.node),
      h("label", { class: "field check" }, byHand, h("span", {}, "I add them on Put.io myself", h("small", { text: "From the Put.io site or app." }))),
      h("label", { class: "field check" }, watchOn, h("span", {}, "I drop .torrent or .magnet files in a folder", h("small", { text: "putio-sync sends them to Put.io." }))),
      watchPath,
    ),
  );
  const read = () =>
    answersFrom({
      kinds: kinds.filter((k) => k.box.checked).map((k) => k.id),
      arr: arr.filter((a) => a.box.checked).map((a) => a.id),
      byHand: byHand.checked,
      watchOn: watchOn.checked,
      watch: watchPath.value,
      language: current(),
    });
  return { node, read };
}

/** The editor for one recommendation's value */
function editor(rec, presets, changed) {
  if (rec.id.startsWith("rule:")) {
    const known = presets.some((p) => p.name === rec.value);
    const list = known ? presets : [...presets, { name: rec.value, label: rec.value }];
    const sel = h("select", { class: "input", "aria-label": "Preset" }, list.map((p) => h("option", { value: p.name, text: p.label, selected: p.name === rec.value })));
    sel.onchange = () => changed(sel.value);
    return sel;
  }
  if (rec.id === "precheck_watch") {
    const input = h("input", { class: "input input-short", type: "number", min: "0", step: "1", value: String(rec.value), "aria-label": "Smallest size in MB" });
    input.onchange = () => changed(Math.max(0, Math.round(Number(input.value) || 0)));
    return h("label", { class: "inline" }, input, h("span", { text: "MB; 0 turns the check off" }));
  }
  if (rec.id === "delete") {
    const sel = h("select", { class: "input" }, h("option", { value: "yes", text: "Delete", selected: rec.value === true }), h("option", { value: "no", text: "Keep on Put.io", selected: rec.value !== true }));
    sel.onchange = () => changed(sel.value === "yes");
    return sel;
  }
  if (rec.id === "subtitles") {
    const input = h("input", { class: "input", type: "text", value: (rec.value || []).join(", "), spellcheck: "false", "aria-label": "Subtitle languages" });
    input.onchange = () => changed(languagesFrom(input.value));
    return input;
  }
  if (rec.id === "watch") {
    const input = h("input", { class: "input", type: "text", value: rec.value || "", spellcheck: "false", "aria-label": "Watch folder" });
    input.onchange = () => input.value.trim() && changed(input.value.trim());
    return input;
  }
  return h("span", { translate: "no", text: valueText(rec.value) });
}

const RISK_CLASS = { danger: "error-text", warning: "warn-text", info: "muted" };

/**
 * The review: one line per recommendation with an on/off switch and its value, then the
 * risks and notes. `onChange(id, override)` runs when one is changed.
 */
export function reviewList(review, { presets = [], onChange }) {
  const items = (review.recommendations || []).map((rec) => {
    const on = check(!rec.off);
    let value = rec.value;
    const changed = (v) => {
      value = v;
      onChange(rec.id, { off: !on.checked, value });
    };
    on.onchange = () => onChange(rec.id, { off: !on.checked, value });
    const edit = editor(rec, presets, changed);
    if (rec.off) edit.hidden = true;
    return h(
      "li",
      { class: rec.off ? "profile-rec off" : "profile-rec" },
      h("label", { class: "field check" }, on, h("span", {}, h("strong", { text: title(rec) }), h("small", { text: reason(rec) }))),
      edit,
    );
  });
  return h(
    "div",
    { class: "profile-review" },
    items.length ? h("ul", { class: "profile-recs" }, items) : h("p", { class: "muted", text: "Nothing to set up for these answers." }),
    (review.risks || []).map((r) => h("p", { class: RISK_CLASS[r.level] || "warn-text", text: r.message })),
    (review.notes || []).map(note).filter(Boolean).map((t) => h("p", { class: "notice", text: t })),
  );
}

/** What a save changes, and the settings kept because they were changed by hand */
export function diffList(changes = [], kept = []) {
  const row = (key, ...rest) => h("li", {}, h("span", { text: keyLabel(key) }), ": ", ...rest);
  const val = (v) => h("code", { text: valueText(v) });
  return h(
    "div",
    { class: "profile-diff" },
    changes.length
      ? [
          h("h3", { class: "sub", text: "What changes" }),
          h(
            "ul",
            {},
            changes.map((c) => row(c.key, val(c.before), " → ", val(c.after), c.restart ? h("small", { text: " (after a restart)" }) : null)),
          ),
        ]
      : h("p", { class: "muted", text: "No setting changes." }),
    kept.length
      ? [
          h("h3", { class: "sub", text: "Kept as you set them" }),
          h("ul", {}, kept.map((k) => row(k.key, val(k.current), h("small", {}, " · ", h("span", { text: "Profile:" }), " ", val(k.profile))))),
        ]
      : null,
  );
}
