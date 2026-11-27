// First-run setup wizard (feature 013). The shell mounts it instead of the dashboard
// while /auth/status says setup_required. Every /setup/* call carries the setup code
// (kept in sessionStorage) or an API token; the Put.io token never comes back.
//
// Feature 056: the setup link carries the code in its fragment (`/setup#code=…`), so it is
// never sent to the server in a URL. The quick path asks the language, links Put.io, asks
// how putio-sync is used and shows what that sets up; "I'll set it up myself" keeps the
// step-by-step wizard.

import { arrStatus } from "../api.js";
import { docs, fill, h, toast } from "../app.js";
import { guideFor } from "../guides.js";
import { bytes } from "../format.js";
import { LANGUAGES, choice, nativeName, setLanguage } from "../i18n.js";
import { afterStartApps, codeFromHash, startAnswers } from "../profile.js";
import { reviewList, usageForm } from "../profile_view.js";
import { arrChecks } from "../settings/arr.js";

const CODE_KEY = "putio-sync.setup-code";
const TOKEN_KEY = "putio-sync.token";

// The steps of each path: [name, screen]
const FLOWS = {
  quick: () => [["Language", languageStep], ["Put.io", putioStep], ["How you use it", usageStep], ...(state.needFolder ? [["Download folder", folderStep]] : []), ["Review", profileReviewStep]],
  manual: () => [["Language", languageStep], ["Put.io", putioStep], ["Download folder", folderStep], ["What to download", downloadStep], ["Extras", extrasStep], ["Review", reviewStep]],
};
const flow = () => FLOWS[state.path]();

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
  state = { step: -1, code: stored("sessionStorage", CODE_KEY), session: null, busy: false, path: "quick", needFolder: false, detect: null, profile: null, overrides: {}, review: null, arr: {}, arrSetUp: [], linked: false };
  // The code from the setup link; the fragment is dropped so it isn't kept in history
  const linked = codeFromHash(window.location.hash);
  if (linked) {
    state.code = linked;
    state.linked = true;
    try {
      window.history.replaceState(null, "", "/");
    } catch {
      // the code stays in the address bar until the next page
    }
  }
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
      stored("sessionStorage", CODE_KEY, state.code);
      await refresh();
      return show(0);
    } catch {
      stored("sessionStorage", CODE_KEY, null);
      state.code = null;
    }
  }
  render(unlockStep(state.linked));
}

async function refresh() {
  state.session = await call("GET", "/setup/session");
}

/** The language switcher at the top of every screen */
function languagePicker(id = "setup-language") {
  const select = h(
    "select",
    { id, class: "input", "aria-label": "Language", onchange: async () => (await setLanguage(select.value)) || toast("Couldn't load that language", "error", "The page stays in English.") },
    h("option", { value: "", text: "Automatic" }),
    ["en", ...LANGUAGES].map((code) => h("option", { value: code, translate: "no", lang: code, text: nativeName(code) })),
  );
  select.value = choice();
  return select;
}

function render(...children) {
  if (!root) return;
  const top = h("div", { class: "setup-language" }, languagePicker());
  root.replaceChildren(h("section", { class: "card setup" }, top, ...children));
  // The language screen has its own picker
  if (root.querySelector("#setup-language-main")) top.remove();
  const first = root.querySelector("input, select, button.btn-primary");
  if (first) first.focus();
}

