// Downloads section (feature 046): the Download settings group shown as three cards, each
// saving its own fields on top of the rest of the group. The bandwidth schedule is feature 032.

import { entryFromRow, move, rowFromEntry } from "../bandwidth.js";
import { h } from "../app.js";
import { groupCard } from "./form.js";
import { cards as subtitleCards } from "./subtitles.js";
import { cards as translationCards } from "./translation.js";

// Field kinds: b checkbox, n number, n? number or empty, s text, s? text or empty,
// l one entry per line, or an array of <select> values.
const DOWNLOAD = {
  id: "download",
  key: "download",
  part: true,
  title: "Downloads",
  fields: [
    ["workers", "Workers", "Downloads that run at once", "n", 1],
    ["target_directory", "Download folder", "Where files are saved", "s"],
    ["max_retries", "Retries", "Attempts after a failed download", "n", 0],
    ["sort_order", "Sort order", "Order new files are queued in", ["ByDate", "ByName", "ByExtension", "BySize"]],
    ["on_existing", "When a file already exists", null, [
      ["resume", "Resume", "Finish a partial file, skip a complete one; a different file is kept and the new one saved as name (1)"],
      ["overwrite", "Overwrite", "Replace it with the Put.io copy once the download completes"],
      ["keep_both", "Keep both", "Keep it and save the new file as name (1), (2)…"],
    ]],
    ["resume_enabled", "Resume", "Continue a partial download from where it stopped; off starts it again", "b"],
    ["delete_after_download", "Delete from Put.io", "Remove each file from Put.io once downloaded", "b"],
    ["delete_empty_folders", "Delete empty folders", "Remove Put.io folders left empty", "b"],
    ["buffer_size_mb", "Buffer (MB)", "Write buffer per download", "n", 1],
    ["chunk_size_mb", "Chunk size (MB)", "Empty downloads each file in one request", "n?", 1],
    ["retry_initial_delay_secs", "First retry delay (s)", "Doubles on each retry", "n", 0],
    ["progress_style", "Progress style", "Console progress bars", ["PerFile", "Overall", "Individual", "Rotating", "None"]],
    ["reverse_sort", "Reverse sort", "Queue in the opposite order", "b"],
  ],
  advanced: ["buffer_size_mb", "chunk_size_mb", "retry_initial_delay_secs", "progress_style", "reverse_sort"],
};
const BANDWIDTH = {
  id: "bandwidth",
  key: "download",
  part: true,
  title: "Bandwidth",
  fields: [
    ["bandwidth_limit_bytes_per_sec", "Bandwidth limit (bytes/s)", "Outside the schedule below; empty for unlimited", "n?", 1],
    ["slow_limit_bytes_per_sec", "Slow limit (bytes/s)", "Used by Slow 1 h on the Status page; 1048576 is 1 MB/s", "n", 1],
  ],
  advanced: ["slow_limit_bytes_per_sec"],
  owns: ["bandwidth_schedule"],
  extra: (data) => {
    const schedule = scheduleEditor(data.bandwidth_schedule || []);
    return { nodes: schedule.node, read: () => ({ bandwidth_schedule: schedule.read() }) };
  },
};
const VERIFY = {
  id: "verify",
  key: "download",
  part: true,
  title: "Verification",
  fields: [
    ["verify", "Verify downloads", "Check each file against Put.io's CRC32 (or size) before it is finished", "b"],
    ["verify_max_redownloads", "Re-downloads on a mismatch", "Then the download fails; 0 to 5", "n", 0, 5],
    ["verify_read_limit_mb_per_sec", "Verify read limit (MB/s)", "How fast a verify run reads files from disk; 1 to 10000", "n", 1, 10000],
    ["verify_follow_bandwidth_pause", "Pause verify runs with downloads", "A verify run waits while the bandwidth schedule pauses downloads", "b"],
  ],
  advanced: ["verify_max_redownloads", "verify_read_limit_mb_per_sec", "verify_follow_bandwidth_pause"],
};

