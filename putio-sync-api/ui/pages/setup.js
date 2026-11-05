// First-run setup wizard (feature 013). The shell mounts it instead of the dashboard
// while /auth/status says setup_required. Every /setup/* call carries the setup code
// (kept in sessionStorage) or an API token; the Put.io token never comes back.

import { fill, h, toast } from "../app.js";
import { bytes } from "../format.js";

const CODE_KEY = "putio-sync.setup-code";
const TOKEN_KEY = "putio-sync.token";
const STEPS = ["Put.io", "Download folder", "What to download", "Extras", "Review"];

let root = null;
let state = null;
let linkTimer = null;

function stored(store, key, value) {
  try {
    if (value === undefined) return window[store].getItem(key);
    if (value === null) window[store].removeItem(key);
    else window[store].setItem(key, value);
  } catch {
    // storage unavailable: the code lasts until the page closes
  }
  return null;
}

/** Call a setup route. Throws `{status, error, body}` on failure. */
async function call(method, path, body) {
  const headers = { Accept: "application/json" };
  const code = state.code || stored("sessionStorage", CODE_KEY);
  if (code) headers.Authorization = `Bearer ${code}`;
  const init = { method, headers };
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  let res;
  try {
    res = await fetch(path, init);
  } catch {
    throw { status: 0, error: "Can't reach the daemon" };
  }
  const text = res.status === 204 ? "" : await res.text();
  let data = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = text;
  }
  if (res.ok) return data;
  throw { status: res.status, error: (data && data.error) || res.statusText, body: data };
}

export function mount(el) {
  root = el;
  state = { step: -1, code: stored("sessionStorage", CODE_KEY), session: null, busy: false };
  document.title = "Set up · putio-sync";
  start();
}

export function unmount() {
  clearTimeout(linkTimer);
  root = null;
}

async function start() {
  if (state.code) {
    try {
      await call("POST", "/setup/unlock");
      await refresh();
      return show(0);
    } catch {
      stored("sessionStorage", CODE_KEY, null);
      state.code = null;
    }
  }
  render(unlockStep());
}

async function refresh() {
  state.session = await call("GET", "/setup/session");
}

function render(...children) {
  if (!root) return;
  root.replaceChildren(h("section", { class: "card setup" }, ...children));
  const first = root.querySelector("input, select, button.btn-primary");
  if (first) first.focus();
}

function progress() {
  return h(
    "ol",
    { class: "setup-steps" },
    STEPS.map((name, i) =>
      h("li", { class: i === state.step ? "now" : i < state.step ? "done" : null, text: name }),
    ),
  );
}

function errorLine() {
  return h("p", { class: "error-text", role: "alert" });
}

function fail(line, e) {
  line.textContent = e.status === 429 ? "Too many attempts; wait a minute." : e.error;
}

function nav({ back = true, next = "Next", onNext, skip = null }) {
  const nextBtn = h("button", { type: "submit", class: "btn btn-primary", text: next });
  return h(
    "div",
    { class: "setup-nav" },
    back && state.step > 0
      ? h("button", { type: "button", class: "btn btn-quiet", text: "Back", onclick: () => show(state.step - 1) })
      : h("span"),
    h(
      "span",
      {},
      skip ? h("button", { type: "button", class: "btn btn-quiet", text: "Skip", onclick: skip }) : null,
      nextBtn,
    ),
  );
}

/** A form whose submit runs `action(err)`; the button is disabled while it runs. */
function form(children, action) {
  const err = errorLine();
  const el = h("form", { class: "setup-form", novalidate: true }, children, err);
  el.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (state.busy) return;
    state.busy = true;
    for (const b of el.querySelectorAll("button")) b.disabled = true;
    err.textContent = "";
    try {
      await action(err);
    } catch (e) {
      fail(err, e);
    }
    state.busy = false;
    for (const b of el.querySelectorAll("button")) b.disabled = false;
  });
  return el;
}

function field(label, input, hint = null) {
  return h("label", { class: "field" }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);
}

