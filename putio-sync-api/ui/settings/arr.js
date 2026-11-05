// Radarr / Sonarr card (features 011, 018, 049): the download client form, auto-setup with
// its folder and recommendation steps, and the Applications list with Add, Edit, Delete and Test.

import {
  addArrApplication,
  applyArrMapping,
  applyArrRecommendation,
  arrStatus,
  checkArrSetup,
  deleteArrApplication,
  discoverArr,
  previewArrSetup,
  runArrSetup,
  saveArrApplication,
  saveArrClient,
  testArrApplication,
  testArrForm,
} from "../api.js";
import { confirm, fill, h, toast } from "../app.js";
import { relative } from "../format.js";
import { reason } from "./form.js";

// FR-026 reasons (plus the server test's no_api_key): what happened, then what to check (US8)
const ARR_HELP = {
  unreachable: ["Unreachable", "Check the address and port, and that the app is running."],
  timed_out: ["Unreachable (timed out)", "The app didn't answer in time. Check it isn't stuck or behind a firewall."],
  untrusted_certificate: ["Untrusted HTTPS certificate", "Use the http:// address, or give the app a certificate this machine trusts."],
  unauthorized: ["API key refused", "Copy the key again from Settings → General in Radarr or Sonarr."],
  not_arr: ["Not a Radarr or Sonarr server", "Check the port: Radarr is usually 7878 and Sonarr 8989. Add the URL base if it has one."],
  unsupported_version: ["Version 3 or later is needed", "Update Radarr or Sonarr, then try again."],
  invalid_category: ["Invalid category", "Use a folder name without .. or backslashes, or leave it empty."],
  arr_cannot_reach: ["Radarr/Sonarr can't reach putio-sync", "Enter the address Radarr/Sonarr can reach putio-sync at in “putio-sync host”, then try again."],
  arr_test_failed: ["The download client test failed", "The app's own message is above. Nothing was saved."],
  arr_refused: ["Radarr/Sonarr refused the download client", "The app's own message is above. Nothing was left behind."],
  config_write_failed: ["Could not write the config file", "Check the config file isn't read-only. Nothing was changed."],
  no_api_key: ["No API key", "Edit the application and enter its API key."],
  no_client: ["No putio-sync download client", "Set up the download client first."],
  marker_failed: ["Could not write to the download folder", "Check putio-sync can write to its download folder."],
};
const arrReason = (r) => (ARR_HELP[r] || [r])[0];
const arrNext = (r) => (ARR_HELP[r] || [])[1];
const FOUND_BY = { saved: "saved", container: "container", host: "this host", browser: "browser address" };

export function arrCard(ctx) {
  const card = h("section", { class: "card", id: "card-arr" });
  const title = () => h("h2", { text: "Radarr / Sonarr" });
  const load = async () => {
    card.replaceChildren(title(), h("p", { class: "muted", text: "Loading…" }));
    try {
      const data = await arrStatus();
      if (!ctx.alive()) return;
      ctx.report("arr", { problem: false });
      // replaceChildren prints null as text, unlike h(), so drop the optional parts
      card.replaceChildren(title(), ...arrBody(data, load, ctx).filter(Boolean));
    } catch (e) {
      if (!ctx.alive() || e.status === 401 || e.status === 429) return;
      ctx.report("arr", { problem: true });
      card.replaceChildren(
        title(),
        h("p", { class: "error-text", text: reason(e) }),
        h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"),
      );
    }
  };
  load();
  return card;
}

const kv = (pairs) => h("dl", { class: "kv" }, pairs.flatMap(([k, v]) => [h("dt", { text: k }), h("dd", {}, v)]));

