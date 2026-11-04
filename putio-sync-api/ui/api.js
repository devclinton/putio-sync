// Fetch wrapper for the putio-sync API: bearer token, JSON in and out, typed errors.

const TOKEN_KEY = "putio-sync.token";

let expiredHandler = null;

/** Error thrown for any failed request. `status` is 0 when the daemon was unreachable. */
export class ApiError extends Error {
  constructor(status, error, details = null, retryAfter = null) {
    super(error);
    this.status = status;
    this.error = error;
    this.details = details;
    this.retryAfter = retryAfter;
    this.network = status === 0;
  }
}

// Who Tailscale sign-in let in (feature 036): `{login, name, via}`, or null
let identity = null;

export const getIdentity = () => identity;

/** Ask /auth/status who this browser is; keeps and returns the whole answer */
export async function authStatus() {
  const res = await fetch("/auth/status", { headers: { Accept: "application/json" } });
  const status = res.ok ? await res.json() : {};
  identity = status.tailscale || null;
  return status;
}

export function getToken() {
  try {
    return window.localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

function setToken(token) {
  try {
    if (token == null) window.localStorage.removeItem(TOKEN_KEY);
    else window.localStorage.setItem(TOKEN_KEY, token);
  } catch {
    // Private mode or storage disabled: the session lasts until the page closes
  }
}

/** Register the handler called when a request is refused (401, or 429 rate limit). */
export function onAuthExpired(fn) {
  expiredHandler = fn;
}

function expired(err) {
  window.dispatchEvent(new CustomEvent("auth:expired", { detail: err }));
  if (expiredHandler) expiredHandler(err);
}

export async function request(method, path, body) {
  const headers = { Accept: "application/json" };
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  const init = { method, headers };
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }

  let res;
  try {
    res = await fetch(path, init);
  } catch {
    throw new ApiError(0, "Can't reach the daemon");
  }

  if (res.status === 204) return null;
  const text = await res.text();
  let data = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
  }
  if (res.ok) return data;

  const retry = Number.parseInt(res.headers.get("Retry-After") || "", 10);
  const err = new ApiError(
    res.status,
    (data && data.error) || res.statusText || `HTTP ${res.status}`,
    (data && data.details) || null,
    Number.isFinite(retry) ? retry : null,
  );
  if (path !== "/auth/login") {
    if (res.status === 401) {
      setToken(null);
      expired(err);
    } else if (res.status === 429) {
      expired(err);
    }
  }
  throw err;
}

export const get = (path) => request("GET", path);
export const post = (path, body) => request("POST", path, body);
export const put = (path, body) => request("PUT", path, body);

// Configuration assistant (feature 015)
export const settingsAssistant = () => get("/settings/assistant");
export const previewSettings = (group, body) => post(`/settings/${group}/preview`, body);
export const exportSettings = () => get("/settings/export");
export const importSettings = (toml, apply) => post("/settings/import", { toml, apply });
export const del = (path) => request("DELETE", path);

/** Check `token` with the daemon and keep it only when accepted (204). */
export async function login(token) {
  await request("POST", "/auth/login", { token });
  setToken(token);
}

export function logout() {
  setToken(null);
}

/** Start a folder sync now. Resolves with the sync object; 409 means one is running. */
export function syncNow() {
  return post("/local/sync");
}

/** The Status page's Radarr / Sonarr card: `{enabled, clients_seen, items}` */
export function arrRuntime() {
  return get("/arr/status");
}

/** The Radarr / Sonarr card: `{client, applications}`. Never a password or API key. */
export function arrStatus() {
  return get("/arr");
}

/** Save the download client form `{enabled, username, password}`; an empty password keeps it. */
export function saveArrClient(body) {
  return put("/arr/client", body);
}

/** Look for Radarr and Sonarr (at most 10 s). Resolves with `{instances, tried}`; 409 while one runs. */
export function discoverArr() {
  return post("/arr/discover");
}

/** What auto-setup would change. Resolves with a plan, or `{ok: false, reason, detail}`. */
export function previewArrSetup(req) {
  return post("/arr/setup/preview", req);
}

/** Run auto-setup. Resolves with a result whose `outcome` may be `failed`. */
export function runArrSetup(req) {
  return post("/arr/setup", req);
}

/**
 * Test the saved application at `index` and re-read its categories. Resolves with
 * `{ok, app, version, categories, client_present}` or `{ok: false, reason, detail}`.
 */
export function testArrApplication(index) {
  return post(`/arr/applications/${index}/test`);
}

/** Test unsaved form values; an empty `api_key` with `index` uses that application's saved key. */
export function testArrForm(body) {
  return post("/arr/applications/test", body);
}

/** Add an application `{kind, name, host, port, ssl, url_base, api_key}`. Resolves with `{index, message}`. */
export function addArrApplication(body) {
  return post("/arr/applications", body);
}

/** Edit the application at `index`; an empty `api_key` keeps the saved one. */
export function saveArrApplication(index, body) {
  return put(`/arr/applications/${index}`, body);
}

/** Remove the application at `index` from the config. Radarr/Sonarr itself is left as it is. */
export function deleteArrApplication(index) {
  return del(`/arr/applications/${index}`);
}

/** What the release pre-check rejected in `range`: `{range, rejected, magnets, reports}`. */
export function releaseStats(range, tz) {
  return get(`/stats/releases?${new URLSearchParams({ range, tz: String(tz) })}`);
}

/**
 * Save the release reports for `range` as `format` ("json" or "csv"). The export needs the
 * token, so it is fetched here and handed to the browser as a file.
 */
export async function downloadReports(range, format) {
  const path = `/releases/reports?${new URLSearchParams({ range, format })}`;
  const headers = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  let res;
  try {
    res = await fetch(path, { headers });
  } catch {
    throw new ApiError(0, "Can't reach the daemon");
  }
  if (!res.ok) return request("GET", path); // for the error, and the 401 handling
  const name = /filename="([^"]+)"/.exec(res.headers.get("Content-Disposition") || "");
  const url = URL.createObjectURL(await res.blob());
  const a = document.createElement("a");
  a.href = url;
  a.download = name ? name[1] : `putio-sync-releases-${range}.${format}`;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
