// The update card on What's new (feature 055): a newer release, how this install gets it, its
// notes, and when the last check ran. Loaded on demand from the page, so it does not count
// towards the first load.

import { post } from "../api.js";
import { copyText, h, toast, toastError } from "../app.js";
import { relative } from "../format.js";
import { notes, pending } from "./news.js";

/** A command with a Copy button */
function command(text) {
  const copy = h("button", { type: "button", class: "btn btn-small" }, "Copy");
  copy.onclick = () => copyText(text).then(() => toast("Copied"), () => toast("Couldn't copy", "error", text));
  return h("div", { class: "cmd" }, h("code", { text }), copy);
}

/** How a managed install gets the new version: a download, commands, a pinned
 * alternative, then any steps. These channels never get an Update button (FR-010). */
function howTo(i, releaseUrl) {
  return [
    i.link && i.link !== releaseUrl ? h("p", {}, h("a", { href: i.link, target: "_blank", rel: "noopener", text: `Download ${i.link.split("/").pop()}` })) : null,
    i.commands.map(command),
    i.alternatives.length ? h("p", { class: "muted", text: "Or pin the new version:" }) : null,
    i.alternatives.map(command),
    i.steps ? h("p", { text: i.steps }) : null,
  ];
}

/** The update card: a newer release and its notes, or when the last check ran */
export function updateCard(u, reload) {
  const a = pending(u);
  const busy = (button, run) => async () => {
    button.disabled = true;
    try {
      await run();
      await reload();
    } catch (e) {
      if (e.error === "checked_recently") toast("Checked less than a minute ago", "ok", "Try again in a minute.");
      else toastError(e);
    } finally {
      button.disabled = false;
    }
  };
  const checkNow = h("button", { type: "button", class: "btn" }, "Check now");
  checkNow.onclick = busy(checkNow, () => post("/update/check"));
  const actions = [checkNow];
  if (a) {
    const dismiss = h("button", { type: "button", class: "btn" }, "Dismiss");
    dismiss.onclick = busy(dismiss, () => post("/update/dismiss", { version: a.version }));
    actions.unshift(h("a", { class: "btn btn-primary", href: a.url, target: "_blank", rel: "noopener", text: "Release page" }), dismiss);
  }
  const c = u.check;
  const last = c.last_attempt_at ? `Last checked ${relative(c.last_attempt_at)}.` : "Not checked yet.";
  const status = [
    h("p", { class: "muted", text: c.enabled ? last : `${last} Checks are off in Settings › General › Updates.` }),
    c.error ? h("p", { class: "error-text", text: `The last check failed: ${c.error}.` }) : null,
    u.rolled_back ? h("p", { class: "error-text", text: `Rolled back from ${u.rolled_back.version}: ${u.rolled_back.reason}` }) : null,
  ];
  const title = a ? `${a.version} is available` : "Updates";
  let subtitle = `You're running ${u.current}, the newest release known.`;
  if (a) subtitle = a.date ? `Released ${a.date}. You're running ${u.current}.` : `You're running ${u.current}.`;
  else if (u.available) subtitle = `You're running ${u.current}; you dismissed the notice for ${u.available.version}.`;
  return h(
    "section",
    { class: "card", id: "card-update" },
    h("h2", { text: title }),
    h("p", { text: subtitle }),
    h("p", { class: "muted", text: `Installed with ${u.channel.label}.` }),
    a && u.channel.mode === "instructions" ? howTo(a.instruction, a.url) : null,
    a ? h("div", { class: "stack" }, notes(a.releases)) : null,
    status,
    h("div", { class: "dialog-actions" }, actions),
  );
}