function arrBody(data, reload, ctx) {
  const c = data.client;
  const connection = kv([
    ["Type", "Transmission"],
    ["Host", h("span", { class: "name", text: c.connection.hosts.join(" or ") })],
    ["Port", String(c.connection.port)],
    ["Use SSL", c.connection.ssl ? "Yes" : "No"],
    ["URL base", c.connection.url_base],
    ["Username", c.username],
    ["Download folder", h("span", { class: "name", text: c.download_dir })],
  ]);
  return [
    c.available ? null : h("p", { class: "warn-text", text: "The download client needs the database, which isn't open." }),
    h("h3", { class: "sub", text: "Download client" }),
    clientForm(c, reload),
    h("h3", { class: "sub", text: "Connection" }),
    h("p", { class: "muted", text: "Set up automatically below, or add a Transmission download client in Radarr or Sonarr with these details. Use the first host that works." }),
    connection,
    h(
      "p",
      { class: "muted" },
      "Radarr and Sonarr import from the download folder, so they must see it: at the same path, or through a Remote Path Mapping. ",
      h("a", { href: PATH_HELP, target: "_blank", rel: "noopener", text: "How to map paths" }),
    ),
    h("h3", { class: "sub", text: "Set up automatically" }),
    setupView(reload, ctx),
    h("h3", { class: "sub", text: "Applications" }),
    applications(data.applications, reload),
  ];
}

/** 32 characters from [A-Za-z0-9], from the browser's CSPRNG */
function generatePassword() {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  const bytes = crypto.getRandomValues(new Uint8Array(64));
  let out = "";
  // Reject bytes past the last whole multiple of 62, so every character is equally likely
  for (const b of bytes) {
    if (b < 248 && out.length < 32) out += chars[b % 62];
  }
  return out.length === 32 ? out : generatePassword();
}

function clientForm(c, reload) {
  const envPassword = c.env && c.env.password;
  const enabled = h("input", { type: "checkbox", id: "arr-enabled", checked: Boolean(c.enabled) });
  const username = h("input", { class: "input", id: "arr-username", type: "text", required: true, autocomplete: "off", spellcheck: "false" });
  username.value = c.username;
  const password = h("input", {
    class: "input",
    id: "arr-password",
    type: "password",
    autocomplete: "new-password",
    disabled: envPassword,
    placeholder: envPassword ? "set by environment" : c.password_set ? "set; leave empty to keep" : "not set",
  });
  const copy = h("button", { type: "button", class: "btn", hidden: true }, "Copy");
  copy.onclick = async () => {
    try {
      await navigator.clipboard.writeText(password.value);
      toast("Password copied");
    } catch {
      password.select();
    }
  };
  const generate = h("button", { type: "button", class: "btn", disabled: envPassword }, "Generate");
  generate.onclick = () => {
    password.value = generatePassword();
    // Shown until the form is saved or left, so it can be copied
    password.type = "text";
    copy.hidden = false;
  };
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const note = h("p", { class: "muted", role: "status" });
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");

  const onSubmit = async (ev) => {
    ev.preventDefault();
    save.disabled = true;
    error.hidden = true;
    note.textContent = "";
    try {
      const res = await saveArrClient({ enabled: enabled.checked, username: username.value.trim(), password: envPassword ? "" : password.value });
      toast(res.message || "Saved");
      // Shown in the Applications list once the card reloads (FR-008)
      if (res.password_changed && c.password_set) passwordOffer = true;
      await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      error.textContent = reason(e);
      error.hidden = false;
      if (save.isConnected) save.disabled = false;
    }
  };

  return h(
    "form",
    { onsubmit: onSubmit, oninput: () => (note.textContent = "") },
    h(
      "div",
      { class: "fields" },
      h("label", { class: "field check", for: "arr-enabled" }, enabled, h("span", {}, "On", h("small", { text: "Answer Radarr and Sonarr as a Transmission client" }))),
      h("label", { class: "field", for: "arr-username" }, h("span", { text: "Username" }), username),
      h(
        "label",
        { class: "field", for: "arr-password" },
        h("span", { text: "Password" }),
        password,
        h("small", { text: envPassword ? "Set by environment (PUTIO_SYNC_ARR_PASSWORD)" : "Never shown once saved" }),
      ),
    ),
    h("div", { class: "form-foot" }, generate, copy),
    error,
    h("div", { class: "form-foot" }, save, note),
  );
}

let setupSeq = 0;

