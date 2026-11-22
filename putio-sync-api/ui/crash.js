// The "putio-sync stopped unexpectedly" notice (feature 058). After sign-in, shows the newest
// crash report nobody has dismissed. The full report is in a <pre> (text only) so names and
// paths can be checked before anything is reported. Nothing is sent from here: "Report on
// GitHub" opens a prefilled new-issue page in a new tab, and the person submits it themselves.

import { get, post } from "./api.js";
import { copyText, h, toast, toastError } from "./app.js";

const GITHUB = "https://github.com/";

/** Only links to GitHub are ever put in an href */
export const githubUrl = (url) => (typeof url === "string" && url.startsWith(GITHUB) ? url : null);

/** The notice for one report (the API's `GET /crash-reports/{id}`); `onDismiss` runs after a dismissal */
export function crashNotice(report, onDismiss) {
  const report_url = githubUrl(report.url);
  const search_url = githubUrl(report.search_url);
  const dismiss = h("button", { type: "button", class: "btn" }, "Dismiss");
  dismiss.addEventListener("click", async () => {
    dismiss.disabled = true;
    try {
      await post(`/crash-reports/${encodeURIComponent(report.id)}/dismiss`, {});
      onDismiss();
    } catch (e) {
      dismiss.disabled = false;
      toastError(e);
    }
  });
  const copy = h("button", { type: "button", class: "btn" }, "Copy report");
  copy.addEventListener("click", async () => {
    try {
      await copyText(report.text);
      toast("Report copied");
    } catch (e) {
      toastError(e);
    }
  });
  const text = h("pre", { class: "crash-text", tabindex: "0", "aria-label": "Crash report" });
  text.textContent = report.text;
  return h(
    "section",
    { class: "notice crash", role: "alert" },
    h("h2", { text: "putio-sync stopped unexpectedly" }),
    h("p", { class: "muted" }, "Nothing has been sent. Check the report below for names and paths you would rather not share, then report it on GitHub if you like."),
    search_url ? h("p", {}, h("a", { href: search_url, target: "_blank", rel: "noopener", text: "Search existing issues" }), h("span", { class: "muted" }, " Repeats can go on the same issue.")) : null,
    text,
    report.truncated ? h("p", { class: "muted small" }, "The report is too long for the link. Use Copy report and paste it into the issue.") : null,
    h(
      "div",
      { class: "crash-actions" },
      report_url ? h("a", { href: report_url, target: "_blank", rel: "noopener", class: "btn btn-primary", text: "Report on GitHub" }) : null,
      copy,
      dismiss,
    ),
  );
}

/**
 * Once per page load after sign-in: show the notice for the newest undismissed report.
 * Resolves false when the check failed, so the next page tries again.
 */
export async function checkCrash() {
  const slot = document.getElementById("crash");
  if (!slot) return true;
  try {
    const { reports } = await get("/crash-reports");
    const next = reports.find((r) => !r.dismissed);
    if (!next) {
      slot.replaceChildren();
      return true;
    }
    const detail = await get(`/crash-reports/${encodeURIComponent(next.id)}`);
    slot.replaceChildren(crashNotice(detail, checkCrash));
    return true;
  } catch (e) {
    return e?.status === 404; // no crash folder: nothing to show
  }
}
