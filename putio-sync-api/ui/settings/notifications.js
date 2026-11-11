// Settings › Notifications (feature 030): email over SMTP and the bundled notifier plugins
// on one page. Loaded the first time the section opens (pages/settings.js), so it costs
// nothing until then. Plugin credentials stay on their tab in Settings › Plugins.

import { get, post, put } from "../api.js";
import { confirm, fill, h, toast, toastError } from "../app.js";
import * as plugins from "../plugins.js";
import * as notify from "../notify.js";
import { quiet, reason } from "./form.js";
import { settingsHref } from "./sections.js";

export const cards = (ctx) => [email(ctx), notifiers(ctx)];

const field = (id, label, input, hint) => h("label", { class: "field", for: id }, h("span", { text: label }), input, hint ? h("small", { text: hint }) : null);

/** One checkbox per event type; `on(type, checked)` runs on each change */
function eventBoxes(prefix, chosen, on) {
  return h(
    "fieldset",
    { class: "choice" },
    h("legend", { text: "Events" }),
    notify.EVENTS.map(([type, label]) => {
      const box = h("input", { type: "checkbox", id: `${prefix}-${type}`, checked: chosen.includes(type), onchange: () => on(type, box.checked) });
      return h("label", { class: "field check", for: box.id }, box, h("span", { text: label }));
    }),
  );
}