/** Discovery, then preview, then setup (US1). A late answer for a closed view is dropped. */
function setupView(reload, ctx) {
  const wrap = h("div", { class: "stack" });
  const open = h("button", { type: "button", class: "btn" }, "Set up automatically");
  const live = (view) => ctx.alive() && view === setupSeq && wrap.isConnected;

  const close = () => {
    setupSeq += 1;
    wrap.replaceChildren(open);
  };
  open.onclick = () => {
    const view = ++setupSeq;
    const found = h("div", { class: "stack" }, h("p", { class: "muted", text: "Looking for Radarr and Sonarr…" }));
    const url = h("input", { class: "input", id: "arr-setup-url", type: "text", placeholder: "http://radarr:7878", autocomplete: "off", spellcheck: "false" });
    const key = h("input", { class: "input", id: "arr-setup-key", type: "password", autocomplete: "off" });
    const category = h("input", { class: "input", id: "arr-setup-category", type: "text", placeholder: "radarr or sonarr", autocomplete: "off", spellcheck: "false" });
    const host = h("input", { class: "input", id: "arr-setup-host", type: "text", placeholder: "found automatically", autocomplete: "off", spellcheck: "false" });
    const remember = h("input", { type: "checkbox", id: "arr-setup-remember" });
    // Unticked by default: the operator may still use the blackhole (US5)
    const blackhole = h("input", { type: "checkbox", id: "arr-setup-blackhole" });
    const out = h("div", { role: "status" });
    const previewBtn = h("button", { type: "button", class: "btn" }, "Preview");
    const runBtn = h("button", { type: "submit", class: "btn btn-primary" }, "Set up");
    const cancel = h("button", { type: "button", class: "btn", onclick: close }, "Close");

    const req = () => ({
      target: { url: url.value.trim() },
      api_key: key.value.trim(),
      category: category.value.trim() || null,
      host: host.value.trim() || null,
      remember: remember.checked,
      disable_blackhole: blackhole.checked,
    });
    const busy = (on) => {
      previewBtn.disabled = on;
      runBtn.disabled = on;
    };
    const failure = (r) => {
      // The host override is the fix for this one: take the operator there
      if (r.reason === "arr_cannot_reach") {
        host.focus();
        host.select();
      }
      return h(
        "div",
        {},
        h("p", { class: "error-text", text: `${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}` }),
        r.hosts_tried && r.hosts_tried.length ? h("p", { class: "muted", text: `Hosts tried: ${r.hosts_tried.join(", ")}` }) : null,
        arrNext(r.reason) ? h("p", { class: "muted", text: arrNext(r.reason) }) : null,
      );
    };
    const fail = (e) => {
      if (e.status === 401 || e.status === 429) return;
      out.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    };

    previewBtn.onclick = async () => {
      busy(true);
      out.replaceChildren(h("p", { class: "muted", text: "Checking…" }));
      try {
        const plan = await previewArrSetup(req());
        if (!live(view)) return;
        if (!plan.ok) {
          out.replaceChildren(failure(plan));
          return;
        }
        if (!category.value.trim()) category.placeholder = plan.category;
        const what = plan.action === "update" ? `Update ${plan.client_name}` : plan.action === "unchanged" ? "Already set up" : `Create ${plan.client_name}`;
        const notes = [];
        if (plan.download_client.will_enable) notes.push("The download client will be turned on.");
        if (plan.download_client.will_generate_password) notes.push("A password will be generated for it.");
        fill(
          out,
          kv([
            ["App", `${plan.name} ${plan.version}`],
            ["Address", h("span", { class: "name", text: plan.url })],
            ["Change", what],
            ["Category", plan.category],
          ]),
          existingClients(plan.existing_clients),
          plan.blackhole_enabled
            ? h("p", { class: "warn-text", text: "An enabled Torrent Blackhole may still take releases. Tick “Disable Torrent Blackhole” to turn it off." })
            : null,
          notes.length ? h("p", { class: "muted", text: notes.join(" ") }) : null,
        );
      } catch (e) {
        if (live(view)) fail(e);
      } finally {
        if (live(view)) busy(false);
      }
    };

    const onSubmit = async (ev) => {
      ev.preventDefault();
      busy(true);
      out.replaceChildren(h("p", { class: "muted", text: "Setting up… this can take up to 30 s." }));
      try {
        const r = await runArrSetup(req());
        if (!live(view)) return;
        if (r.outcome === "failed") {
          out.replaceChildren(failure(r));
          busy(false);
          return;
        }
        if (r.blackhole_disabled) toast("Torrent Blackhole disabled");
        if (r.warning) toast(r.warning);
        busy(false);
        guided({ target: { url: r.url }, api_key: key.value.trim(), host: r.host }, out, r, reload);
      } catch (e) {
        if (live(view)) {
          fail(e);
          busy(false);
        }
      }
    };

    wrap.replaceChildren(
      found,
      h(
        "form",
        { onsubmit: onSubmit },
        h(
          "div",
          { class: "fields" },
          h("label", { class: "field", for: "arr-setup-url" }, h("span", { text: "Address" }), url, h("small", { text: "Pick one above, or type it. A bare host like nas is tried on ports 7878 and 8989." })),
          h("label", { class: "field", for: "arr-setup-key" }, h("span", { text: "API key" }), key, h("small", { text: "Settings → General in Radarr or Sonarr" })),
          h("label", { class: "field", for: "arr-setup-category" }, h("span", { text: "Category" }), category),
          h("label", { class: "field", for: "arr-setup-host" }, h("span", { text: "putio-sync host" }), host, h("small", { text: "Only if the automatic choice fails" })),
          h(
            "label",
            { class: "field check", for: "arr-setup-blackhole" },
            blackhole,
            h("span", {}, "Disable Torrent Blackhole", h("small", { text: "Only after putio-sync's client is saved. It's turned off, not deleted." })),
          ),
          h("label", { class: "field check", for: "arr-setup-remember" }, remember, h("span", {}, "Remember", h("small", { text: "Save this app and its API key in the config file" }))),
        ),
        h("div", { class: "form-foot" }, previewBtn, runBtn, cancel),
        out,
      ),
    );

    discoverArr()
      .then((d) => {
        if (!live(view)) return;
        // Open when nothing was found, so the operator sees where it looked (US2)
        const tried = h("details", { open: !d.instances.length }, h("summary", { class: "muted", text: `Addresses tried (${d.tried.length})` }), h("p", { class: "muted name", text: d.tried.join(", ") }));
        if (!d.instances.length) {
          found.replaceChildren(h("p", { class: "muted", text: "No Radarr or Sonarr found. Type the address below." }), tried);
          return;
        }
        const pick = (i) =>
          h("button", { type: "button", class: "btn", onclick: () => ((url.value = i.url), key.focus()) }, `${i.kind === "radarr" ? "Radarr" : "Sonarr"} at ${i.url} (${FOUND_BY[i.found_by] || i.found_by})`);
        found.replaceChildren(h("div", { class: "form-foot" }, d.instances.map(pick)), tried);
      })
      .catch((e) => {
        if (!live(view) || e.status === 401 || e.status === 429) return;
        found.replaceChildren(h("p", { class: "muted", text: `Discovery didn't run: ${reason(e)}. Type the address below.` }));
      });
  };
  wrap.append(open);
  return wrap;
}

