// Stats page: totals, bandwidth over time, an hour × weekday heatmap, file types,
// origins and sources from GET /stats/summary. Filters live in the hash query, so a
// reload or a shared link keeps them. Loads on filter change and Refresh only.

import { get } from "../api.js";
import { h } from "../app.js";
import { bytes, duration, speed } from "../format.js";

const RANGES = { "24h": "Last 24 hours", "7d": "Last 7 days", "30d": "Last 30 days", "90d": "Last 90 days", "1y": "Last year", all: "All time" };
const DEFAULT_RANGE = "7d";
const CATEGORIES = { video: "Video", audio: "Audio", archive: "Archive", image: "Image", document: "Document", subtitle: "Subtitle", other: "Other" };
const SOURCES = { torrent: "Torrent", url: "URL", other: "Other", unknown: "Unknown" };
const ORIGINS = { watch: "Watch folder", manual: "Added in putio-sync", putio: "Added on Put.io", unknown: "Unknown" };
const PRIVACY = { private: "Private", public: "Public", unknown: "Unknown" };
/** Hash keys, in the order they are written */
const KEYS = ["range", "cat", "ext", "source", "tracker", "private", "origin", "watch", "folder"];
const DOTS = "…other";
const PLOT_H = 176;
const INSET = 8;
const SVG_NS = "http://www.w3.org/2000/svg";
const DAY_MS = 86400000;

let f = {};
let els = null;
let data = null;
let seq = 0;
let sel = { bar: null, cell: null, heat: "bytes" };
let resize = null;

export function mount(el, params = {}) {
  const q = params.query || new URLSearchParams();
  f = Object.fromEntries(KEYS.map((k) => [k, q.get(k) || ""]));
  if (!RANGES[f.range]) f.range = DEFAULT_RANGE;
  data = null;
  sel = { bar: null, cell: null, heat: "bytes" };

  els = {
    summary: h("p", { class: "muted" }),
    filters: h("div", { class: "toolbar filters", role: "group", "aria-label": "Filters" }),
    body: h("div", { class: "stats" }, h("p", { class: "muted", text: "Loading…" })),
  };
  el.append(
    h(
      "div",
      { class: "page-head" },
      h("div", {}, h("h1", { text: "Stats" }), els.summary),
      h("button", { type: "button", class: "btn", onclick: refresh }, "Refresh"),
    ),
    els.filters,
    els.body,
  );
  renderFilters();
  refresh();
  resize = new ResizeObserver(() => data && els && els.plot && drawBars());
  resize.observe(els.body);
}

export function unmount() {
  if (resize) resize.disconnect();
  resize = null;
  els = null;
  data = null;
  seq += 1;
}

// ---- Filters ----------------------------------------------------------------

function set(changes) {
  Object.assign(f, changes);
  const q = new URLSearchParams();
  for (const k of KEYS) if (f[k] && !(k === "range" && f[k] === DEFAULT_RANGE)) q.set(k, f[k]);
  const query = q.toString();
  history.replaceState(null, "", `#/stats${query ? `?${query}` : ""}`);
  sel.bar = null;
  sel.cell = null;
  renderFilters();
  refresh();
}

/** The value of the combined type select: "cat:video", "ext:mkv" or "" */
const typeValue = () => (f.ext ? `ext:${f.ext}` : f.cat ? `cat:${f.cat}` : "");
const originValue = () => (f.watch ? `watch:${f.watch}` : f.origin);

function select(label, value, options, onchange, hidden = false) {
  const list = [...options];
  if (value && !list.some(([v]) => v === value)) list.push([value, value.replace(/^\w+:/, "")]);
  return h(
    "select",
    { class: "input", "aria-label": label, hidden: hidden && !value, onchange: (e) => onchange(e.target.value) },
    list.map(([v, text]) => h("option", { value: v, text, selected: v === value })),
  );
}