function show(step) {
  clearTimeout(linkTimer);
  state.step = step;
  const steps = [putioStep, folderStep, downloadStep, extrasStep, reviewStep];
  render(progress(), steps[step]());
}

// ---- Unlock -----------------------------------------------------------------

function unlockStep() {
  const input = h("input", { class: "input", type: "password", autocomplete: "one-time-code", autocapitalize: "characters", spellcheck: "false" });
  return [
    h("h1", { text: "Set up putio-sync" }),
    h("p", { class: "muted" }, "Enter the setup code printed in the daemon's log or console. An existing API token works too."),
    form([field("Setup code", input), h("button", { type: "submit", class: "btn btn-primary", text: "Continue" })], async (err) => {
      const code = input.value.trim();
      if (!code) {
        err.textContent = "Enter the setup code.";
        return;
      }
      state.code = code;
      try {
        await call("POST", "/setup/unlock");
      } catch (e) {
        state.code = null;
        throw e.status === 401 ? { error: "That code is not right." } : e;
      }
      stored("sessionStorage", CODE_KEY, code);
      await refresh();
      show(0);
    }),
  ];
}

// ---- Put.io -----------------------------------------------------------------

function account(a) {
  return h("p", { class: "notice" }, `Connected as ${a.username}, using ${bytes(a.used)} of ${bytes(a.size)}.`);
}

function putioStep() {
  const s = state.session;
  const box = h("div", {});
  const err = errorLine();
  const token = h("input", { class: "input", type: "password", autocomplete: "off", spellcheck: "false" });
  const paste = form([field("Put.io OAuth token", token, "From put.io → Settings → API, or an existing token."), h("button", { type: "submit", class: "btn", text: "Use token" })], async () => {
    const res = await call("POST", "/setup/putio/token", { token: token.value.trim() });
    token.value = "";
    s.account = res.account;
    show(0);
  });
  const link = h("button", {
    type: "button",
    class: "btn btn-primary",
    text: "Link with Put.io",
    onclick: async () => {
      link.disabled = true;
      try {
        const res = await call("POST", "/setup/putio/link");
        box.replaceChildren(
          h("p", {}, "Go to ", h("a", { href: res.link_url, target: "_blank", rel: "noopener", text: res.link_url }), " and enter:"),
          h("p", { class: "setup-code", text: res.code }),
          h("p", { class: "muted", role: "status", text: "Waiting for Put.io…" }),
        );
        waitForLink(box, err);
      } catch (e) {
        fail(err, e);
        link.disabled = false;
      }
    },
  });
  return [
    h("h2", { text: "Connect Put.io" }),
    s.account ? account(s.account) : null,
    s.app_id_available ? [h("p", { class: "muted" }, "Link this install to your Put.io account without copying a token."), link, box] : null,
    s.app_id_available ? h("h3", { class: "sub", text: "Or paste a token" }) : h("p", { class: "muted" }, "Linking with a code at put.io/link is off: it needs a Put.io app ID. Register one at ", h("a", { href: s.app_register_url, target: "_blank", rel: "noopener", text: s.app_register_url }), " and set it as [putio] app_id or PUTIO_SYNC_APP_ID, or paste a token below."),
    paste,
    err,
    form(nav({ next: "Next" }), async (e) => {
      if (!state.session.account) {
        e.textContent = "Connect Put.io first.";
        return;
      }
      show(1);
    }),
  ];
}

function waitForLink(box, err) {
  linkTimer = setTimeout(async () => {
    if (!root || state.step !== 0) return;
    try {
      const res = await call("GET", "/setup/putio/link");
      if (res.state === "linked") {
        state.session.account = res.account;
        toast("Put.io linked");
        return show(0);
      }
      if (res.state === "expired") {
        box.replaceChildren(h("p", { class: "error-text", text: "The code expired. Link again." }));
        return show(0);
      }
    } catch (e) {
      fail(err, e);
    }
    waitForLink(box, err);
  }, 3000);
}