/** The *arr's download clients, putio-sync's own marked */
function existingClients(list) {
  if (!list.length) return h("p", { class: "muted", text: "No download clients there yet." });
  return h(
    "ul",
    { class: "stack" },
    list.map((c) =>
      h(
        "li",
        {},
        h("strong", { text: c.name || "(no name)" }),
        ` ${c.implementation}`,
        c.enabled ? null : h("span", { class: "muted", text: " (off)" }),
        c.points_at_putio_sync ? [" ", h("span", { class: "badge", text: "putio-sync" })] : null,
      ),
    ),
  );
}

/** Set after a password change: the saved applications still use the old one (FR-008) */
let passwordOffer = false;

const KIND_NAME = { radarr: "Radarr", sonarr: "Sonarr" };
const DEFAULT_PORT = { radarr: 7878, sonarr: 8989 };

/** A legacy typed category as the server compares it */
const normalCategory = (c) => c.trim().replace(/^\/+|\/+$/g, "").toLowerCase();

/** The Applications list, its Add buttons and the password update offer */
function applications(list, reload) {
  const wrap = h("div", { class: "stack" });
  const rows = list.map((s) => applicationRow(s, list, reload));
  const add = (kind) => {
    const form = applicationForm(kind, null, reload, () => form.replaceWith(addBar));
    addBar.replaceWith(form);
  };
  const addBar = h(
    "div",
    { class: "form-foot" },
    h("button", { type: "button", class: "btn", onclick: () => add("radarr") }, "Add Radarr"),
    h("button", { type: "button", class: "btn", onclick: () => add("sonarr") }, "Add Sonarr"),
  );
  if (passwordOffer) wrap.append(updateOffer(list, reload));
  wrap.append(
    rows.length ? h("div", { class: "stack" }, rows) : h("p", { class: "muted", text: "No applications saved. Add one, or set one up above with Remember ticked." }),
    addBar,
  );
  return wrap;
}