function renderFilters() {
  if (!els) return;
  const opts = (data && data.options) || { extensions: [], trackers: [], watch_folders: [], folders: [] };
  const range = h(
    "div",
    { class: "seg", role: "group", "aria-label": "Range" },
    Object.keys(RANGES).map((r) => h("button", { type: "button", class: "btn btn-small", "aria-pressed": String(f.range === r), onclick: () => set({ range: r }) }, r === "all" ? "All" : r)),
  );
  const types = [["", "All types"], ...Object.entries(CATEGORIES).map(([k, v]) => [`cat:${k}`, v]), ...opts.extensions.map((e) => [`ext:${e}`, `.${e}`])];
  const origins = [["", "All origins"], ["watch", ORIGINS.watch], ...opts.watch_folders.map((w) => [`watch:${w}`, ` ${w}`]), ["manual", ORIGINS.manual], ["putio", ORIGINS.putio], ["unknown", ORIGINS.unknown]];
  const filtered = KEYS.some((k) => k !== "range" && f[k]);
  els.filters.replaceChildren(
    range,
    select("Type", typeValue(), types, (v) => set({ cat: v.startsWith("cat:") ? v.slice(4) : "", ext: v.startsWith("ext:") ? v.slice(4) : "" })),
    select("Source", f.source, [["", "All sources"], ...Object.entries(SOURCES)], (v) => set({ source: v })),
    select("Tracker", f.tracker, [["", "All trackers"], ...opts.trackers.map((t) => [t, t])], (v) => set({ tracker: v }), !opts.trackers.length),
    select("Privacy", f.private, [["", "Public and private"], ["true", "Private"], ["false", "Public"]], (v) => set({ private: v })),
    select("Origin", originValue(), origins, (v) => set(v.startsWith("watch:") ? { origin: "watch", watch: v.slice(6) } : { origin: v, watch: "" })),
    select("Put.io folder", f.folder, [["", "All folders"], ...opts.folders.map((d) => [d, d])], (v) => set({ folder: v }), !opts.folders.length),
    h("button", { type: "button", class: "btn btn-small btn-quiet", hidden: !filtered, onclick: () => set(Object.fromEntries(KEYS.slice(1).map((k) => [k, ""]))) }, "Clear"),
  );
  const parts = [RANGES[f.range]];
  if (f.ext) parts.push(`.${f.ext}`);
  else if (f.cat) parts.push(CATEGORIES[f.cat] || f.cat);
  if (f.source) parts.push(SOURCES[f.source] || f.source);
  if (f.tracker) parts.push(f.tracker);
  if (f.private) parts.push(f.private === "true" ? "Private" : "Public");
  if (f.watch) parts.push(f.watch);
  else if (f.origin) parts.push(ORIGINS[f.origin] || f.origin);
  if (f.folder) parts.push(`Put.io: ${f.folder}`);
  els.summary.textContent = parts.join(" · ");
}

// ---- Data -------------------------------------------------------------------

function refresh() {
  load().catch(() => {});
}

async function load() {
  const mine = ++seq;
  const q = new URLSearchParams({ range: f.range, tz: String(-new Date().getTimezoneOffset()) });
  const api = { cat: "category", ext: "ext", source: "source", tracker: "tracker", private: "private", origin: "origin", watch: "watch", folder: "folder" };
  for (const [k, p] of Object.entries(api)) if (f[k]) q.set(p, f[k]);
  els.body.classList.add("loading");
  let res;
  try {
    res = await get(`/stats/summary?${q}`);
  } catch (e) {
    if (mine !== seq || !els) return;
    els.body.classList.remove("loading");
    els.plot = null;
    els.body.replaceChildren(
      h(
        "section",
        { class: "card" },
        h("h2", { text: "Can't load stats" }),
        h("p", { class: "error-text", text: e.details ? `${e.error}: ${e.details}` : e.error }),
        h("button", { type: "button", class: "btn spaced", onclick: refresh }, "Retry"),
      ),
    );
    throw e;
  }
  if (mine !== seq || !els) return;
  data = res;
  els.body.classList.remove("loading");
  renderFilters();
  render();
}

// ---- Rendering --------------------------------------------------------------

const count = (n) => Number(n || 0).toLocaleString();