function progress() {
  return h(
    "ol",
    { class: "setup-steps" },
    flow().map(([name], i) =>
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
  render(progress(), flow()[step][1]());
}

// ---- Unlock -----------------------------------------------------------------

/** The first screen: the language, before anything else */
function languageStep() {
  return [
    h("h1", { text: "Set up putio-sync" }),
    h("p", { class: "muted", text: "Choose the language for this page. Subtitles in it are suggested too." }),
    form([field("Language", languagePicker("setup-language-main")), nav({})], async () => show(state.step + 1)),
  ];
}

function unlockStep(linkFailed = false) {
  const input = h("input", { class: "input", type: "password", autocomplete: "one-time-code", autocapitalize: "characters", spellcheck: "false" });
  return [
    h("h1", { text: "Set up putio-sync" }),
    h("p", { class: "muted small" }, docs("guides/index", "", "Setup guide")),
    linkFailed ? h("p", { class: "error-text", role: "alert", text: "The code in that link didn't work. The daemon makes a new one each time it starts: use the newest link in its log." }) : null,
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
    show(state.step);
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
    // setup mode has no /update; the session says how this was installed
    h("p", { class: "muted small" }, docs(guideFor(s.install_kind), "", "Setup guide")),
    s.account ? account(s.account) : null,
    s.app_id_available ? [h("p", { class: "muted" }, "Link this install to your Put.io account without copying a token."), link, box] : null,
    s.app_id_available ? h("h3", { class: "sub", text: "Or paste a token" }) : appIdForm(s),
    paste,
    err,
    form(nav({ next: "Next" }), async (e) => {
      if (!state.session.account) {
        e.textContent = "Connect Put.io first.";
        return;
      }
      show(state.step + 1);
    }),
  ];
}

/** No app ID yet: enter one to turn on the device link, or paste a token below */
function appIdForm(s) {
  const input = h("input", { class: "input", inputmode: "numeric", autocomplete: "off", spellcheck: "false", placeholder: "e.g. 1234" });
  const register = h("a", { href: s.app_register_url, target: "_blank", rel: "noopener", text: "Register an app" });
  return [
    h("p", { class: "muted" }, "Linking with a code at put.io/link needs a Put.io app ID. Register one at ", h("a", { href: s.app_register_url, target: "_blank", rel: "noopener", text: s.app_register_url }), " (account menu → API → Create App; any website and callback URL will do) and enter its ID, or paste a token below."),
    form([field("Put.io app ID", input, "The number Put.io shows for the app."), h("p", {}, register), h("button", { type: "submit", class: "btn", text: "Use app ID" })], async (err) => {
      const id = input.value.trim();
      if (!/^\d{1,12}$/.test(id)) {
        err.textContent = "Enter the app ID Put.io shows for the app: a number.";
        return;
      }
      await call("PUT", "/setup/putio/app-id", { app_id: id });
      s.app_id_available = true;
      show(state.step);
    }),
    h("h3", { class: "sub", text: "Or paste a token" }),
  ];
}

function waitForLink(box, err) {
  linkTimer = setTimeout(async () => {
    if (!root || flow()[state.step][1] !== putioStep) return;
    try {
      const res = await call("GET", "/setup/putio/link");
      if (res.state === "linked") {
        state.session.account = res.account;
        toast("Put.io linked");
        return show(state.step);
      }
      if (res.state === "expired") {
        box.replaceChildren(h("p", { class: "error-text", text: "The code expired. Link again." }));
        return show(state.step);
      }
    } catch (e) {
      fail(err, e);
    }
    waitForLink(box, err);
  }, 3000);
}

// ---- How you use it (056) ------------------------------------------------------

/** The suggested download folder, unless one is chosen; the folder screen only if it fails */
async function ensureFolder() {
  const s = state.session;
  if (s.download_dir) return;
  const path = s.suggestions.download_dir;
  if (!path) {
    state.needFolder = true;
    return;
  }
  try {
    const c = await call("PUT", "/setup/folder", { path, create: true });
    s.download_dir = c.path;
    state.needFolder = false;
  } catch {
    state.needFolder = true;
  }
}

const ARR_NAME = { radarr: "Radarr", sonarr: "Sonarr" };

/** Address and API key for each ticked *arr; they are set up when syncing starts */
function arrFields(box, kinds) {
  if (!kinds.length) return box.replaceChildren();
  const rows = kinds.map((kind) => {
    const saved = (state.arr[kind] ||= { url: "", key: "", done: false });
    const url = h("input", { class: "input", type: "text", value: saved.url, placeholder: kind === "radarr" ? "http://radarr:7878" : "http://sonarr:8989", spellcheck: "false", oninput: () => ((saved.url = url.value.trim()), (saved.done = false)) });
    const key = h("input", { class: "input", type: "password", value: saved.key, autocomplete: "off", spellcheck: "false", oninput: () => ((saved.key = key.value.trim()), (saved.done = false)) });
    return h("fieldset", { class: "fields" }, h("legend", { text: ARR_NAME[kind] }), field("Address", url), field("API key", key, "Radarr/Sonarr → Settings → General."));
  });
  const look = h("button", {
    type: "button",
    class: "btn btn-small",
    text: "Look for them",
    onclick: async () => {
      look.disabled = true;
      try {
        const found = await call("POST", "/setup/arr/discover");
        for (const i of found.instances || []) {
          const a = state.arr[i.kind];
          if (a && !a.url) a.url = i.url;
        }
        if (!(found.instances || []).length) toast("None found; enter the address.");
        return arrFields(box, kinds);
      } catch (e) {
        toast(e.error, "error");
      }
      look.disabled = false;
    },
  });
  fill(
    box,
    h("h3", { class: "sub", text: "Connect Radarr and Sonarr" }),
    h("p", { class: "muted", text: "Optional. With an API key, putio-sync becomes their download client when you start; or do it later in Settings › Integrations." }),
    rows,
    look,
  );
}

function usageStep() {
  const s = state.session;
  const box = h("div", {}, h("p", { class: "muted", text: "Looking at your Put.io folders…" }));
  const arrBox = h("div", {});
  let questions = null;
  const ask = (d, warning) => {
    questions = usageForm({ start: state.profile || startAnswers(null, d.found, d.watch_dir || s.suggestions.watch_dir), found: d.found || {}, onArr: (kinds) => arrFields(arrBox, kinds) });
    fill(box, warning ? h("p", { class: "warn-text", text: warning }) : null, questions.node);
  };
  (state.detect ? Promise.resolve(state.detect) : call("GET", "/setup/profile/detect").then((d) => (state.detect = d))).then(
    (d) => root && ask(d),
    (e) => root && ask({ found: {} }, e.error),
  );
  return [
    h("h2", { text: "How will you use putio-sync?" }),
    h("p", { class: "muted", text: "Your answers pick the settings. The next screen shows each one; change any of them there or later in Settings." }),
    form([box, arrBox, nav({})], async (err) => {
      if (!questions) return;
      const answers = questions.read();
      if (!answers.kinds.length) {
        err.textContent = "Pick at least one kind of download.";
        return;
      }
      state.profile = answers;
      state.overrides = {};
      await ensureFolder();
      state.review = await call("POST", "/setup/profile", { answers, overrides: {} });
      show(state.step + 1);
    }),
    h(
      "p",
      { class: "setup-manual" },
      h("button", {
        type: "button",
        class: "btn btn-quiet",
        text: "I'll set it up myself",
        onclick: async () => {
          try {
            await call("POST", "/setup/profile", { answers: null });
          } catch (e) {
            return toast(e.error, "error");
          }
          state.path = "manual";
          state.review = null;
          show(2);
        },
      }),
    ),
  ];
}

function profileReviewStep() {
  const s = state.session;
  const list = h("div", { role: "status" });
  let timer = 0;
  const onChange = (id, override) => {
    state.overrides[id] = override;
    clearTimeout(timer);
    timer = setTimeout(async () => {
      try {
        state.review = await call("POST", "/setup/profile", { answers: state.profile, overrides: state.overrides });
        if (root) draw();
      } catch (e) {
        toast(e.error, "error");
      }
    }, 250);
  };
  const draw = () => fill(list, reviewList(state.review, { presets: s.presets, onChange }));
  draw();
  const ready = (state.profile.arr || []).filter((k) => state.arr[k] && state.arr[k].url && state.arr[k].key);
  const arr = ready.map((k) => ARR_NAME[k]);
  // Each app with a key is checked now, so a wrong key shows before Start syncing
  const arrCheck = h("div", { role: "status" });
  for (const kind of ready) {
    const a = state.arr[kind];
    const line = h("p", { class: "muted", text: `Checking ${ARR_NAME[kind]}…` });
    arrCheck.append(line);
    call("POST", "/setup/arr/preview", { target: { url: a.url }, api_key: a.key, remember: true }).then(
      (p) => ((line.className = p.ok ? "notice" : "error-text"), (line.textContent = p.ok ? `Ready: ${p.name} ${p.version} will get the download client.` : `${ARR_NAME[kind]}: ${p.detail || p.reason}`)),
      (e) => ((line.className = "error-text"), (line.textContent = `${ARR_NAME[kind]}: ${e.error}`)),
    );
  }
  return [
    h("h2", { text: "Review" }),
    h(
      "dl",
      { class: "setup-review" },
      h("dt", { text: "Put.io" }),
      h("dd", { text: s.account ? s.account.username : "Not connected" }),
      h("dt", { text: "Download folder" }),
      h("dd", { translate: "no", text: s.download_dir || "Not chosen" }),
      (state.profile.arr || []).length ? [h("dt", { text: "Radarr and Sonarr" }), h("dd", { text: arr.join(", ") || "Set up later in Settings" })] : null,
    ),
    arrCheck,
    h("h3", { class: "sub", text: "What your answers set up" }),
    list,
    h("p", { class: "muted", text: "Nothing is downloaded until you start. A dry run shows what the first sync would fetch." }),
    dryRunBox(),
    form(nav({ next: "Start syncing" }), startSyncing),
  ];
}

/** Sets up the *arr apps that have a key, then saves and starts */
async function startSyncing(err) {
  for (const kind of state.profile.arr || []) {
    const a = state.arr[kind];
    if (!a || !a.url || !a.key || a.done) continue;
    let r;
    try {
      r = await call("POST", "/setup/arr/run", { target: { url: a.url }, api_key: a.key, remember: true });
    } catch (e) {
      r = { outcome: "failed", detail: e.error };
    }
    if (r.outcome === "failed") {
      err.textContent = `${ARR_NAME[kind]}: ${r.detail || r.reason}`;
      return;
    }
    a.done = true;
    state.arrSetUp.push({ url: r.url || a.url, host: r.host || null });
    state.session.arr_enabled = true;
  }
  await finish(err);
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
        if (state.path === "quick") state.review = await call("POST", "/setup/profile", { answers: state.profile, overrides: state.overrides });
        show(state.step + 1);
      },
    ),
    state.path === "manual" ? h("p", { class: "setup-manual" }, h("button", { type: "button", class: "btn btn-quiet", text: "Use the quick setup instead", onclick: () => ((state.path = "quick"), show(2)) })) : null,
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
        show(state.step + 1);
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
        show(state.step + 1);
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
          if (ok) {
            s.arr_enabled = true;
            state.arrSetUp.push({ url: r.url || req().target.url, host: r.host || null });
          }
          out.replaceChildren(h("p", { class: ok ? "notice" : "error-text", text: ok ? "Set up." : r.detail || r.reason }));
        }),
      ),
    ),
    out,
  );
}

