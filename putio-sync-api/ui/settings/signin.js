// Settings › Sign-in (feature 019): a username and password, and Google, GitHub and Discord
// with an allow-list each. Loaded the first time the section opens (pages/settings.js).
// Secrets never come back: the client secret reads "********" and is kept unless replaced,
// and the password is only "set" or not.

import { get, post, put } from "../api.js";
import { confirm, h, toast } from "../app.js";
import { PROVIDERS, providerTitle } from "../signin.js";
import { parseUsers, providerProblem } from "../signin_text.js";
import { quiet, reason } from "./form.js";

const MASK = "********";

export const cards = (ctx) => [signIn(ctx)];

const field = (id, label, input, hint) => h("label", { class: "field", for: id }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);

function signIn(ctx) {
  const card = h("section", { class: "card", id: "card-sign-in" }, h("h2", { text: "Sign-in" }), h("p", { class: "muted", text: "Loading…" }));
  let form = null;
  const dirty = () => ctx.report("sign-in", { dirty: true });
  const load = async () => {
    try {
      const data = await get("/settings/sign-in");
      if (!ctx.alive()) return;
      form = {
        public_url: data.public_url || "",
        session_hours: data.session_hours,
        password: { username: data.password.username, set: data.password.set, new_password: "", clear: false },
        providers: Object.fromEntries(PROVIDERS.map((p) => [p, { ...data.providers[p], name: p }])),
      };
      draw();
      ctx.report("sign-in", { dirty: false, problem: false });
    } catch (e) {
      if (!ctx.alive() || quiet(e)) return;
      ctx.report("sign-in", { problem: true });
      card.replaceChildren(h("h2", { text: "Sign-in" }), h("p", { class: "error-text", text: reason(e) }), h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"));
    }
  };
  const input = (id, value, on, attrs = {}) => {
    const el = h("input", { class: "input", id, autocomplete: "off", spellcheck: "false", ...attrs });
    el.value = value;
    el.addEventListener("input", () => {
      on(el.value);
      dirty();
    });
    return el;
  };
  const provider = (p) => {
    const f = form.providers[p];
    const id = (k) => `set-sign-in-${p}-${k}`;
    const on = h("input", { type: "checkbox", id: id("enabled"), checked: f.enabled });
    on.addEventListener("change", () => {
      f.enabled = on.checked;
      dirty();
    });
    const users = h("textarea", { class: "input", id: id("allowed_users"), rows: 3, spellcheck: "false" });
    users.value = (f.allowed_users || []).join("\n");
    users.addEventListener("input", () => {
      f.allowed_users = parseUsers(users.value);
      dirty();
    });
    const copy = h("button", { type: "button", class: "btn btn-small" }, "Copy");
    copy.addEventListener("click", () => navigator.clipboard?.writeText(f.redirect_url).then(() => toast("Copied"), () => {}));
    const title = providerTitle(p);
    return h(
      "fieldset",
      { class: "choice spaced", id: `sign-in-${p}` },
      h("legend", { text: title }),
      h("label", { class: "field check", for: on.id }, on, h("span", { text: `Allow sign-in with ${title}` })),
      field(id("client_id"), "Client ID", input(id("client_id"), f.client_id, (v) => (f.client_id = v))),
      field(
        id("client_secret"),
        "Client secret",
        input(id("client_secret"), f.client_secret === MASK ? "" : f.client_secret, (v) => (f.client_secret = v || (f.was_set ? MASK : "")), {
          type: "password",
          autocomplete: "new-password",
          placeholder: f.client_secret === MASK ? "Set; type to replace" : "",
        }),
      ),
      field(users.id, "Allowed accounts", users, p === "github" ? "GitHub usernames or verified emails, one per line" : "Verified emails, or account names, one per line"),
      h("div", { class: "field" }, h("span", { text: "Redirect URL to register with the provider" }), h("div", { class: "toolbar" }, h("code", { text: f.redirect_url }), copy)),
    );
  };
  const draw = () => {
    for (const p of PROVIDERS) form.providers[p].was_set = form.providers[p].client_secret === MASK;
    const error = h("p", { class: "error-text", role: "alert", hidden: true });
    const pw = form.password;
    const pass = input("set-sign-in-password-new", "", (v) => (pw.new_password = v), {
      type: "password",
      autocomplete: "new-password",
      placeholder: pw.set ? "Set; type to replace" : "At least 8 characters",
    });
    const clear = h("input", { type: "checkbox", id: "set-sign-in-password-clear" });
    clear.addEventListener("change", () => {
      pw.clear = clear.checked;
      pass.disabled = clear.checked;
      dirty();
    });
    const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
    const everywhere = h("button", { type: "button", class: "btn" }, "Sign out everywhere");
    everywhere.addEventListener("click", async () => {
      if (!(await confirm({ title: "Sign out everywhere?", text: "Every browser signed in with a password or a provider has to sign in again, this one too. The API token and Tailscale still work.", verb: "Sign out" }))) return;
      try {
        await post("/settings/sign-in/sign-out-everywhere");
        toast("Signed out everywhere");
      } catch (e) {
        if (!quiet(e)) toast(reason(e));
      }
    });
    const submit = async (ev) => {
      ev.preventDefault();
      const problem = PROVIDERS.map((p) => providerProblem(form.providers[p])).find(Boolean);
      error.hidden = !problem;
      error.textContent = problem || "";
      if (problem) return;
      const body = {
        public_url: form.public_url.trim() || null,
        session_hours: Number(form.session_hours) || 0,
        password: { username: pw.username, new_password: pw.clear ? null : pw.new_password || null, clear: pw.clear },
        providers: Object.fromEntries(PROVIDERS.map((p) => [p, (({ enabled, client_id, client_secret, allowed_users }) => ({ enabled, client_id, client_secret, allowed_users }))(form.providers[p])])),
      };
      save.disabled = true;
      try {
        const r = await put("/settings/sign-in", body);
        toast(r.message || "Saved");
        ctx.onSaved();
        await load();
      } catch (e) {
        if (!quiet(e)) {
          error.textContent = reason(e);
          error.hidden = false;
        }
      }
      if (save.isConnected) save.disabled = false;
    };
    card.replaceChildren(
      h(
        "form",
        { onsubmit: submit },
        h("h2", { text: "Sign-in" }),
        h("p", { class: "muted", text: "Ways into this page besides the API token. Scripts, Radarr and Sonarr keep using the token." }),
        field("set-sign-in-public_url", "Public address", input("set-sign-in-public_url", form.public_url, (v) => (form.public_url = v), { placeholder: "https://sync.example.com" }), "The address browsers use, for the redirect URLs and secure cookies. Empty uses the address of each request."),
        field("set-sign-in-session_hours", "Stay signed in for (hours)", input("set-sign-in-session_hours", String(form.session_hours), (v) => (form.session_hours = v), { type: "number", min: 1, max: 8760 })),
        h(
          "fieldset",
          { class: "choice spaced" },
          h("legend", { text: "Username and password" }),
          field("set-sign-in-password-username", "Username", input("set-sign-in-password-username", pw.username, (v) => (pw.username = v))),
          field(pass.id, pw.set ? "New password" : "Password", pass, "Also works as HTTP basic auth"),
          pw.set ? h("label", { class: "field check", for: clear.id }, clear, h("span", { text: "Remove the password" })) : null,
        ),
        PROVIDERS.map(provider),
        error,
        h("div", { class: "toolbar spaced" }, save, everywhere),
      ),
    );
  };
  load();
  return card;
}