function render() {
  const t = data.totals;
  const empty = !t.files;
  const none = KEYS.some((k) => k !== "range" && f[k]) ? "Nothing matches the filters." : "Nothing in this range.";
  const card = (title, ...body) => h("section", { class: "card" }, h("h2", { text: title }), empty ? h("p", { class: "muted", text: none }) : body);
  const tile = (label, value) => h("div", { class: "card tile" }, h("p", { class: "muted", text: label }), h("p", { class: "big", text: value }));

  els.plot = null;
  els.body.replaceChildren(
    h("div", { class: "tiles" }, tile("Files", count(t.files)), tile("Downloaded", bytes(t.bytes)), tile("Time downloading", t.seconds ? duration(t.seconds) : "—"), tile("Average speed", t.avg_bps ? speed(t.avg_bps) : "—")),
    h("div", { class: "stats-grid" }, card("Bandwidth", empty ? null : bandwidth()), card("When", empty ? null : heatmap()), card("File types", empty ? null : types()), card("Origin", empty ? null : origin()), card("Sources", empty ? null : sources())),
  );
  if (els.plot) drawBars();
}

// ---- Bandwidth --------------------------------------------------------------

function svg(tag, attrs = {}) {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  return el;
}

const dayFmt = (d) => d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
const timeFmt = (d) => d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
const dateFmt = (d) => d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });

/** "Tue 21 Oct, 18:00–00:00", "Tue 21 Oct" or "20 Oct 2026 – 26 Oct 2026" */
function bucketLabel(start, secs) {
  const a = new Date(start * 1000);
  if (secs < 86400) return `${dayFmt(a)}, ${timeFmt(a)}–${timeFmt(new Date((start + secs) * 1000))}`;
  if (secs === 86400) return dayFmt(a);
  return `${dateFmt(a)} – ${dateFmt(new Date((start + secs) * 1000 - DAY_MS))}`;
}

function axisLabel(start, secs) {
  const d = new Date(start * 1000);
  return secs < 86400 ? `${dayFmt(d)} ${timeFmt(d)}` : secs < 604800 ? dayFmt(d) : dateFmt(d);
}

function bandwidth() {
  const s = data.series;
  const secs = data.range.bucket_seconds;
  const max = Math.max(0, ...s.map((p) => p.bytes));
  if (sel.bar == null || sel.bar >= s.length) sel.bar = s.findIndex((p) => p.bytes === max);
  const mid = s[Math.floor((s.length - 1) / 2)];
  els.plot = h("div", {
    class: "plot",
    tabindex: "0",
    role: "group",
    "aria-label": `Bandwidth per ${data.range.bucket} over ${s.length} periods; peak ${bytes(max)} on ${bucketLabel(s[sel.bar].start, secs)}. Arrow keys move between bars.`,
    onpointermove: pickBar,
    onpointerdown: pickBar,
    onkeydown: (e) => stepKey(e, (d) => moveBar(d[0])),
  });
  els.barReadout = h("p", { class: "readout num", "aria-live": "polite" });
  return [
    h(
      "div",
      { class: "chart" },
      h("div", { class: "ylabels num muted", "aria-hidden": "true" }, h("span", { text: bytes(max) }), h("span", { text: bytes(max / 2) }), h("span", { text: "0" })),
      els.plot,
      h("div", {}),
      h("div", { class: "xlabels num muted", "aria-hidden": "true" }, h("span", { text: axisLabel(s[0].start, secs) }), h("span", { text: axisLabel(mid.start, secs) }), h("span", { text: axisLabel(s[s.length - 1].start, secs) })),
    ),
    els.barReadout,
  ];
}

/** Bars with a rounded data end and a square baseline */
function barPath(x, y, w, base) {
  const r = Math.min(4, w / 2, base - y);
  return `M${x},${base}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${base}Z`;
}

function drawBars() {
  const s = data.series;
  const n = s.length;
  const width = Math.max(1, els.plot.clientWidth);
  const max = Math.max(1, ...s.map((p) => p.bytes));
  const step = width / n;
  const gap = step >= 6 ? 2 : step >= 3 ? 1 : 0;
  const w = Math.min(24, step - gap);
  const base = PLOT_H - INSET;
  const span = base - INSET;
  const root = svg("svg", { width, height: PLOT_H, viewBox: `0 0 ${width} ${PLOT_H}`, "aria-hidden": "true" });
  for (const y of [INSET, INSET + span / 2]) root.append(svg("line", { class: "grid", x1: 0, x2: width, y1: y + 0.5, y2: y + 0.5 }));
  if (sel.bar != null) root.append(svg("rect", { class: "band", x: sel.bar * step, y: 0, width: step, height: PLOT_H }));
  s.forEach((p, i) => {
    if (!p.bytes) return;
    const y = base - Math.max(1, (p.bytes / max) * span);
    root.append(svg("path", { class: i === sel.bar ? "mark on" : "mark", d: barPath(i * step + (step - w) / 2, y, w, base) }));
  });
  root.append(svg("line", { class: "axis", x1: 0, x2: width, y1: base + 0.5, y2: base + 0.5 }));
  els.plot.replaceChildren(root);
  barReadout();
}

