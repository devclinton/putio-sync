// The plugin workbench (feature 055), loaded from the Plugins settings card when it is
// opened: the editor setting, New plugin from a starter, a plain code editor with line
// numbers, the Run panel, the last 20 runs, and Install (029) for WASM plugins.

import { get, post, put } from "../api.js";
import { docs, fill, h, toast, toastError } from "../app.js";
import * as wb from "../workbench.js";
import { form as installForm } from "./plugin_install.js";
import { quiet, reason } from "./form.js";

const GUIDE = "plugin-workbench";
const CODE = "font-family:var(--mono,monospace);font-size:13px;line-height:1.45;white-space:pre;tab-size:2";
const fail = (e) => quiet(e) || toastError(e instanceof Error && !e.status ? { error: e.message } : e);

const field = (label, input, hint) => h("label", { class: "field" }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);
const select = (label, options, value) =>
  h("select", { class: "input", "aria-label": label }, options.map(([v, text]) => h("option", { value: v, selected: v === value }, text)));
const pre = (text) => h("pre", { class: "input", style: `${CODE};max-height:16rem;overflow:auto;margin:0` }, text);

/** A code area with line numbers; `readOnly` for files that can't be edited here */
function codeArea(label, text, readOnly) {
  // The numbers sit in a clipped box as tall as the text area and move with its scroll
  const gutter = h("pre", { "aria-hidden": "true", style: `${CODE};position:absolute;right:.4rem;top:0;margin:0;text-align:right;opacity:.6;user-select:none` });
  const box = h("div", { style: "position:relative;overflow:hidden" }, gutter);
  const area = h("textarea", { class: "input", rows: 16, spellcheck: "false", wrap: "off", readonly: readOnly, "aria-label": label, style: `${CODE};flex:1;min-width:0;resize:vertical;margin:0` }, text);
  const scroll = () => {
    const css = getComputedStyle(area);
    gutter.style.top = `${area.clientTop + parseFloat(css.paddingTop || 0) - area.scrollTop}px`;
  };
  const number = () => {
    gutter.textContent = wb.lineNumbers(area.value);
    box.style.minWidth = `${String(area.value.split("\n").length).length + 1.5}ch`;
    scroll();
  };
  area.addEventListener("input", number);
  area.addEventListener("scroll", scroll);
  number();
  /** Put the caret on a line and show it */
  const goTo = (line) => {
    const lines = area.value.split("\n");
    const at = lines.slice(0, line - 1).reduce((n, l) => n + l.length + 1, 0);
    area.focus();
    area.setSelectionRange(at, at + (lines[line - 1] || "").length);
    area.scrollTop = Math.max(0, (line - 4) * parseFloat(getComputedStyle(area).lineHeight || 18));
  };
  // The padding is known once the area is on the page
  requestAnimationFrame(scroll);
  return { node: h("div", { style: "display:flex;align-items:stretch" }, box, area), area, goTo };
}