// ---- Download folder --------------------------------------------------------

function checkLines(c) {
  const lines = [];
  if (c.error) lines.push(h("p", { class: "error-text", text: c.error }));
  else if (c.is_dir && c.writable) lines.push(h("p", { class: "notice", text: "Writable." }));
  else if (!c.exists && c.can_create) lines.push(h("p", { class: "notice", text: "Doesn't exist yet; it will be created." }));
  if (c.free_bytes != null) lines.push(h("p", { class: "muted", text: `${bytes(c.free_bytes)} free of ${bytes(c.total_bytes)}.` }));
  for (const w of c.warnings || []) {
    const text = {
      less_than_putio_used: "Less free space than your Put.io account uses.",
      low_space: "Less than 10 GB free.",
      container_path: "This is a path inside the container: map it to a folder on the host.",
    }[w.warning] || "Check this folder.";
    lines.push(h("p", { class: "warn-text", text }));
  }
  return lines;
}

function folderStep() {
  const s = state.session;
  const input = h("input", { class: "input", type: "text", value: s.download_dir || s.suggestions.download_dir || "", spellcheck: "false" });
  const result = h("div", { role: "status" });
  const check = async () => {
    const c = await call("POST", "/setup/folder/check", { path: input.value.trim() });
    result.replaceChildren(...checkLines(c));
    return c;
  };
  input.addEventListener("change", () => check().catch(() => {}));
  queueMicrotask(() => check().catch(() => {}));
  return [
    h("h2", { text: "Download folder" }),
    h("p", { class: "muted" }, "Where files from Put.io are saved."),
    form(
      [
        field("Folder", input, s.in_container ? "Running in a container: use the path you mapped, such as /downloads." : null),
        result,
        h("button", { type: "button", class: "btn btn-small", text: "Check", onclick: () => check().catch((e) => toast(e.error, "error")) }),
        nav({}),
      ],
      async (err) => {
        try {
          const c = await call("PUT", "/setup/folder", { path: input.value.trim(), create: true });
          s.download_dir = c.path;
        } catch (e) {
          if (e.body && e.body.path !== undefined) {
            result.replaceChildren(...checkLines(e.body));
            err.textContent = "This folder can't be used.";
            return;
          }
          throw e;
        }
        show(2);
      },
    ),
  ];
}

// ---- What to download -------------------------------------------------------

function presetSelect(value, withNone) {
  const options = state.session.presets.map((p) => h("option", { value: p.name, text: p.label, selected: p.name === value }));
  return h("select", { class: "input" }, withNone ? h("option", { value: "", text: withNone }) : null, options);
}

function downloadStep() {
  const s = state.session;
  const preset = presetSelect(s.preset || "", "Everything except blocked file types");
  const rules = h("div", { class: "setup-rules" }, h("p", { class: "muted", text: "Loading your Put.io folders…" }));
  const rows = [];
  call("GET", "/setup/putio/folders")
    .then((folders) => {
      if (!folders.length) return rules.replaceChildren(h("p", { class: "muted", text: "No folders at the top of your Put.io account." }));
      const saved = Object.fromEntries((s.folder_rules || []).map((r) => [r.path, r]));
      rules.replaceChildren(
        ...folders.map((f) => {
          const r = saved[f.path] || {};
          const sel = presetSelect(r.skip ? "__skip" : r.preset || "", "As above");
          sel.append(h("option", { value: "__skip", text: "Don't download", selected: !!r.skip }));
          rows.push({ path: f.path, sel });
          return h("label", { class: "field setup-rule" }, h("span", { text: f.name }), sel, h("small", { text: bytes(f.size) }));
        }),
      );
    })
    .catch((e) => rules.replaceChildren(h("p", { class: "error-text", text: e.error })));
  return [
    h("h2", { text: "What to download" }),
    form(
      [
        field("For the whole account", preset),
        h("h3", { class: "sub", text: "Top-level folders" }),
        rules,
        nav({}),
      ],
      async () => {
        const folder_rules = rows
          .filter((r) => r.sel.value)
          .map((r) => (r.sel.value === "__skip" ? { path: r.path, skip: true } : { path: r.path, preset: r.sel.value }));
        await call("PUT", "/setup/download", { preset: preset.value || null, folder_rules });
        s.preset = preset.value || null;
        s.folder_rules = folder_rules;
        show(3);
      },
    ),
  ];
}