/** Run setup again for each saved application with a key, so it gets the new password */
function updateOffer(list, reload) {
  const withKey = list.filter((s) => s.api_key_set);
  const out = h("div", { role: "status" });
  const update = h("button", { type: "button", class: "btn btn-primary", disabled: !withKey.length }, `Update ${withKey.length === 1 ? withKey[0].name : `${withKey.length} applications`}`);
  const dismiss = h("button", { type: "button", class: "btn" }, "Not now");
  const box = h(
    "div",
    { class: "stack" },
    h("p", { class: "warn-text", text: "Radarr and Sonarr still use the old password until their putio-sync client is updated." }),
    h("div", { class: "form-foot" }, update, dismiss),
    out,
  );
  dismiss.onclick = () => {
    passwordOffer = false;
    box.remove();
  };
  update.onclick = async () => {
    update.disabled = true;
    dismiss.disabled = true;
    const lines = [];
    for (const s of withKey) {
      out.replaceChildren(h("p", { class: "muted", text: `Updating ${s.name}…` }), ...lines);
      try {
        const r = await runArrSetup({ target: { application: s.index } });
        lines.push(
          r.outcome === "failed"
            ? h("p", { class: "error-text", text: `${s.name}: ${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}` })
            : h("p", {}, h("span", { class: "dot ok" }), `${s.name}: updated`),
        );
      } catch (e) {
        if (e.status === 401 || e.status === 429) return;
        lines.push(h("p", { class: "error-text", text: `${s.name}: ${reason(e)}` }));
      }
    }
    out.replaceChildren(...lines);
    passwordOffer = false;
    if (lines.every((l) => l.className !== "error-text")) {
      toast("Applications updated");
      await reload();
    } else {
      dismiss.disabled = false;
      dismiss.textContent = "Close";
    }
  };
  return box;
}

const PATH_HELP = "https://github.com/devclinton/putio-sync#path-mapping";

/** Can the application see the download folder (R10); null before a check */
function pathLines(check, app) {
  if (!check) return [];
  if (check.status === "visible") {
    return [h("p", {}, h("span", { class: "dot ok" }), `Folder visible to ${app}`)];
  }
  if (check.status === "unknown") {
    return [h("p", { class: "muted", text: `Folder not checked${check.detail ? `: ${check.detail}` : ""}` })];
  }
  const mapped = check.arr_path !== check.folder ? ` (${check.folder} after its path mappings)` : "";
  const mappings = check.mappings.length
    ? `Its path mappings: ${check.mappings.map((m) => `${m.host}: ${m.remote_path} → ${m.local_path}`).join("; ")}.`
    : "It has no path mappings.";
  return [
    h("p", { class: "error-text", text: `${app} cannot see ${check.arr_path}${mapped}` }),
    h(
      "p",
      { class: "muted" },
      `Mount putio-sync's download folder in ${app} at ${check.folder}, or add a Remote Path Mapping in ${app} from ${check.folder} to where ${app} sees it. ${mappings} `,
      h("a", { href: PATH_HELP, target: "_blank", rel: "noopener", text: "How to map paths" }),
    ),
  ];
}

const MAP_VERB = { create: "Create mapping", update: "Change mapping", remove: "Remove mapping" };
const RECOMMEND = {
  completed_download_handling: "Completed Download Handling, so finished downloads are imported",
  import_extra_files: "Import Extra Files with srt, so subtitles come too",
};

/**
 * After a setup `r`: one line per step (client, folder, recommended settings), with the
 * mapping to confirm and the settings to turn on (049). Done reloads the card.
 */