function email(ctx) {
  const card = h("section", { class: "card", id: "card-notifications" });
  let data = null;
  let form = null;
  const load = async () => {
    try {
      data = await get("/settings/notifications");
      if (!ctx.alive()) return;
      form = notify.emailForm(data.smtp);
      draw();
      ctx.report("notifications", { dirty: false, problem: false });
    } catch (e) {
      if (!ctx.alive() || quiet(e)) return;
      ctx.report("notifications", { problem: true });
      card.replaceChildren(h("h2", { text: "Email" }), h("p", { class: "error-text", text: reason(e) }), h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"));
    }
  };
  const dirty = () => ctx.report("notifications", { dirty: true });
  const draw = () => {
    const status = h("p", { class: "muted", role: "status", text: notify.emailStatusText(data.smtp_status) });
    const error = h("p", { class: "error-text", role: "alert", hidden: true });
    const text = (key, label, hint, attrs = {}) => {
      const input = h("input", { class: "input", id: `set-notifications-smtp-${key}`, autocomplete: "off", spellcheck: "false", ...attrs });
      input.value = form[key];
      input.addEventListener("input", () => {
        form[key] = input.value;
        dirty();
      });
      return field(input.id, label, input, hint);
    };
    const enabled = h("input", { type: "checkbox", id: "set-notifications-smtp-enabled", checked: form.enabled });
    enabled.addEventListener("change", () => {
      form.enabled = enabled.checked;
      dirty();
    });
    const security = h("select", { class: "input", id: "set-notifications-smtp-security" }, notify.SECURITY.map(([v, label]) => h("option", { value: v, selected: v === form.security, text: label })));
    const login = h("div", { class: "stack", hidden: form.security === "none" });
    security.addEventListener("change", () => {
      form.security = security.value;
      login.hidden = form.security === "none";
      dirty();
    });
    // The password is never shown: it is set or not, and can be replaced
    const pass = h("input", { class: "input", id: "set-notifications-smtp-password", type: "password", autocomplete: "new-password", placeholder: form.password.set ? "Set; type to replace" : "" });
    pass.addEventListener("input", () => {
      form.password = { ...form.password, replace: true, value: pass.value };
      dirty();
    });
    login.append(text("username", "Username", "Leave empty for a server that needs no login"), field(pass.id, "Password", pass));
    const send = async (body, button, what) => {
      button.disabled = true;
      error.hidden = true;
      try {
        await what(body);
      } catch (e) {
        if (!quiet(e)) {
          error.textContent = reason(e);
          error.hidden = false;
        }
      }
      if (button.isConnected) button.disabled = false;
    };
    const save = h("button", { type: "submit", class: "btn btn-primary" }, "Save");
    const test = h("button", { type: "button", class: "btn" }, "Send test email");
    test.addEventListener("click", () =>
      send(notify.emailBody(form), test, async (smtp) => {
        status.textContent = "Sending…";
        const r = await post("/settings/notifications/test", smtp);
        status.textContent = r.ok ? `Test email sent to ${smtp.to.join(", ")}.` : `Test failed: ${r.error}`;
        status.className = r.ok ? "muted" : "error-text";
      }),
    );
    card.replaceChildren(
      h(
        "form",
        {
          onsubmit: (ev) => {
            ev.preventDefault();
            const body = { ...data, smtp: notify.emailBody(form) };
            delete body.smtp_status;
            send(body, save, async (b) => {
              const r = await put("/settings/notifications", b);
              toast(r.message || "Saved");
              ctx.onSaved();
              await load();
            });
          },
        },
        h("h2", { text: "Email" }),
        h("p", { class: "muted", text: "Plain-text email through your mail server. The first event of a kind is sent at once; repeats within the grouping time come as one summary." }),
        h("label", { class: "field check", for: enabled.id }, enabled, h("span", { text: "Send email" })),
        text("host", "Mail server", "Such as smtp.example.com"),
        field(security.id, "Security", security, "STARTTLS must succeed; nothing is sent unencrypted unless you choose None"),
        text("port", "Port", "Leave empty for the usual port", { type: "number", min: 1, max: 65535 }),
        login,
        text("from", "From", "Such as putio-sync <nas@example.com>"),
        text("to", "To", "One or more addresses, separated by commas"),
        text("group_minutes", "Group repeats for (minutes)", "0 sends each event", { type: "number", min: 0 }),
        eventBoxes("set-notifications-smtp-events", form.events, (type, on) => {
          form.events = on ? [...form.events, type] : form.events.filter((t) => t !== type);
          dirty();
        }),
        error,
        h("div", { class: "toolbar spaced" }, save, test),
        status,
      ),
    );
  };
  card.append(h("h2", { text: "Email" }), h("p", { class: "muted", text: "Loading…" }));
  load();
  return card;
}

/** The bundled notifier plugins: on or off, what they send, and a test */
function notifiers(ctx) {
  const card = h("section", { class: "card", id: "card-notifiers" }, h("h2", { text: "Push notifications" }), h("p", { class: "muted", text: "Loading…" }));
  let stored = [];
  let data = null;
  const load = async () => {
    try {
      const [found, settings] = await Promise.all([get("/plugins"), get("/settings/plugins")]);
      if (!ctx.alive()) return;
      data = settings;
      stored = settings.plugins || [];
      draw(notify.notifierRows(found.plugins || [], stored));
    } catch (e) {
      if (!ctx.alive() || quiet(e)) return;
      card.replaceChildren(h("h2", { text: "Push notifications" }), h("p", { class: "error-text", text: reason(e) }), h("button", { type: "button", class: "btn spaced", onclick: load }, "Retry"));
    }
  };
  const save = async (list, done) => {
    const body = { ...data, plugins: list };
    delete body.effective;
    delete body.status;
    try {
      await put("/settings/plugins", body);
      toast(done);
      ctx.onSaved();
    } catch (e) {
      if (!quiet(e)) toastError(e);
    }
    await load();
  };
  const row = ({ plugin, saved, row }) => {
    const on = plugin.state && plugin.state !== "disabled";
    const out = h("span", { class: "muted", role: "status" });
    const turn = async () => {
      const ok = on ? true : await confirm({ title: `Enable ${plugin.name}?`, text: plugins.approvalText(plugin, row), verb: "Enable" });
      if (ok) await save(plugins.withEnabled(stored, plugin, !on), `${plugin.name} is ${on ? "off" : "on"}`);
    };
    const test = async (ev) => {
      const button = ev.currentTarget;
      button.disabled = true;
      out.textContent = "Sending…";
      try {
        out.textContent = plugins.runLabel(await post("/plugins/test", { name: plugin.name }));
      } catch (e) {
        if (!quiet(e)) out.textContent = reason(e);
      }
      if (button.isConnected) button.disabled = false;
    };
    return h(
      "div",
      { class: "spaced", id: `notifier-${plugin.name}` },
      h(
        "div",
        { class: "toolbar" },
        h("strong", { text: plugin.name }),
        h("span", { class: `badge ${plugin.state === "enabled" ? "ok" : on ? "warn" : ""}`, text: plugins.stateLabel(plugin.state) }),
        h("button", { type: "button", class: "btn btn-small", onclick: turn }, on ? "Disable" : "Enable"),
        on ? h("button", { type: "button", class: "btn btn-small", onclick: test }, "Send test") : null,
        h("a", { href: settingsHref("plugins", "card-plugins"), text: saved ? "Edit settings" : "Set up" }),
        out,
      ),
      h("small", { class: "muted", text: notify.notifierSummary(row) || plugins.summary(plugin.description) }),
      on ? eventBoxes(`notifier-${plugin.name}-events`, notify.notifierEvents(row), (type, checked) => save(notify.withEvents(stored, row, type, checked), `${plugin.name} events saved`)) : null,
    );
  };
  const draw = (rows) =>
    fill(
      card,
      h("h2", { text: "Push notifications" }),
      h("p", { class: "muted", text: "Phone and chat notifications through the bundled plugins. Their accounts and tokens are set on their tab in Settings › Plugins." }),
      rows.length ? rows.map(row) : h("p", { class: "muted", text: "The bundled notifier plugins were not found." }),
    );
  load();
  return card;
}