// ---- Extras -----------------------------------------------------------------

function extrasStep() {
  const s = state.session;
  const watchOn = h("input", { type: "checkbox", checked: !!s.watch_dir });
  const watch = h("input", { class: "input", type: "text", value: s.watch_dir || s.suggestions.watch_dir || "", spellcheck: "false" });
  const metrics = h("input", { type: "checkbox", checked: !!s.metrics });
  const arr = h("div", {});
  arrSection(arr);
  return [
    h("h2", { text: "Extras" }),
    h("p", { class: "muted", text: "All optional; you can change them later in Settings." }),
    form(
      [
        h("label", { class: "field check" }, watchOn, h("span", {}, "Watch a folder for .torrent and .magnet files", h("small", { text: "They are sent to Put.io." }))),
        field("Watch folder", watch),
        h("label", { class: "field check" }, metrics, h("span", {}, "Prometheus metrics", h("small", { text: `On ${s.metrics_bind}.` }))),
        h("h3", { class: "sub", text: "Radarr and Sonarr" }),
        arr,
        nav({}),
      ],
      async () => {
        await call("PUT", "/setup/extras", { watch_dir: watchOn.checked ? watch.value.trim() : null, metrics: metrics.checked });
        s.watch_dir = watchOn.checked ? watch.value.trim() : null;
        s.metrics = metrics.checked;
        show(4);
      },
    ),
  ];
}

function arrSection(box) {
  const s = state.session;
  const url = h("input", { class: "input", type: "text", placeholder: "http://radarr:7878", spellcheck: "false" });
  const key = h("input", { class: "input", type: "password", autocomplete: "off", spellcheck: "false" });
  const out = h("div", { role: "status" });
  const btn = (text, fn) => h("button", { type: "button", class: "btn btn-small", text, onclick: fn });
  const req = () => ({ target: { url: url.value.trim() }, api_key: key.value.trim(), remember: true });
  const run = async (fn) => {
    out.replaceChildren(h("p", { class: "muted", text: "Working…" }));
    try {
      await fn();
    } catch (e) {
      out.replaceChildren(h("p", { class: "error-text", text: e.error }));
    }
  };
  box.replaceChildren(
    h("p", { class: "muted" }, s.arr_enabled ? "The download client is on." : "Point Radarr or Sonarr at putio-sync as their download client."),
    btn("Look for them", () =>
      run(async () => {
        const found = await call("POST", "/setup/arr/discover");
        out.replaceChildren(
          found.instances.length
            ? h("ul", {}, found.instances.map((i) => h("li", {}, h("button", { type: "button", class: "btn btn-quiet btn-small", text: `${i.kind} at ${i.url}`, onclick: () => (url.value = i.url) }))))
            : h("p", { class: "muted", text: "None found; enter the address." }),
        );
      }),
    ),
    h("div", { class: "fields" }, field("Address", url), field("API key", key, "Radarr/Sonarr → Settings → General.")),
    h(
      "div",
      { class: "row-actions" },
      btn("Preview", () =>
        run(async () => {
          const p = await call("POST", "/setup/arr/preview", req());
          out.replaceChildren(h("p", { class: p.ok ? "notice" : "error-text", text: p.ok ? `Ready: ${p.name} ${p.version} will get the download client.` : p.detail || p.reason }));
        }),
      ),
      btn("Set up", () =>
        run(async () => {
          const r = await call("POST", "/setup/arr/run", req());
          const ok = r.outcome !== "failed";
          if (ok) s.arr_enabled = true;
          out.replaceChildren(h("p", { class: ok ? "notice" : "error-text", text: ok ? "Set up." : r.detail || r.reason }));
        }),
      ),
    ),
    out,
  );
}