function guided(base, box, r, reload) {
  const okLine = (t) => h("p", {}, h("span", { class: "dot ok" }), t);
  const bad = (f) => h("p", { class: "error-text", text: `${arrReason(f.reason)}${f.detail ? `: ${f.detail}` : ""}. ${arrNext(f.reason) || ""}`.trim() });
  const caught = (el) => (e) => {
    if (e.status !== 401 && e.status !== 429) el.append(h("p", { class: "error-text", text: reason(e) }));
  };
  const verb = r.outcome === "updated" ? "Updated" : r.outcome === "unchanged" ? "Already set up:" : "Created";
  const client = okLine(`${verb} ${r.client_name} in ${r.app}${r.category ? `, category ${r.category}` : ""}, host ${r.host}`);
  const folder = h("div", { role: "status" });
  const recs = h("div", { role: "status" });
  const app = r.app;
  box.replaceChildren(client, folder, recs, h("div", { class: "form-foot" }, h("button", { type: "button", class: "btn", onclick: () => reload() }, "Done")));

  const showRecs = (list) =>
    recs.replaceChildren(
      ...list.map((x) => {
        if (x.on) return okLine(`${RECOMMEND[x.name]}: on`);
        const on = h("button", { type: "button", class: "btn" }, "Turn on");
        on.onclick = async () => {
          on.disabled = true;
          try {
            const t = await applyArrRecommendation({ ...base, name: x.name });
            if (!t.ok) return recs.append(bad(t));
            showRecs(t.recommendations);
          } catch (e) {
            caught(recs)(e);
          } finally {
            on.disabled = false;
          }
        };
        return h("p", { class: "muted" }, `Recommended: ${RECOMMEND[x.name]} `, on);
      }),
    );

  const check = async () => {
    folder.replaceChildren(h("p", { class: "muted", text: "Checking the download folder… this can take up to 30 s." }));
    let c;
    try {
      c = await checkArrSetup(base);
    } catch (e) {
      folder.replaceChildren();
      return caught(folder)(e);
    }
    if (!box.isConnected) return;
    if (!c.ok) return folder.replaceChildren(bad(c));
    showRecs(c.recommendations);
    if (c.recommendations_error) recs.append(bad(c.recommendations_error));
    const f = c.folder;
    if (!f) return folder.replaceChildren(bad(c.folder_error));
    const p = f.proposal;
    if (p.action === "none") return folder.replaceChildren(...pathLines(f.path_check, app));
    if (p.action === "not_found") {
      return folder.replaceChildren(
        ...pathLines(f.path_check, app),
        h("p", { class: "muted", text: `Not found in the ${f.looked.length} folders ${app} lists. Mount the download folder in ${app}, then Set up again.` }),
      );
    }
    const what =
      p.action === "remove"
        ? `${app} sees ${p.remote_path} as is, but a mapping for host ${p.host} hides it: remove that mapping`
        : `${app} sees the folder at ${f.found}: map ${p.remote_path} → ${p.local_path} for host ${p.host}`;
    const btn = h("button", { type: "button", class: "btn btn-primary" }, MAP_VERB[p.action]);
    btn.onclick = async () => {
      if (!(await confirm({ title: MAP_VERB[p.action], text: `${what}?`, verb: MAP_VERB[p.action].split(" ")[0] }))) return;
      btn.disabled = true;
      try {
        const m = await applyArrMapping({ ...base, host: c.host, proposal: p });
        if (m.ok) {
          toast(`${MAP_VERB[p.action]}: done`);
          return folder.replaceChildren(...pathLines(m.path_check, app));
        }
        if (m.reason === "changed") return check();
        folder.append(bad(m));
      } catch (e) {
        caught(folder)(e);
      }
      btn.disabled = false;
    };
    folder.replaceChildren(h("p", { class: "warn-text", text: what }), h("div", { class: "form-foot" }, btn));
  };
  check();
}

/** What putio-sync knows about an application's categories (R4) */
function categoriesLine(s) {
  if (!s.api_key_set) return null;
  if (!s.read_at) {
    return h("p", { class: "muted", text: s.read_error ? `Categories not read: ${arrReason(s.read_error)}` : "Categories not read yet" });
  }
  const age = `read ${relative(s.read_at)}`;
  const stale = s.read_error ? `; last read failed: ${arrReason(s.read_error)}` : "";
  if (!s.client_present) return h("p", { class: "muted", text: `Not set up: no putio-sync client there (${age}${stale})` });
  const cats = s.categories.length ? s.categories.join(", ") : "none";
  return h("p", { class: s.read_error ? "warn-text" : "muted", text: `Category ${cats} (${age}${stale})` });
}

