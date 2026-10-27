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

/** The Radarr / Sonarr download client: connection details, servers, item counts. */
export function arrStatus() {
  return get("/arr");
}

/** Test the *arr server at `index`. Resolves with `{ok, app, version}` or `{ok: false, reason, detail}`. */
export function testArrServer(index) {
  return post(`/arr/servers/${index}/test`);
}