// ---- Review and finish -------------------------------------------------------

/** The dry run button and its report */
function dryRunBox() {
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
  return [dryRun, report];
}

function reviewStep() {
  const s = state.session;
  const presetName = (name) => (s.presets.find((p) => p.name === name) || {}).label || name;
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
    dryRunBox(),
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
    res && res.api_token ? tokenLines(res.api_token) : h("p", { class: "muted", text: "Sign in with your API token once the daemon has started." }),
    h("p", { class: "muted", role: "status", text: "Waiting for the daemon to start…" }),
  );
  waitForDaemon(res && res.api_token);
}

/** The new API token, shown until the wizard leaves */
function tokenLines(token) {
  return [
    h("p", {}, "This is your API token. It signs you in to this page and the API; this browser keeps it."),
    h("p", { class: "setup-code", text: token }),
    h("p", { class: "muted", text: "It is saved in the config file as [api] auth_token." }),
  ];
}

function leave(withToken) {
  window.location.hash = withToken ? "#/status" : "#/login";
  window.location.reload();
}

/**
 * After start: each Radarr / Sonarr the wizard set up, with its folder mapping and the
 * recommended settings to fix or turn on (049). Nothing changes in the app without a click.
 */
async function afterStart(token) {
  const done = h("button", { type: "button", class: "btn btn-primary", onclick: () => leave(true) }, "Continue");
  const body = h("div", { role: "status" }, h("p", { class: "muted", text: "Loading…" }));
  render(
    h("h1", { text: "Radarr and Sonarr" }),
    tokenLines(token),
    h("p", { class: "muted", text: "putio-sync has started. Check each app's download folder and settings below; nothing changes in the app until you press its button." }),
    body,
    h("div", { class: "form-foot" }, done),
  );
  let apps = [];
  try {
    apps = afterStartApps((await arrStatus()).applications, state.arrSetUp);
  } catch (e) {
    if (e.status === 401 || e.status === 429) return leave(true);
    return body.replaceChildren(h("p", { class: "error-text", text: e.error || String(e) }), h("p", { class: "muted", text: "Run Set up for each app in Settings › Radarr / Sonarr later." }));
  }
  if (!apps.length) return leave(true);
  body.replaceChildren(
    ...apps.map((a) => h("section", { class: "setup-arr-check" }, h("h3", { class: "sub", text: a.name }), arrChecks({ target: { application: a.index }, ...(a.host ? { host: a.host } : {}) }, a.kind === "sonarr" ? "Sonarr" : "Radarr"))),
  );
  done.focus();
}

function waitForDaemon(withToken) {
  setTimeout(async () => {
    try {
      const res = await fetch("/auth/status");
      const s = await res.json();
      if (!s.setup_required) {
        // The folder mapping and recommendations need the running daemon (T019)
        if (withToken && state.arrSetUp.length) return afterStart(withToken);
        return leave(withToken);
      }
    } catch {
      // restarting: try again
    }
    waitForDaemon(withToken);
  }, 1500);
}
