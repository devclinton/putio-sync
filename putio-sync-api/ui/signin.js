// Sign-in options (feature 019): who the top bar shows. The rest of the sign-in page's
// and Settings › Sign-in's text is in signin_text.js, loaded with them.

export const PROVIDERS = ["google", "github", "discord"];

const TITLES = { google: "Google", github: "GitHub", discord: "Discord", password: "password" };

/** "Google", "GitHub", "Discord" */
export const providerTitle = (p) => TITLES[p] || p;

/** The top bar's "who": `{ textContent, title }` for a Tailscale identity or a session user */
export function who(id) {
  if (!id) return { textContent: "", title: "" };
  if (!id.session) return { textContent: `${id.login} (Tailscale)`, title: `Signed in by Tailscale as ${id.name}` };
  const how = providerTitle(id.via);
  return { textContent: `${id.login} (${how})`, title: `Signed in with ${how} as ${id.name || id.login}` };
}
