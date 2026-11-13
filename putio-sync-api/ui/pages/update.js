// The update card on What's new (feature 055): a newer release, how this install gets it, its
// notes, and when the last check ran. Loaded on demand from the page, so it does not count
// towards the first load. GitHub archive and installer installs get "Update now" (US3):
// the card polls GET /update every 2 s while an attempt runs, then GET /status through the
// restart.

import { get, post } from "../api.js";
import { confirm, copyText, h, toast, toastError } from "../app.js";
import { bytes, relative } from "../format.js";

const POLL_MS = 2000;
const RESTART_WAIT_MS = 3 * 60 * 1000;
const RUNNING = ["waiting_for_idle", "downloading", "verifying", "swapping", "restarting"];

let restartWatch = null;

/** Through the restart: wait for the API to come back, then load the page again (the new
 * version, or the old one with its rollback message) */
function watchRestart(current, line) {
  if (restartWatch) return;
  const until = Date.now() + RESTART_WAIT_MS;
  let away = false;
  restartWatch = setInterval(async () => {
    try {
      const status = await get("/status");
      if (status.version !== current || away) {
        clearInterval(restartWatch);
        location.reload();
      }
    } catch {
      away = true; // the old process has gone
    }
    if (Date.now() > until) {
      clearInterval(restartWatch);
      restartWatch = null;
      line.textContent = "putio-sync hasn't come back after the restart. Check its log.";
      line.className = "error-text";
    }
  }, POLL_MS);
}

/** What the running or last attempt is doing */
function attemptView(t, size) {
  const of = size ? ` of ${bytes(size)}` : "";
  switch (t.state) {
    case "waiting_for_idle":
      return h("p", { text: `Waiting for transfers to finish before updating to ${t.to}…` });
    case "downloading":
      return h("div", {}, h("p", { text: `Downloading ${t.to}: ${bytes(t.downloaded_bytes)}${of}` }), size ? h("progress", { max: size, value: Math.min(t.downloaded_bytes, size), "aria-label": "Download progress" }) : null);
    case "verifying":
      return h("p", { text: "Checking the signature and hashes…" });
    case "swapping":
      return h("p", { text: "Replacing the binary…" });
    case "restarting":
      return h("p", { text: `Restarting into ${t.to}…` });
    case "opened":
      return h("p", { text: `The ${t.to} installer is open. Follow its steps; putio-sync starts again when it finishes.` });
    case "downloaded":
      return h("p", {}, `The ${t.to} installer is downloaded to `, h("code", { text: t.path }), ". Open it to finish the update.");
    case "failed":
      return h("p", { class: "error-text", text: `The update to ${t.to} failed: ${t.error}. Nothing was changed.` });
    default:
      return null;
  }
}

/** "Update now", with what it downloads, for the swap and installer channels */
function updateNow(u, a, reload, problem) {
  let what = "full download";
  if (u.channel.mode === "installer") what = "installer";
  else if (a.patch) what = "patch";
  const size = a.download_bytes ? `${bytes(a.download_bytes)} ${what}` : what;
  const button = h("button", { type: "button", class: "btn btn-primary" }, "Update now");
  const start = async (interrupt) => {
    button.disabled = true;
    try {
      await post("/update/apply", { interrupt });
      await reload();
    } catch (e) {
      button.disabled = false;
      const body = e.body || {};
      if (e.error === "busy") {
        const n = body.active_transfers;
        const ok = await confirm({
          title: "Interrupt transfers?",
          text: `${n} ${n === 1 ? "transfer" : "transfers"} will be interrupted and resume after the restart.`,
          verb: "Update now",
        });
        if (ok) await start(true);
      } else if (e.error === "not_writable") {
        problem.replaceChildren(
          h("p", { class: "error-text" }, "putio-sync can't replace itself: ", h("code", { text: body.path }), " isn't writable by the user it runs as."),
          h("p", {}, "Download the new version from the ", h("a", { href: a.url, target: "_blank", rel: "noopener", text: "release page" }), " and replace the binary by hand."),
        );
        button.remove();
      } else if (e.error === "in_progress") {
        await reload();
      } else {
        toastError(e);
      }
    }
  };
  button.onclick = () => start(false);
  return { button, size };
}
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
  const t = u.attempt;
  const running = t && RUNNING.includes(t.state);
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
  const problem = h("div");
  let sizeLine = null;
  if (a && u.channel.mode !== "instructions" && !running) {
    const { button, size } = updateNow(u, a, reload, problem);
    actions.unshift(button);
    sizeLine = h("p", { class: "muted", text: `Update now downloads a ${size}${u.channel.mode === "swap" ? ", then restarts putio-sync" : ""}.` });
  }
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
  ];
  if (u.rolled_back) {
    const dismissRollback = h("button", { type: "button", class: "btn btn-small" }, "Dismiss");
    dismissRollback.onclick = busy(dismissRollback, () => post("/update/dismiss", { rolled_back: true }));
    status.push(h("div", { class: "cmd" }, h("p", { class: "error-text", text: `The update to ${u.rolled_back.version} was rolled back: ${u.rolled_back.reason}. You're still on ${u.current}.` }), dismissRollback));
  }
  const progress = t ? attemptView(t, a && a.download_bytes) : null;
  const title = a ? `${a.version} is available` : "Updates";
  let subtitle = `You're running ${u.current}, the newest release known.`;
  if (a) subtitle = a.date ? `Released ${a.date}. You're running ${u.current}.` : `You're running ${u.current}.`;
  else if (u.available) subtitle = `You're running ${u.current}; you dismissed the notice for ${u.available.version}.`;
  const card = h(
    "section",
    { class: "card", id: "card-update" },
    h("h2", { text: title }),
    h("p", { text: subtitle }),
    h("p", { class: "muted", text: `Installed with ${u.channel.label}.` }),
    a && u.channel.mode === "instructions" ? howTo(a.instruction, a.url) : null,
    sizeLine,
    progress,
    problem,
    a ? h("div", { class: "stack" }, notes(a.releases)) : null,
    status,
    h("div", { class: "dialog-actions" }, actions),
  );
  if (t && t.state === "restarting") watchRestart(u.current, progress);
  else if (running) setTimeout(() => card.isConnected && reload(), POLL_MS);
  return card;
}