function barReadout() {
  const p = data.series[sel.bar];
  if (!p) return;
  const secs = data.range.bucket_seconds;
  els.barReadout.textContent = `${bucketLabel(p.start, secs)} · ${bytes(p.bytes)} · ${count(p.files)} ${p.files === 1 ? "file" : "files"} · ${speed(p.bytes / secs)} avg`;
}

function pickBar(e) {
  const rect = els.plot.getBoundingClientRect();
  const n = data.series.length;
  const i = Math.min(n - 1, Math.max(0, Math.floor(((e.clientX - rect.left) / rect.width) * n)));
  if (i !== sel.bar) {
    sel.bar = i;
    drawBars();
  }
}

function moveBar(d) {
  sel.bar = Math.min(data.series.length - 1, Math.max(0, sel.bar + d));
  drawBars();
}

/** Call `fn([dx, dy])` for an arrow key and swallow the key */
function stepKey(e, fn) {
  const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key];
  if (!d) return;
  e.preventDefault();
  fn(d);
}

// ---- Heatmap ----------------------------------------------------------------

/** Monday-first day name; 1 Jan 2024 was a Monday */
const dayName = (d, weekday) => new Date(Date.UTC(2024, 0, 1 + d)).toLocaleDateString(undefined, { weekday, timeZone: "UTC" });

function heatmap() {
  const wrap = h("div", {});
  const draw = () => {
    const m = data.heatmap[sel.heat];
    const max = Math.max(0, ...m.flat());
    if (!sel.cell) {
      let best = [0, 0];
      m.forEach((row, d) => row.forEach((v, hr) => v > m[best[0]][best[1]] && (best = [d, hr])));
      sel.cell = best;
    }
    const value = (v) => (sel.heat === "bytes" ? bytes(v) : count(v));
    const level = (v) => (v > 0 ? Math.min(5, Math.ceil((v / max) * 5)) : 0);
    const readout = h("p", { class: "readout num", "aria-live": "polite" });
    const show = () => {
      const [d, hr] = sel.cell;
      readout.textContent = `${dayName(d, "long")} ${String(hr).padStart(2, "0")}:00 · ${count(data.heatmap.files[d][hr])} files · ${bytes(data.heatmap.bytes[d][hr])}`;
      for (const c of grid.querySelectorAll(".on")) c.classList.remove("on");
      grid.querySelector(`[data-cell="${d}-${hr}"]`).classList.add("on");
    };
    const pick = (d, hr) => {
      sel.cell = [d, hr];
      show();
    };
    const grid = h(
      "div",
      {
        class: "heat",
        tabindex: "0",
        role: "group",
        "aria-label": `Downloads by local hour and weekday, by ${sel.heat}. Arrow keys move between cells.`,
        onkeydown: (e) => stepKey(e, ([dx, dy]) => pick(Math.min(6, Math.max(0, sel.cell[0] + dy)), Math.min(23, Math.max(0, sel.cell[1] + dx)))),
      },
      h("span", {}),
      Array.from({ length: 24 }, (_, hr) => h("span", { class: "hour muted", text: hr % 6 ? "" : String(hr) })),
      m.map((row, d) => [
        h("span", { class: "day muted", text: dayName(d, "short") }),
        row.map((v, hr) => h("span", { class: `cell l${level(v)}`, dataset: { cell: `${d}-${hr}` }, onpointerenter: () => pick(d, hr), onpointerdown: () => pick(d, hr) })),
      ]),
    );
    const toggle = h(
      "div",
      { class: "seg", role: "group", "aria-label": "Measure" },
      ["bytes", "files"].map((k) => h("button", { type: "button", class: "btn btn-small", "aria-pressed": String(sel.heat === k), onclick: () => ((sel.heat = k), draw()) }, k === "bytes" ? "Bytes" : "Files")),
    );
    const legend = h("div", { class: "legend muted num", "aria-hidden": "true" }, h("span", { text: "0" }), [1, 2, 3, 4, 5].map((l) => h("i", { class: `cell l${l}` })), h("span", { text: value(max) }));
    wrap.replaceChildren(h("div", { class: "card-tools" }, toggle, legend), grid, readout);
    show();
  };
  draw();
  return wrap;
}