// ---- Review and finish -------------------------------------------------------

function reviewStep() {
  const s = state.session;
  const presetName = (name) => (s.presets.find((p) => p.name === name) || {}).label || name;
  const report = h("div", { role: "status" });
  const dryRun = h("button", {
    type: "button",
    class: "btn",
    text: "Dry run",
    onclick: async () => {
      dryRun.disabled = true;
      report.replaceChildren(h("p", { class: "muted", text: "Listing your Put.io files (up to a minute)…" }));
      try {
        const r = await call("POST", "/setup/dry-run");
        fill(
          report,
          h("p", {}, `${r.to_download.count} files, ${bytes(r.to_download.bytes)}, would be downloaded. ${r.already_present.count} are already here; ${r.skipped.count} are skipped by the rules.`),
          r.partial ? h("p", { class: "warn-text", text: "Stopped after a minute; the counts cover what was listed." }) : null,
          r.samples.length ? h("ul", { class: "setup-samples" }, r.samples.map((x) => h("li", {}, h("span", { text: x.path }), h("small", { text: bytes(x.size) })))) : null,
        );
      } catch (e) {
        report.replaceChildren(h("p", { class: "error-text", text: e.error }));
      }
      dryRun.disabled = false;
    },
  });
  return [
    h("h2", { text: "Review" }),
    h(
      "dl",
      { class: "setup-review" },
      h("dt", { text: "Put.io" }),
      h("dd", { text: s.account ? s.account.username : "Not connected" }),
      h("dt", { text: "Download folder" }),
      h("dd", { text: s.download_dir || "Not chosen" }),
      h("dt", { text: "What to download" }),
      h("dd", { text: s.preset ? presetName(s.preset) : "Everything except blocked file types" }),
      h("dt", { text: "Folder rules" }),
      h("dd", { text: (s.folder_rules || []).map((r) => `${r.path}: ${r.skip ? "skip" : presetName(r.preset)}`).join(", ") || "None" }),
      h("dt", { text: "Watch folder" }),
      h("dd", { text: s.watch_dir || "Off" }),
      h("dt", { text: "Metrics" }),
      h("dd", { text: s.metrics ? "On" : "Off" }),
    ),
    h("p", { class: "muted", text: "Nothing is downloaded until you start. A dry run shows what the first sync would fetch." }),
    dryRun,
    report,
    form(nav({ next: "Save and start syncing" }), finish),
  ];
}

async function finish(err) {
  let res;
  try {
    res = await call("POST", "/setup/finish");
  } catch (e) {
    if (e.body && e.body.missing) {
      err.textContent = `Still to choose: ${e.body.missing.join(", ")}.`;
      return;
    }
    throw e;
  }
  stored("sessionStorage", CODE_KEY, null);
  if (res && res.api_token) stored("localStorage", TOKEN_KEY, res.api_token);
  render(
    h("h1", { text: "Starting…" }),
    res && res.api_token
      ? [
          h("p", {}, "This is your API token. It signs you in to this page and the API; this browser keeps it."),
          h("p", { class: "setup-code", text: res.api_token }),
          h("p", { class: "muted", text: "It is saved in the config file as [api] auth_token." }),
        ]
      : h("p", { class: "muted", text: "Sign in with your API token once the daemon has started." }),
    h("p", { class: "muted", role: "status", text: "Waiting for the daemon to start…" }),
  );
  waitForDaemon(res && res.api_token);
}

function waitForDaemon(withToken) {
  setTimeout(async () => {
    try {
      const res = await fetch("/auth/status");
      const s = await res.json();
      if (!s.setup_required) {
        window.location.hash = withToken ? "#/status" : "#/login";
        window.location.reload();
        return;
      }
    } catch {
      // restarting: try again
    }
    waitForDaemon(withToken);
  }, 1500);
}
