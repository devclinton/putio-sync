// Sign-in page, loaded by pages/login.js when first shown. It offers what the daemon has
// on: the API token (checked with POST /auth/login and kept only when accepted), the
// [sign_in] username and password, and Google, GitHub or Discord (feature 019). The shell
// sends signed-out visitors here, with `reason` and, after a 429, `retryAfter`; a
// provider that refused comes back as `#/login?error=…`.

import { authStatus, login, passwordLogin } from "../api.js";
import { afterLogin, h } from "../app.js";
import { providerTitle } from "../signin.js";
import { errorText, oauthHref } from "../signin_text.js";

let countdown = null;
let mounted = false;

export function mount(el, params = {}) {
  mounted = true;
  const error = errorText(params.query && params.query.get("error"));
  const message = h("p", { class: error ? "error-text" : "muted", role: "status" }, error || params.reason || null);
  const body = h("div", {}, h("p", { class: "muted" }, "Checking the daemon…"));
  el.append(h("section", { class: "card login" }, h("h1", { text: "Sign in" }), message, body));
  load(body, message, params);
}

export function unmount() {
  mounted = false;
  stopCountdown();
}

async function load(body, message, params) {
  let status;
  try {
    status = await authStatus();
    // Tailscale or a session cookie let this browser in: nothing to enter
    if ((status.tailscale || status.user) && !params.retryAfter && mounted) {
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

  const options = status.sign_in || { password: false, providers: [] };
  if (!status.token_configured && !options.password && !options.providers.length) {
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

  const parts = [];
  if (options.providers.length) {
    parts.push(
      h(
        "div",
        { class: "login-providers" },
        ...options.providers.map((p) =>
          h("a", { class: "btn", href: oauthHref(p, params.back) }, `Sign in with ${providerTitle(p)}`),
        ),
      ),
    );
  }
  if (options.password) parts.push(passwordForm(message));
  if (status.token_configured) {
    const token = tokenForm(message);
    // With other ways in, the token is for scripts mostly: tuck it away
    parts.push(parts.length ? h("details", { class: "login-token" }, h("summary", {}, "Use the API token"), token) : token);
  }
  body.replaceChildren(...parts);
  if (params.retryAfter) startCountdown(message, params.retryAfter);
}

function field(id, label, attrs) {
  const input = h("input", { id, autocapitalize: "off", spellcheck: "false", required: true, ...attrs });
  return [h("label", { for: id, text: label }), input];
}

function passwordForm(message) {
  const [userLabel, user] = field("login-user", "Username", { type: "text", name: "username", autocomplete: "username", autofocus: true });
  const [passLabel, pass] = field("login-password", "Password", { type: "password", name: "password", autocomplete: "current-password" });
  return submitting(h("form", { class: "login-form", novalidate: true }, userLabel, user, passLabel, pass), message, async () => {
    if (!user.value.trim() || !pass.value) return { focus: user.value.trim() ? pass : user, text: "Enter the username and password." };
    await passwordLogin(user.value.trim(), pass.value);
    await authStatus();
    return null;
  }, pass, "Wrong username or password.");
}

function tokenForm(message) {
  const [label, input] = field("login-token", "API token", { type: "password", name: "token", autocomplete: "current-password", autofocus: true });
  return submitting(h("form", { class: "login-form", novalidate: true }, label, input), message, async () => {
    const token = input.value.trim();
    if (!token) return { focus: input, text: "Enter the API token." };
    await login(token);
    return null;
  }, input, "Token not accepted.");
}

/** Add a submit button and handler to `form`: `attempt` signs in, or returns what to fix */
function submitting(form, message, attempt, retryInput, refused) {
  const submit = h("button", { type: "submit", class: "btn btn-primary" }, "Sign in");
  form.append(submit);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (countdown) return;
    submit.disabled = true;
    try {
      const fix = await attempt();
      if (!fix) {
        if (mounted) afterLogin();
        return;
      }
      showError(message, fix.text);
      fix.focus.focus();
    } catch (e) {
      if (!mounted) return;
      if (e.status === 429) {
        startCountdown(message, e.retryAfter);
      } else if (e.network) {
        showError(message, "Can't reach the daemon.");
      } else if (e.status === 401) {
        showError(message, e.details ? `${e.error}. ${e.details}.` : refused);
      } else {
        showError(message, e.error);
      }
      retryInput.select();
      retryInput.focus();
    }
    if (!countdown) submit.disabled = false;
  });
  return form;
}

function showError(message, text) {
  message.className = "error-text";
  message.replaceChildren(text);
}

/** Show "Too many attempts" and keep the forms disabled until `seconds` have passed. */
function startCountdown(message, seconds) {
  stopCountdown();
  const submits = document.querySelectorAll(".login-form button[type=submit]");
  const disable = (on) => submits.forEach((b) => (b.disabled = on));
  let left = Math.max(1, Number(seconds) || 60);
  const render = () => {
    showError(message, `Too many attempts, try again in ${left} s.`);
  };
  render();
  disable(true);
  countdown = setInterval(() => {
    left -= 1;
    if (left > 0) {
      render();
      return;
    }
    stopCountdown();
    message.className = "muted";
    message.replaceChildren("You can try again now.");
    disable(false);
  }, 1000);
}

function stopCountdown() {
  clearInterval(countdown);
  countdown = null;
}
