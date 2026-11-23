// Anonymous usage stats card (feature 058, part 2), loaded when Settings › Advanced opens.
// Off by default. The exact JSON that would be sent is shown (as text) before and after the
// toggle; nothing is sent from the browser.

import { get, post, put } from "./api.js";
import { h, toast, toastError } from "./app.js";

/** Fill `card` (a section element) with the usage stats controls */
export async function fillUsageStats(card, alive = () => true) {
  const box = h("input", { type: "checkbox", id: "set-usage-stats-enabled" });
  const pre = h("pre", { class: "usage-json", tabindex: "0", "aria-label": "Usage stats payload" });
  const status = h("p", { class: "muted", role: "status" });
  const reset = h("button", { type: "button", class: "btn" }, "Reset id");
  const show = (v) => {
    box.checked = v.enabled;
    pre.textContent = JSON.stringify(v.payload, null, 2);
    status.textContent = !v.enabled
      ? "Off: nothing is sent."
      : !v.endpoint_set
        ? "On, but no receiver is set up yet, so nothing is sent."
        : v.last_sent
          ? `Last sent ${new Date(v.last_sent).toLocaleString()}.`
          : "On: sent about once a day. Not sent yet.";
  };
  const run = async (fn, undo) => {
    box.disabled = reset.disabled = true;
    try {
      const v = await fn();
      show(v);
      toast(v.message);
    } catch (e) {
      undo?.();
      toastError(e);
    } finally {
      box.disabled = reset.disabled = false;
    }
  };
  box.addEventListener("change", () => run(() => put("/usage-stats", { enabled: box.checked }), () => (box.checked = !box.checked)));
  reset.addEventListener("click", () => run(() => post("/usage-stats/reset-id", {})));
  try {
    show(await get("/usage-stats"));
  } catch (e) {
    if (alive()) card.append(h("p", { class: "error-text", text: e.error || String(e) }));
    return;
  }
  if (!alive()) return;
  card.replaceChildren(
    h("h2", { text: "Usage stats" }),
    h("p", { class: "muted", text: "Off by default. Turn it on to help show which features are used: a random id, version, install kind, OS, which features are on and rough size buckets. Never names, paths, hosts, Put.io data or tokens." }),
    h("label", { class: "field check", for: "set-usage-stats-enabled" }, box, h("span", { text: "Send anonymous usage stats once a day" })),
    status,
    h("h3", { text: "This is exactly what would be sent" }),
    pre,
    h("div", { class: "form-foot" }, reset, h("small", { class: "muted", text: "Makes a new random id; the old one is forgotten." })),
  );
}