/** The workbench's nodes; `reloaded` draws the Plugins list again after a change */
export function panel(reloaded) {
  let overview = null;
  let plugins = [];
  let source = null; // the opened plugin's files
  let editors = {}; // path → code area
  const head = h("div", { class: "toolbar" });
  const maker = h("div", { class: "stack" });
  const opened = h("div", { class: "stack" });
  const runner = h("div", { class: "stack" });
  const result = h("div", { class: "stack" });
  const history = h("div", { class: "stack" });
  const picker = h("select", { class: "input", "aria-label": "Plugin", onchange: () => open(picker.value) });

  const refresh = async () => {
    [overview, plugins] = await Promise.all([get("/plugins/workbench"), get("/plugins").then((s) => s.plugins || [])]);
    drawHead();
    drawMaker();
    fill(picker, plugins.map((p) => h("option", { value: p.name, selected: p.name === source?.name }, `${p.name} (${p.runtime}, ${p.source})`)));
    if (!source && plugins.length) await open(plugins[0].name);
  };

  const drawHead = () => {
    const toggle = h("input", {
      type: "checkbox",
      checked: overview.editor,
      onchange: async (ev) => {
        const box = ev.currentTarget;
        const on = box.checked;
        try {
          toast((await put("/plugins/workbench/editor", { enabled: on })).message);
          await refresh();
          if (source) await open(source.name);
        } catch (e) {
          box.checked = !on;
          fail(e);
        }
      },
    });
    fill(
      head,
      h("label", { class: "field check" }, toggle, h("span", { text: "Edit plugins here" })),
      h("small", { class: "muted", text: overview.dir ? `Plugins folder: ${overview.dir}` : "No plugins folder" }),
      docs(GUIDE, "", "Writing and debugging plugins"),
    );
  };

  const drawMaker = () => {
    if (!overview.editor) return fill(maker, h("p", { class: "muted", text: "Turn on Edit plugins here to make, edit and copy plugins. Running a saved plugin needs no editor." }));
    const starter = select("Starter", overview.starters.map((s) => [s.id, s.title]));
    const hint = h("small", { class: "muted" });
    const describe = () => (hint.textContent = overview.starters.find((s) => s.id === starter.value)?.description || "");
    starter.addEventListener("change", describe);
    describe();
    const name = h("input", { class: "input", placeholder: "my-plugin", autocomplete: "off", spellcheck: "false", "aria-label": "New plugin name" });
    const create = async (ev) => {
      const button = ev.currentTarget;
      button.disabled = true;
      try {
        const answer = await post("/plugins/workbench/new", { starter: starter.value, name: name.value.trim() });
        toast(answer.message);
        name.value = "";
        source = null;
        await reloaded();
        await refresh();
        picker.value = answer.name;
        await open(answer.name);
      } catch (e) {
        fail(e);
      }
      button.disabled = false;
    };
    fill(
      maker,
      h("h3", { class: "sub", text: "New plugin" }),
      h("div", { class: "fields" }, field("Starter", starter), field("Name", name, "Letters, digits, - and _")),
      hint,
      h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small", onclick: create }, "Make plugin"), docs(GUIDE, "starters", "About the starters")),
    );
  };

  /** " Go to line N" after an error, when that file is open in the editor */
  const goTo = (file, line) => {
    const area = line && editors[String(file).split("/").pop()];
    return area ? [" ", h("a", { href: "#", onclick: (ev) => (ev.preventDefault(), area.goTo(line)) }, `Go to line ${line}`)] : null;
  };
  // A problem's message is a reload's, file and line included
  const problemsList = (problems) =>
    problems.length ? h("ul", { class: "error-text" }, problems.map((p) => h("li", null, p.message, goTo(p.file, p.line)))) : null;
  const problemsBox = h("div");

  /** The editor's manifest and script */
  const texts = () => ({ manifest: editors["plugin.toml"].area.value, script: editors[source.files[1]?.path]?.area.value ?? "" });

  const check = async () => {
    const { manifest, script } = texts();
    const { problems } = await post("/plugins/workbench/check", { manifest, script });
    fill(problemsBox, problems.length ? problemsList(problems) : h("p", { class: "muted", text: "No problems found" }));
    return problems;
  };

  const save = async (ev, force = false) => {
    const button = ev?.currentTarget;
    if (button) button.disabled = true;
    try {
      const { manifest, script } = texts();
      const versions = Object.fromEntries(source.files.map((f) => [f.path, f.version]));
      const answer = await put(`/plugins/workbench/source/${encodeURIComponent(source.name)}`, { manifest, script, versions, force });
      toast(answer.message);
      fill(problemsBox, problemsList(answer.problems));
      await reloaded();
      await open(source.name, true);
    } catch (e) {
      if (e.status === 409 && !force) {
        fill(
          problemsBox,
          h("p", { class: "error-text", text: reason(e) }),
          h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small", onclick: () => save(null, true) }, "Save over the changes"), h("button", { type: "button", class: "btn btn-small", onclick: () => open(source.name) }, "Load the files again")),
        );
      } else fail(e);
    }
    if (button && button.isConnected) button.disabled = false;
  };

  const copy = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    try {
      toast((await post(`/plugins/workbench/copy/${encodeURIComponent(source.name)}`, {})).message);
      await reloaded();
      await open(source.name);
    } catch (e) {
      fail(e);
      button.disabled = false;
    }
  };

  /** Open a plugin's files; `keepProblems` leaves the save's problems on show */
  async function open(name, keepProblems = false) {
    try {
      source = await get(`/plugins/workbench/source/${encodeURIComponent(name)}`);
    } catch (e) {
      return fill(opened, h("p", { class: "error-text", text: reason(e) }));
    }
    const how = wb.mode(source, overview.editor);
    editors = {};
    const areas = source.files.map((f) => {
      const file = f.path;
      const area = codeArea(file, f.text, how !== "edit");
      editors[file] = area;
      return field(file, area.node);
    });
    if (!keepProblems) fill(problemsBox);
    const note = {
      edit: null,
      copy: "A bundled plugin is read-only. Copy to edit puts a copy in the plugins folder, which replaces the bundled one.",
      view: source.source === "bundled" ? "A bundled plugin is read-only; turn on Edit plugins here to copy it." : "Turn on Edit plugins here to change this plugin.",
      wasm: "A WebAssembly plugin is built elsewhere, so only its manifest shows here. Install a new build over it with Install or replace a plugin, Replace ticked.",
    }[how];
    fill(
      opened,
      note ? h("p", { class: "muted", text: note }) : null,
      areas,
      source.other_files.length ? h("small", { class: "muted", text: `Also in the folder: ${source.other_files.join(", ")}` }) : null,
      how === "edit"
        ? h(
            "div",
            { class: "toolbar" },
            h("button", { type: "button", class: "btn btn-small", onclick: () => check().catch(fail) }, "Check"),
            h("button", { type: "button", class: "btn btn-small btn-primary", onclick: save }, "Save and reload"),
            docs(GUIDE, "the-editor", "About the editor"),
          )
        : null,
      how === "copy" ? h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small", onclick: copy }, "Copy to edit")) : null,
      problemsBox,
    );
    drawRunner();
  }

  const drawRunner = () => {
    const status = plugins.find((p) => p.name === source.name) || {};
    const kinds = [["event", "An event"], ["tick", "on_tick"], ["health", "health"]];
    if (status.actions?.length) kinds.push(["action", "A settings action"]);
    if (status.hooks?.length) kinds.push(["hook", "A sync rule"]);
    const kind = select("Call", kinds);
    const events = overview.events.filter((e) => !status.events?.length || status.events.some((x) => x === "*" || x === e.type || (x.endsWith(".*") && e.type.startsWith(x.slice(0, -1)))));
    const eventType = select("Event type", (events.length ? events : overview.events).map((e) => [e.type, e.type]));
    const action = select("Action", (status.actions || []).map((a) => [a.id, a.label || a.id]));
    const hook = select("Sync rule", (status.hooks || []).map((x) => [x, x]));
    const json = h("textarea", { class: "input", rows: 8, spellcheck: "false", style: CODE, "aria-label": "Input" });
    const eventDocs = h("span");
    const real = h("input", { type: "checkbox" });
    const replyStatus = h("input", { class: "input", type: "number", value: 200, min: 100, max: 599, "aria-label": "Reply status" });
    const replyBody = h("input", { class: "input", value: "", spellcheck: "false", "aria-label": "Reply body" });
    const unsaved = h("input", { type: "checkbox" });
    const rows = { event: field("Event type", eventType), action: field("Action", action), hook: field("Sync rule", hook) };
    const jsonField = field("Input", json);
    const dry = h("div", { class: "fields" }, field("Dry run reply status", replyStatus), field("Dry run reply body", replyBody, "What each request gets back in a dry run"));
    const sample = () => {
      const k = kind.value;
      const ev = overview.events.find((e) => e.type === eventType.value);
      json.value = k === "event" ? JSON.stringify(ev?.data ?? {}, null, 2) : k === "hook" ? JSON.stringify(overview.file, null, 2) : k === "action" ? "{}" : "";
      jsonField.querySelector("span").textContent = { event: "Event data", hook: "File", action: "Action input" }[k] || "Input";
      jsonField.hidden = !["event", "hook", "action"].includes(k);
      for (const [key, node] of Object.entries(rows)) node.hidden = key !== k;
      fill(eventDocs, k === "event" ? docs("events", wb.anchor(eventType.value), `${eventType.value} fields`) : k === "hook" ? docs("plugins", "sync-rules", "Sync rules") : null);
    };
    kind.addEventListener("change", sample);
    eventType.addEventListener("change", sample);
    real.addEventListener("change", () => (dry.hidden = real.checked));
    sample();
    const go = async (ev) => {
      const button = ev.currentTarget;
      button.disabled = true;
      try {
        const values = { name: source.name, kind: kind.value, eventType: eventType.value, action: action.value, hook: hook.value, json: json.value, real: real.checked, status: replyStatus.value, body: replyBody.value };
        if (unsaved.checked) {
          const { manifest, script } = texts();
          values.unsaved = { manifest, script };
        }
        show(await post("/plugins/workbench/run", wb.runBody(values)));
        await drawHistory();
      } catch (e) {
        fail(e);
      }
      button.disabled = false;
    };
    const canUnsaved = overview.editor && wb.mode(source, overview.editor) === "edit";
    fill(
      runner,
      h("h3", { class: "sub", text: "Run" }),
      h("p", { class: "muted", text: "A dry run records HTTP requests instead of sending them and uses a scratch copy of the store. Send for real uses the approved hosts and the real store." }),
      h("div", { class: "fields" }, field("Call", kind), rows.event, rows.action, rows.hook),
      jsonField,
      eventDocs,
      h("label", { class: "field check" }, real, h("span", { text: "Send for real" })),
      dry,
      canUnsaved ? h("label", { class: "field check" }, unsaved, h("span", { text: "Run the text in the editor, not the saved files" })) : null,
      h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small btn-primary", onclick: go }, "Run"), docs(GUIDE, "the-run-panel", "About the Run panel")),
    );
  };

  /** One run's result: the value or error with its line, logs, requests, writes and store */
  const show = (run) => {
    const where = wb.errorLine(run.run.error);
    const block = (title, ...body) => h("details", { open: true }, h("summary", { text: title }), ...body);
    const value = run.problems.length
      ? problemsList(run.problems)
      : run.run.error
        ? h(
            "p",
            { class: "error-text" },
            run.run.error,
            where ? goTo(where.file, where.line) : null,
          )
        : h("div", null, run.answer ? h("p", { text: run.answer }) : null, pre(run.value == null ? "Returned nothing" : JSON.stringify(run.value, null, 2)));
    fill(
      result,
      h("h3", { class: "sub", text: "Result" }),
      h("p", { class: "muted", text: wb.runSummary(run) }),
      value,
      block(`Log (${run.run.logs.length})`, run.run.logs.length ? pre(run.run.logs.map((l) => `${l.level.padEnd(5)} ${l.message}`).join("\n")) : h("p", { class: "muted", text: "Nothing logged" })),
      block(
        `HTTP requests (${run.requests.length})`,
        run.requests.map((r) =>
          h("details", null, h("summary", { text: wb.requestLine(r) }), pre([...Object.entries(r.headers || {}).map(([k, v]) => `${k}: ${v}`), "", r.body || ""].join("\n"))),
        ),
      ),
      run.writes.length ? block(`Files it would write (${run.writes.length})`, h("ul", null, run.writes.map((w) => h("li", { text: `${w.path} (${w.bytes} bytes)` })))) : null,
      run.store_before != null || run.store_after != null
        ? block("Store", h("div", { class: "fields" }, field("Before", pre(wb.storeText(run.store_before))), field("After", pre(wb.storeText(run.store_after)))))
        : null,
    );
  };

  const drawHistory = async () => {
    const { runs } = await get("/plugins/workbench/runs");
    const again = async (ev, id) => {
      const button = ev.currentTarget;
      button.disabled = true;
      try {
        show(await post(`/plugins/workbench/runs/${id}`, {}));
        await drawHistory();
      } catch (e) {
        fail(e);
        button.disabled = false;
      }
    };
    fill(
      history,
      runs.length ? h("h3", { class: "sub", text: "Last runs" }) : null,
      runs.length
        ? h(
            "ul",
            { class: "plugin-list" },
            runs.map((r) => h("li", null, h("span", { text: wb.runSummary(r) }), " ", h("button", { type: "button", class: "btn btn-small", onclick: (ev) => again(ev, r.id) }, "Run again"))),
          )
        : null,
    );
  };

  refresh()
    .then(drawHistory)
    .catch((e) => fill(opened, h("p", { class: "error-text", text: reason(e) })));
  const install = h("details", { class: "spaced" }, h("summary", { text: "Install or replace a plugin" }), ...installForm(async () => (await reloaded(), await refresh(), source && open(source.name))));
  return [head, maker, install, h("h3", { class: "sub", text: "Plugin" }), field("Plugin", picker), opened, runner, result, history];
}