function testResult(r) {
  if (r.ok) {
    const cats = r.client_present ? `, category ${r.categories.join(", ") || "none"}` : ", no putio-sync client yet";
    return h("div", {}, h("p", {}, h("span", { class: "dot ok" }), `Connected to ${r.app} ${r.version}${cats}`), pathLines(r.path_check, r.app));
  }
  return h("p", { class: "error-text", text: `${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}. ${arrNext(r.reason) || ""}`.trim() });
}

/** One saved application: categories, warnings, Test, Set up, Edit and Delete */
function applicationRow(s, list, reload) {
  const result = h("div", { role: "status" });
  const test = h("button", { type: "button", class: "btn" }, "Test");
  // The client, the folder mapping and the recommendations; safe to run again (049)
  const setUp = s.api_key_set ? h("button", { type: "button", class: "btn" }, "Set up") : null;
  const edit = h("button", { type: "button", class: "btn" }, "Edit");
  const remove = h("button", { type: "button", class: "btn" }, "Delete");
  const legacy = s.legacy_category && s.read_at && !s.categories.includes(normalCategory(s.legacy_category));

  test.onclick = async () => {
    test.disabled = true;
    result.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const r = await testArrApplication(s.index);
      if (!row.isConnected) return;
      result.replaceChildren(testResult(r));
      // The categories were re-read: show them
      if (r.ok) await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    } finally {
      if (test.isConnected) test.disabled = false;
    }
  };
  if (setUp) setUp.onclick = async () => {
    setUp.disabled = true;
    result.replaceChildren(h("p", { class: "muted", text: "Setting up… this can take up to 30 s." }));
    try {
      const r = await runArrSetup({ target: { application: s.index } });
      if (!row.isConnected) return;
      if (r.outcome === "failed") {
        fill(
          result,
          h("p", { class: "error-text", text: `${arrReason(r.reason)}${r.detail ? `: ${r.detail}` : ""}` }),
          arrNext(r.reason) ? h("p", { class: "muted", text: arrNext(r.reason) }) : null,
        );
        setUp.disabled = false;
        return;
      }
      guided({ target: { application: s.index }, host: r.host }, result, r, reload);
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
      if (setUp.isConnected) setUp.disabled = false;
    }
  };
  edit.onclick = () => {
    row.replaceWith(applicationForm(s.kind, s, reload, () => reload()));
  };
  remove.onclick = async () => {
    const ok = await confirm({
      title: `Delete ${s.name}`,
      text: `Remove ${s.name} from putio-sync? Its putio-sync download client in ${KIND_NAME[s.kind]} stays as it is.`,
      verb: "Delete",
    });
    if (!ok) return;
    try {
      await deleteArrApplication(s.index);
      toast(`${s.name} removed`);
      await reload();
    } catch (e) {
      if (e.status === 401 || e.status === 429) return;
      result.replaceChildren(h("p", { class: "error-text", text: reason(e) }));
    }
  };

  const row = h(
    "div",
    {},
    h("div", { class: "form-foot" }, h("span", {}, h("strong", { text: s.name }), " ", h("span", { class: "badge", text: KIND_NAME[s.kind] })), test, setUp, edit, remove),
    h("p", { class: "muted name", text: `${s.url}${s.api_key_set ? "" : " (no API key)"}${s.api_key_env ? " (key from environment)" : ""}` }),
    categoriesLine(s),
    pathLines(s.path_check, s.name),
    legacy
      ? h("p", { class: "warn-text", text: `The typed category “${s.legacy_category}” is no longer used. Blocklisting follows the category of ${s.name}'s putio-sync client.` })
      : null,
    result,
  );
  return row;
}

