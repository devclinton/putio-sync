// Feature 070: the logic of the file picker of one transfer. The API lists the files by
// path (`GET /putio/transfers/:id/files`, or the preview of a `.torrent`); this builds the
// tree of folders, keeps which files are ticked and turns the ticks into the choice the
// API stores. Plain functions with no DOM, so tests/ui/pick.test.mjs can run them under
// `node --test`. The dialog is pick_dialog.js.

const byName = (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });

/**
 * The files as a tree: `{ name, path, dirs, files }`, folders and files by name. A file
 * is `{ name, path, size, state }`; the root has an empty name and path.
 */
export function tree(files) {
  const root = { name: "", path: "", dirs: [], files: [] };
  const dirs = new Map([["", root]]);
  for (const f of files || []) {
    const parts = String(f.path).split("/");
    const name = parts.pop();
    let at = root;
    let path = "";
    for (const part of parts) {
      path = path ? `${path}/${part}` : part;
      let dir = dirs.get(path);
      if (!dir) {
        dir = { name: part, path, dirs: [], files: [] };
        dirs.set(path, dir);
        at.dirs.push(dir);
      }
      at = dir;
    }
    at.files.push({ name, path: f.path, size: Number(f.size) || 0, state: f.state || "none" });
  }
  for (const dir of dirs.values()) {
    dir.dirs.sort(byName);
    dir.files.sort(byName);
  }
  return root;
}

/** The paths of every file in a folder of the tree, at any depth */
export function under(dir) {
  return [...dir.files.map((f) => f.path), ...dir.dirs.flatMap(under)];
}

/**
 * The ticks to start from, path → ticked: what the API says of each file, or every file
 * when nothing was picked yet (`fresh`: no choice, or one still waiting to be made).
 */
export function ticksOf(files, fresh = false) {
  return new Map((files || []).map((f) => [f.path, fresh || f.wanted !== false]));
}

/** "all", "none" or "some" of a folder's files are ticked; "none" for an empty folder */
export function folderState(dir, ticks) {
  const paths = under(dir);
  const on = paths.filter((p) => ticks.get(p)).length;
  return on === 0 ? "none" : on === paths.length ? "all" : "some";
}

/** Tick or untick every file in a folder */
export function setFolder(ticks, dir, on) {
  const next = new Map(ticks);
  for (const p of under(dir)) next.set(p, on);
  return next;
}

/** Tick or untick every file */
export function setAll(ticks, on) {
  return new Map([...ticks.keys()].map((p) => [p, on]));
}

/** Only this file: it is ticked and nothing else is */
export function onlyThis(ticks, path) {
  return new Map([...ticks.keys()].map((p) => [p, p === path]));
}

/** All but this file: everything is ticked except it */
export function allBut(ticks, path) {
  return new Map([...ticks.keys()].map((p) => [p, p !== path]));
}

/** `{ count, total, size, totalSize }` of the ticked files, sizes in bytes */
export function summary(files, ticks) {
  const out = { count: 0, total: 0, size: 0, totalSize: 0 };
  for (const f of files || []) {
    const size = Number(f.size) || 0;
    out.total += 1;
    out.totalSize += size;
    if (ticks.get(f.path)) {
      out.count += 1;
      out.size += size;
    }
  }
  return out;
}

/**
 * The body of `PUT /putio/transfers/:id/choice`: every listed file by name, so the choice
 * holds whatever the rules say, and `later` for files that show up afterwards.
 */
export function toChoice(files, ticks, later) {
  return {
    others: later ? "download" : "skip",
    files: (files || []).map((f) => ({ path: f.path, wanted: ticks.get(f.path) === true })),
  };
}

/**
 * What to say beside a file about its download here, or null: unticking a file that is
 * already here or on its way does not take it back.
 */
export function stateNote(state, ticked) {
  switch (state) {
    case "done":
      return ticked ? "Downloaded" : "Already downloaded; it stays on the NAS";
    case "downloading":
      return ticked ? "Downloading" : "Downloading now; it will finish";
    case "pending":
    case "paused":
    case "failed":
      return ticked ? "In the queue" : "In the queue; saving takes it out";
    default:
      return null;
  }
}
