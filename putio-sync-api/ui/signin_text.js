// Sign-in options (feature 019): what the sign-in page and Settings › Sign-in say, kept
// apart from the DOM so tests/ui/signin.test.mjs can check it

import { providerTitle } from "./signin.js";

/** Why an OAuth sign-in came back refused, from `#/login?error=…` */
export function errorText(code) {
  switch (code) {
    case "denied":
      return "The sign-in was cancelled at the provider.";
    case "state":
      return "That sign-in expired or was started in another browser. Try again.";
    case "exchange":
      return "The provider couldn't confirm the sign-in. Check the client ID and secret, then try again.";
    case "not_allowed":
      return "That account isn't allowed to sign in here.";
    default:
      return code ? "The sign-in didn't work. Try again." : null;
  }
}

/** A UI address to come back to after a provider sign-in: `#/…`, else the status page */
export function safeReturn(hash) {
  const h = String(hash || "");
  return /^#\/[A-Za-z0-9/_.?=&-]{0,190}$/.test(h) && !h.startsWith("#/login") ? h : "#/status";
}

/** The link that starts a sign-in with `provider` */
export const oauthHref = (provider, back) => `/auth/oauth/${provider}?return=${encodeURIComponent(safeReturn(back))}`;

/** Lines splitting an allow-list textarea, trimmed, without blanks */
export const parseUsers = (text) =>
  String(text || "")
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter(Boolean);

/** Why the provider form can't be saved, or null */
export function providerProblem(p) {
  if (!p.enabled) return null;
  const missing = [];
  if (!String(p.client_id || "").trim()) missing.push("client ID");
  if (!String(p.client_secret || "").trim()) missing.push("client secret");
  if (!p.allowed_users || !p.allowed_users.length) missing.push("allowed users");
  return missing.length ? `${providerTitle(p.name)} is on but has no ${missing.join(", ")}.` : null;
}