/** Add (`s` null) or Edit form. Saving after a failed or missing test asks first (FR-012). */
function applicationForm(kind, s, reload, onCancel) {
  const id = (f) => `arr-app-${s ? s.index : "new"}-${f}`;
  const input = (f, attrs) => h("input", { class: "input", id: id(f), autocomplete: "off", spellcheck: "false", ...attrs });
  const name = input("name", { type: "text", placeholder: KIND_NAME[kind] });
  const host = input("host", { type: "text", required: true, placeholder: kind });
  const port = input("port", { type: "number", min: 1, max: 65535, placeholder: String(DEFAULT_PORT[kind]) });
  const ssl = h("input", { type: "checkbox", id: id("ssl") });
  const base = input("base", { type: "text", placeholder: "none" });
  const key = input("key", {
    type: "password",
    required: !s,
    disabled: Boolean(s && s.api_key_env),
    placeholder: s ? (s.api_key_env ? "set by environment" : s.api_key_set ? "set; leave empty to keep" : "not set") : "",
  });
  if (s) {
    name.value = s.name;
    host.value = s.host;
    port.value = s.port ? String(s.port) : "";
    ssl.checked = Boolean(s.ssl);
    base.value = s.url_base;
  }
  const out = h("div", { role: "status" });
  const error = h("p", { class: "error-text", role: "alert", hidden: true });
  const testBtn = h("button", { type: "button", class: "btn" }, "Test");
  const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
  const cancel = h("button", { type: "button", class: "btn", onclick: onCancel }, "Cancel");
  // The last test's result for the values as they are now; any change clears it
  let passed = false;

  const body = () => ({
    kind,
    name: name.value.trim(),
    host: host.value.trim(),
    port: port.value ? Number(port.value) : null,
    ssl: ssl.checked,
    url_base: base.value.trim(),
    api_key: key.value.trim(),
  });
  const shown = (e) => {
    if (e.status === 401 || e.status === 429) return;
    error.textContent = reason(e);
    error.hidden = false;
  };

  testBtn.onclick = async () => {
    testBtn.disabled = true;
    error.hidden = true;
    out.replaceChildren(h("p", { class: "muted", text: "Testing…" }));
    try {
      const r = await testArrForm(s ? { ...body(), index: s.index } : body());
      if (!form.isConnected) return;
      passed = r.ok;
      out.replaceChildren(testResult(r));
    } catch (e) {
      out.replaceChildren();
      shown(e);
    } finally {
      if (testBtn.isConnected) testBtn.disabled = false;
    }
  };

  const onSubmit = async (ev) => {
    ev.preventDefault();
    error.hidden = true;
    if (!passed) {
      const ok = await confirm({
        title: "Save without a passing test",
        text: `${KIND_NAME[kind]} hasn't passed a test with these values. Save anyway?`,
        verb: "Save",
      });
      if (!ok) return;
    }
    save.disabled = true;
    try {
      const res = s ? await saveArrApplication(s.index, body()) : await addArrApplication(body());
      toast((res && res.message) || "Saved");
      await reload();
    } catch (e) {
      shown(e);
      if (save.isConnected) save.disabled = false;
    }
  };

  const form = h(
    "form",
    { class: "stack", onsubmit: onSubmit, oninput: () => (passed = false) },
    h("strong", { text: s ? `Edit ${s.name}` : `Add ${KIND_NAME[kind]}` }),
    h(
      "div",
      { class: "fields" },
      h("label", { class: "field", for: id("name") }, h("span", { text: "Name" }), name),
      h("label", { class: "field", for: id("host") }, h("span", { text: "Host" }), host, h("small", { text: "A name or address only, like radarr or 192.168.1.10" })),
      h("label", { class: "field", for: id("port") }, h("span", { text: "Port" }), port),
      h("label", { class: "field check", for: id("ssl") }, ssl, h("span", {}, "Use SSL")),
      h("label", { class: "field", for: id("base") }, h("span", { text: "URL base" }), base, h("small", { text: "Only behind a reverse proxy, like /radarr" })),
      h(
        "label",
        { class: "field", for: id("key") },
        h("span", { text: "API key" }),
        key,
        h("small", { text: s && s.api_key_env ? `Set by environment (PUTIO_SYNC_${kind.toUpperCase()}_API_KEY)` : "Settings → General. Never shown once saved." }),
      ),
    ),
    h("div", { class: "form-foot" }, testBtn, save, cancel),
    out,
    error,
  );
  return form;
}
