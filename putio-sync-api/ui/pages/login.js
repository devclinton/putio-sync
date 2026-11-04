// Sign-in page: checks the token with POST /auth/login and keeps it only when accepted.
// The shell sends signed-out visitors here, with `reason` and, after a 429, `retryAfter`.

import { authStatus, login } from "../api.js";
import { afterLogin, h } from "../app.js";

let countdown = null;
let mounted = false;

export function mount(el, params = {}) {
  mounted = true;
  const message = h("p", { class: "muted", role: "status" }, params.reason || null);
  const body = h("div", {}, h("p", { class: "muted" }, "Checking the daemon…"));
  el.append(h("section", { class: "card login" }, h("h1", { text: "Sign in" }), message, body));
  load(body, message, params);
}

export function unmount() {
  mounted = false;
  stopCountdown();
}

async function load(body, message, params) {
  let configured;
  try {
    const status = await authStatus();
    configured = status.token_configured;
    // Tailscale sign-in let this browser in (feature 036): no token needed
    if (status.tailscale && mounted) {
      afterLogin();
      return;
    }
  } catch (e) {
    if (!mounted) return;
    const retry = h("button", { type: "button", class: "btn" }, "Retry");
    retry.addEventListener("click", () => {
      body.replaceChildren(h("p", { class: "muted" }, "Checking the daemon…"));
      load(body, message, params);
    });
    body.replaceChildren(
      h("p", { class: "error-text", text: e.network ? "Can't reach the daemon." : e.error }),
      retry,
    );
    return;
  }
  if (!mounted) return;

  if (!configured) {
    message.replaceChildren();
    body.replaceChildren(
      h(
        "p",
        { class: "notice" },
        "No API token is configured, so the daemon refuses every request. Set ",
        h("code", { text: "[api] auth_token" }),
        " or ",
        h("code", { text: "PUTIO_SYNC_API_TOKEN" }),
        " and restart.",
      ),
    );
    return;
  }

  body.replaceChildren(form(message));
  if (params.retryAfter) startCountdown(message, params.retryAfter);
}

function form(message) {
  const input = h("input", {
    id: "login-token",
    type: "password",
    name: "token",
    autocomplete: "current-password",
    autocapitalize: "off",
    spellcheck: "false",
    required: true,
    autofocus: true,
  });
  const submit = h("button", { type: "submit", class: "btn btn-primary" }, "Sign in");
  const el = h(
    "form",
    { class: "login-form", novalidate: true },
    h("label", { for: "login-token", text: "API token" }),
    input,
    submit,
  );

  el.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (countdown) return;
    const token = input.value.trim();
    if (!token) {
      showError(message, "Enter the API token.");
      input.focus();
      return;
    }
    submit.disabled = true;
    try {
      await login(token);
      if (mounted) afterLogin();
      return;
    } catch (e) {
      if (!mounted) return;
      if (e.status === 429) {
        startCountdown(message, e.retryAfter);
      } else if (e.network) {
        showError(message, "Can't reach the daemon.");
      } else if (e.status === 401) {
        showError(message, e.details ? `${e.error}. ${e.details}.` : "Token not accepted.");
      } else {
        showError(message, e.error);
      }
    }
    if (!countdown) submit.disabled = false;
    input.select();
    input.focus();
  });

  queueMicrotask(() => input.focus());
  return el;
}

function showError(message, text) {
  message.className = "error-text";
  message.replaceChildren(text);
}

/** Show "Too many attempts" and keep the form disabled until `seconds` have passed. */
function startCountdown(message, seconds) {
  stopCountdown();
  const submit = document.querySelector(".login-form button[type=submit]");
  let left = Math.max(1, Number(seconds) || 60);
  const render = () => {
    showError(message, `Too many attempts, try again in ${left} s.`);
  };
  render();
  if (submit) submit.disabled = true;
  countdown = setInterval(() => {
    left -= 1;
    if (left > 0) {
      render();
      return;
    }
    stopCountdown();
    message.className = "muted";
    message.replaceChildren("You can try again now.");
    if (submit) submit.disabled = false;
  }, 1000);
}

function stopCountdown() {
  clearInterval(countdown);
  countdown = null;
}
