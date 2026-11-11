// Installing a plugin (029), loaded from the Plugins settings card when Install is opened:
// a URL to a .wasm, one .wasm file, or plugin.toml with its entry file.

import { post } from "../api.js";
import { h, toast, toastError } from "../app.js";
import { quiet } from "./form.js";

const base64 = async (file) => {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
};

/** The `POST /plugins/install` body for a URL or chosen files: one .wasm, or plugin.toml and
 *  its entry file */
export async function installBody(url, files, replace) {
  if (url) return { url, replace };
  const list = [...files];
  const manifest = list.find((f) => f.name === "plugin.toml");
  const entry = list.find((f) => f !== manifest);
  if (!manifest && entry && entry.name.endsWith(".wasm") && list.length === 1) return { wasm: await base64(entry), replace };
  if (!manifest || !entry || list.length !== 2) throw new Error("Choose one .wasm file, or plugin.toml and its entry file");
  return { manifest: await manifest.text(), entry: await base64(entry), replace };
}

/** The Install form: a URL or files, Replace, and the button; the new plugin is listed
 *  disabled */
export function form(load) {
  const url = h("input", { class: "input", type: "url", placeholder: "https://…/plugin.wasm", "aria-label": "Plugin URL" });
  const files = h("input", { type: "file", multiple: true, accept: ".wasm,.toml,.lua", "aria-label": "Plugin files" });
  const replace = h("input", { type: "checkbox" });
  const run = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    try {
      const answer = await post("/plugins/install", await installBody(url.value.trim(), files.files, replace.checked));
      toast(answer.message);
      url.value = "";
      files.value = "";
      await load();
    } catch (e) {
      if (!quiet(e)) toastError(e instanceof Error ? { error: e.message } : e);
    }
    button.disabled = false;
  };
  const field = (label, input, hint) => h("label", { class: "field" }, h("span", { text: label }), input, h("small", { text: hint }));
  return [
    h("div", { class: "fields" }, field("From a URL", url, "A .wasm with its plugin.toml inside"), field("Or files", files, "One .wasm, or plugin.toml and its entry file"), h("label", { class: "field check" }, replace, h("span", { text: "Replace a plugin of the same name" }))),
    h("div", { class: "toolbar" }, h("button", { type: "button", class: "btn btn-small", onclick: run }, "Install")),
  ];
}
