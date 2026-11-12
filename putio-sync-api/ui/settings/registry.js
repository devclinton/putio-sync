// More plugins (059), loaded from the Plugins settings card when opened: the plugin
// registry's reviewed plugins, a search, and Install or Update for each. A new plugin is
// listed disabled; nothing runs until it is enabled.

import { get, post } from "../api.js";
import * as plugins from "../plugins.js";
import { confirm, h, toast } from "../app.js";
import { relative } from "../format.js";
import { quiet, reason } from "./form.js";

/** The More plugins panel; `load` redraws the installed plugins after an install */
export function panel(load) {
  let listing = null;
  const search = h("input", { class: "input", type: "search", placeholder: "Search plugins", "aria-label": "Search plugins", oninput: () => draw() });
  const status = h("p", { class: "muted" });
  const list = h("div", { class: "stack" }, h("p", { class: "muted", text: "Loading the plugin registry…" }));

  const install = async (entry, button) => {
    const updating = entry.state === "update";
    const ok = await confirm({
      title: updating ? `Update ${entry.name} to ${entry.version}?` : `Install ${entry.name} ${entry.version}?`,
      text: [`${plugins.byline({ links: entry })}.`, ...plugins.entryReach(entry), updating ? "Its settings are kept." : "It is added turned off; enable it to run it."].join("\n"),
      verb: updating ? "Update" : "Install",
    });
    if (!ok) return;
    button.disabled = true;
    try {
      toast((await post("/plugins/registry/install", { name: entry.name })).message);
      await Promise.all([fetch(false), load()]);
    } catch (e) {
      if (!quiet(e)) toast(reason(e));
      button.disabled = false;
    }
  };

  const item = (entry) => {
    const action = plugins.entryAction(entry);
    const button = h("button", { type: "button", class: "btn btn-small", disabled: !action.enabled }, action.label);
    button.onclick = () => install(entry, button);
    const links = plugins.links({ links: entry });
    return h(
      "div",
      { class: "registry-entry" },
      h("div", { class: "toolbar" }, h("strong", { text: entry.name }), entry.version ? h("span", { class: "muted", text: entry.version }) : null, button),
      h("p", { text: entry.description }),
      h("p", { class: "muted", text: plugins.entryReach(entry).join(" · ") }),
      entry.withdrawn ? h("p", { class: "error-text", text: `Withdrawn: ${entry.withdrawn}` }) : null,
      h(
        "p",
        { class: "plugin-links muted" },
        h("span", { text: plugins.byline({ links: entry }) }),
        ...links.map((l) => h("a", { href: l.href, target: "_blank", rel: "noopener noreferrer" }, l.label)),
        entry.notes && plugins.linkOk(entry.notes) ? h("a", { href: entry.notes, target: "_blank", rel: "noopener noreferrer" }, "Release notes") : null,
      ),
    );
  };

  const draw = () => {
    if (!listing) return;
    if (!listing.enabled) {
      status.textContent = "The plugin registry is off. Turn it on below to see more plugins.";
      list.replaceChildren();
      return;
    }
    const when = listing.fetched_at ? `Updated ${relative(listing.fetched_at)}` : "Not fetched yet";
    status.textContent = [when, listing.error ? `Could not refresh: ${listing.error}` : ""].filter(Boolean).join(". ");
    const shown = plugins.filterEntries(listing.plugins, search.value);
    list.replaceChildren(...(shown.length ? shown.map(item) : [h("p", { class: "muted", text: listing.plugins.length ? "No plugin matches" : "The registry lists no plugins yet" })]));
  };

  const fetch = (refresh) =>
    get(`/plugins/registry${refresh ? "?refresh=true" : ""}`)
      .then((l) => ((listing = l), draw()))
      .catch((e) => list.replaceChildren(h("p", { class: "error-text", text: reason(e) })));
  fetch(false);

  const refresh = async (ev) => {
    const button = ev.currentTarget;
    button.disabled = true;
    await fetch(true);
    button.disabled = false;
  };
  return [
    h("p", { class: "muted", text: "Plugins reviewed for the putio-sync plugin registry. Each is open source; read what it does before you install it." }),
    h("div", { class: "toolbar" }, search, h("button", { type: "button", class: "btn btn-small", onclick: refresh }, "Refresh")),
    status,
    list,
  ];
}