// ---- Bar lists --------------------------------------------------------------

/**
 * Rows of `{ label, files, bytes, sub?, active?, pick? }` with a bar for each row's share
 * of `total` bytes. A row with `pick` is a button that sets a filter.
 */
function barList(title, rows, total) {
  if (!rows.length) return null;
  return h(
    "div",
    { class: "blist" },
    title ? h("h3", { text: title }) : null,
    rows.map((r) => {
      const inner = [
        h("span", { class: `blabel name${r.indent ? " indent" : ""}`, text: r.label }),
        h("span", { class: "num muted right", text: `${count(r.files)} · ${bytes(r.bytes)}` }),
        h("span", { class: "bar" }, h("span", { style: `width:${total ? Math.max(0.5, (r.bytes / total) * 100) : 0}%` })),
        r.sub ? h("small", { class: "muted", text: r.sub }) : null,
      ];
      return r.pick
        ? h("button", { type: "button", class: "brow", "aria-pressed": String(Boolean(r.active)), onclick: r.pick }, inner)
        : h("div", { class: "brow" }, inner);
    }),
  );
}

const fold = (k, labels) => labels[k] || k;

function types() {
  const total = data.totals.bytes;
  return barList(
    null,
    data.types.map((t) => ({
      label: fold(t.category, CATEGORIES),
      files: t.files,
      bytes: t.bytes,
      sub: t.extensions.map((e) => `${e.ext === DOTS ? e.ext : `.${e.ext}`} ${count(e.files)}`).join(" · "),
      active: f.cat === t.category && !f.ext,
      pick: () => set(f.cat === t.category && !f.ext ? { cat: "", ext: "" } : { cat: t.category, ext: "" }),
    })),
    total,
  );
}

function origin() {
  const total = data.totals.bytes;
  const s = data.sources;
  const rows = [];
  for (const o of s.origins) {
    rows.push({ label: fold(o.key, ORIGINS), files: o.files, bytes: o.bytes, active: f.origin === o.key && !f.watch, pick: () => set({ origin: f.origin === o.key && !f.watch ? "" : o.key, watch: "" }) });
    if (o.key === "watch") {
      for (const w of s.watch_folders) {
        rows.push({ label: w.key, indent: true, files: w.files, bytes: w.bytes, active: f.watch === w.key, pick: () => set(f.watch === w.key ? { origin: "", watch: "" } : { origin: "watch", watch: w.key }) });
      }
    }
  }
  const folders = s.folders.map((d) => ({
    label: d.key,
    files: d.files,
    bytes: d.bytes,
    active: f.folder === d.key,
    pick: d.key === "other folders" || d.key === "unknown" ? null : () => set({ folder: f.folder === d.key ? "" : d.key }),
  }));
  return [barList("How it was added", rows, total), barList("Put.io folder", folders, total)];
}

function sources() {
  const total = data.totals.bytes;
  const s = data.sources;
  const typeRows = s.types.map((t) => ({ label: fold(t.key, SOURCES), files: t.files, bytes: t.bytes, active: f.source === t.key, pick: () => set({ source: f.source === t.key ? "" : t.key }) }));
  const trackerRows = s.trackers.map((t) => ({
    label: t.key,
    files: t.files,
    bytes: t.bytes,
    active: f.tracker === t.key,
    pick: t.key === "other trackers" || t.key === "no tracker" ? null : () => set({ tracker: f.tracker === t.key ? "" : t.key }),
  }));
  const privacyRows = s.privacy.map((p) => {
    const value = { private: "true", public: "false" }[p.key];
    return { label: fold(p.key, PRIVACY), files: p.files, bytes: p.bytes, active: Boolean(value) && f.private === value, pick: value ? () => set({ private: f.private === value ? "" : value }) : null };
  });
  return [
    barList("Type", typeRows, total),
    barList("Tracker", trackerRows, total),
    barList("Privacy", privacyRows, total),
    h("p", { class: "muted note", text: "Recorded for downloads queued since this version; earlier ones are unknown. Put.io doesn't report DHT; private torrents don't use it." }),
  ];
}
