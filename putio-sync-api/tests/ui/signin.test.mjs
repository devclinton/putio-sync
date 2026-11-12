// Feature 019: the sign-in page and Settings › Sign-in helpers in ui/signin.js and ui/signin_text.js.
// Run with `node --test`; tests/ui_js.rs runs it in cargo test.

import assert from "node:assert/strict";
import { test } from "node:test";
import { providerTitle, who } from "../../ui/signin.js";
import { errorText, oauthHref, parseUsers, providerProblem, safeReturn } from "../../ui/signin_text.js";

test("providers have their names", () => {
  assert.equal(providerTitle("github"), "GitHub");
  assert.equal(providerTitle("password"), "password");
});

test("refusals say why", () => {
  assert.match(errorText("not_allowed"), /isn't allowed/);
  assert.match(errorText("state"), /another browser/);
  assert.match(errorText("weird"), /didn't work/);
  assert.equal(errorText(null), null);
});

test("returns stay inside the UI", () => {
  assert.equal(safeReturn("#/settings/sign-in"), "#/settings/sign-in");
  assert.equal(safeReturn("#/login?error=state"), "#/status");
  assert.equal(safeReturn("https://evil.example"), "#/status");
  assert.equal(safeReturn("#/x<script>"), "#/status");
  assert.equal(safeReturn(null), "#/status");
  assert.equal(oauthHref("google", "#/queue"), "/auth/oauth/google?return=%23%2Fqueue");
});

test("the top bar says who and how", () => {
  assert.deepEqual(who(null), { textContent: "", title: "" });
  assert.equal(who({ login: "a@ts.net", name: "A", via: "serve" }).textContent, "a@ts.net (Tailscale)");
  const github = who({ login: "octocat", name: "Octo", via: "github", session: true });
  assert.equal(github.textContent, "octocat (GitHub)");
  assert.equal(github.title, "Signed in with GitHub as Octo");
  assert.equal(who({ login: "admin", name: "", via: "password", session: true }).title, "Signed in with password as admin");
});

test("allow-lists split on lines and commas", () => {
  assert.deepEqual(parseUsers(" a@example.com\n\n@example.org, octocat "), ["a@example.com", "@example.org", "octocat"]);
  assert.deepEqual(parseUsers(""), []);
});

test("a provider that's on needs its fields", () => {
  assert.equal(providerProblem({ name: "google", enabled: false }), null);
  assert.equal(
    providerProblem({ name: "discord", enabled: true, client_id: "id", client_secret: "", allowed_users: [] }),
    "Discord is on but has no client secret, allowed users.",
  );
  assert.equal(providerProblem({ name: "github", enabled: true, client_id: "id", client_secret: "********", allowed_users: ["me"] }), null);
});