export const cards = (ctx) => [...[DOWNLOAD, BANDWIDTH, VERIFY].map((g) => groupCard(g, ctx)), ...subtitleCards(ctx), ...translationCards(ctx)];

const LIMIT_KINDS = [
  ["rate", "MB/s"],
  ["unlimited", "Unlimited"],
  ["paused", "Paused"],
];

/**
 * The bandwidth schedule (feature 032): rows of days, start, end and limit, checked in
 * order; the first row containing the daemon's local time wins.
 */
function scheduleEditor(entries) {
  let rows = entries.map(rowFromEntry);
  const body = h("tbody");
  const table = h(
    "table",
    { class: "table override-table schedule-table" },
    h("thead", {}, h("tr", {}, ["Days", "Start", "End", "Limit", ""].map((t) => h("th", { text: t })))),
    body,
  );
  const none = h("p", { class: "muted", text: "No entries: the bandwidth limit above applies all day." });
  const changed = () => table.closest("form")?.dispatchEvent(new Event("change"));
  const input = (row, key, label, placeholder) =>
    h("input", { class: "input", value: row[key], placeholder, "aria-label": label, autocomplete: "off", spellcheck: "false", oninput: (ev) => (row[key] = ev.currentTarget.value) });
  const draw = () => {
    body.replaceChildren(
      ...rows.map((row, i) => {
        const mbs = input(row, "mbs", "Limit in MB/s", "5");
        mbs.type = "number";
        mbs.min = "0.01";
        mbs.step = "any";
        mbs.hidden = row.kind !== "rate";
        const kind = h(
          "select",
          { class: "input", "aria-label": "Limit", onchange: (ev) => ((row.kind = ev.currentTarget.value), (mbs.hidden = row.kind !== "rate")) },
          LIMIT_KINDS.map(([v, t]) => h("option", { value: v, selected: row.kind === v, text: t })),
        );
        const button = (text, label, onclick, disabled = false) => h("button", { type: "button", class: "btn btn-small", "aria-label": label, disabled, onclick }, text);
        const redraw = (next) => () => {
          rows = next();
          draw();
          changed();
        };
        return h(
          "tr",
          {},
          h("td", { "data-label": "Days" }, input(row, "days", "Days", "daily, mon-fri, sat,sun")),
          h("td", { class: "time", "data-label": "Start" }, input(row, "start", "Start", "01:00")),
          h("td", { class: "time", "data-label": "End" }, input(row, "end", "End", "07:00")),
          h("td", { class: "limit", "data-label": "Limit" }, h("div", { class: "toolbar" }, kind, mbs)),
          h(
            "td",
            { class: "actions-cell" },
            button("↑", `Move entry ${i + 1} up`, redraw(() => move(rows, i, -1)), i === 0),
            button("↓", `Move entry ${i + 1} down`, redraw(() => move(rows, i, 1)), i === rows.length - 1),
            button("Remove", `Remove entry ${i + 1}`, redraw(() => rows.filter((_, j) => j !== i))),
          ),
        );
      }),
    );
    table.hidden = !rows.length;
    none.hidden = rows.length > 0;
  };
  draw();
  const add = () => {
    rows = [...rows, { days: "daily", start: "01:00", end: "07:00", kind: "unlimited", mbs: "" }];
    draw();
    changed();
    body.lastChild.querySelector("input").focus();
  };
  const node = h(
    "div",
    { class: "spaced" },
    h("h3", { class: "sub", text: "Bandwidth schedule" }),
    h("p", { class: "muted", text: "In the daemon's local time (see Daemon time on the Status page). The first matching entry wins; an end before the start runs past midnight." }),
    table,
    none,
    h("button", { type: "button", class: "btn btn-small spaced", onclick: add }, "Add entry"),
  );
  return { node, read: () => rows.map(entryFromRow) };
}
