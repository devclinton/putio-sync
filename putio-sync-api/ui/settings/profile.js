// Settings › General › Usage profile (feature 056): answer the setup wizard's "How will you
// use putio-sync?" again. The preview lists what changes; settings changed by hand since the
// profile set them are kept and listed. Saving applies live, like any settings save.

import { post, put, get } from "../api.js";
import { h, fill, toast } from "../app.js";
import { startAnswers } from "../profile.js";
import { diffList, reviewList, usageForm } from "../profile_view.js";
import { quiet, reason } from "./form.js";

export function profileCard(ctx) {
  const questions = h("div", {}, h("p", { class: "muted", text: "Loading…" }));
  const review = h("div", { role: "status" });
  let form = null;
  let presets = [];
  let answers = null;
  let overrides = {};
  let timer = 0;

  const failed = (e) => quiet(e) || fill(review, h("p", { class: "error-text", text: reason(e) }));

  const save = h("button", {
    type: "button",
    class: "btn btn-primary",
    text: "Save",
    hidden: true,
    onclick: async () => {
      save.disabled = true;
      try {
        const res = await put("/settings/profile", { answers, overrides });
        toast(res.changes.length ? "Usage profile saved" : "Saved; nothing changed");
        fill(review, h("p", { class: "notice", text: "Saved." }), diffList(res.changes, res.kept));
        save.hidden = true;
        if (ctx.onSaved) ctx.onSaved();
      } catch (e) {
        failed(e);
      }
      save.disabled = false;
    },
  });

  const show = async () => {
    try {
      const r = await post("/settings/profile/preview", { answers, overrides });
      if (!ctx.alive()) return;
      fill(
        review,
        reviewList(r, {
          presets,
          onChange: (id, o) => {
            overrides[id] = o;
            clearTimeout(timer);
            timer = setTimeout(show, 250);
          },
        }),
        diffList(r.changes, r.kept),
      );
      save.hidden = false;
    } catch (e) {
      save.hidden = true;
      failed(e);
    }
  };

  const previewBtn = h("button", {
    type: "button",
    class: "btn",
    text: "Preview",
    onclick: () => {
      if (!form) return;
      answers = form.read();
      if (!answers.kinds.length) return fill(review, h("p", { class: "error-text", text: "Pick at least one kind of download." }));
      overrides = {};
      show();
    },
  });

  get("/settings/profile")
    .then((d) => {
      if (!ctx.alive()) return;
      presets = d.presets || [];
      form = usageForm({ start: startAnswers(d.answers, d.found, d.watch_dir), found: d.found || {} });
      fill(questions, d.answers ? null : h("p", { class: "muted", text: "No usage profile yet: these answers are what your Put.io folders suggest." }), form.node);
    })
    .catch((e) => quiet(e) || fill(questions, h("p", { class: "error-text", text: reason(e) })));

  return h(
    "section",
    { class: "card", id: "card-profile" },
    h("h2", { text: "Usage profile" }),
    h("p", { class: "muted", text: "The setup wizard's questions. Change the answers to see which settings they would change; settings you changed by hand are kept." }),
    questions,
    h("div", { class: "row-actions" }, previewBtn, save),
    review,
  );
}
