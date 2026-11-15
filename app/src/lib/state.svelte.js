import { call } from "./api.js";

const LAYOUT_KEY = "minediver.layout";
function loadLayout() {
  const base = { side: 240, insp: 380, preview: 300 };
  try {
    return { ...base, ...JSON.parse(localStorage.getItem(LAYOUT_KEY) || "{}") };
  } catch {
    return base;
  }
}

/** Everything the window shares. One object, so components read it directly. */
export const app = $state({
  summary: null,
  loading: false,
  scan: [0, 0],
  error: "",
  settings: null,
  portable: false,
  // query
  text: "",
  cats: [],
  types: [],
  bundles: [],
  include: "",
  exclude: "",
  namedOnly: false,
  minSize: 0,
  sort: { key: "name", desc: false },
  // results
  ids: [],
  rowCache: new Map(),
  /** Checked resources. Kept across searches, so a selection can grow from several. */
  selected: new Set(),
  anchor: -1,
  focus: -1,
  // panels
  layout: loadLayout(),
  big: false,
  info: null,
  dialog: "",
  job: null,
  toast: "",
  modSeed: [],
  modDir: "",
});

export function saveLayout() {
  try {
    localStorage.setItem(LAYOUT_KEY, JSON.stringify($state.snapshot(app.layout)));
  } catch {}
}

let queryToken = 0;

export async function runQuery() {
  if (!app.summary) return;
  const token = ++queryToken;
  const q = {
    text: app.text,
    cats: app.cats,
    types: app.types,
    bundles: app.bundles,
    include: app.include.split(/[,\n]/).map((s) => s.trim()).filter(Boolean),
    exclude: app.exclude.split(/[,\n]/).map((s) => s.trim()).filter(Boolean),
    named_only: app.namedOnly,
    patches: !!app.settings?.show_patches,
    min_size: app.minSize,
  };
  try {
    const focusId = app.ids[app.focus];
    const ids = await call("query", { q, sort: app.sort });
    if (token !== queryToken) return;
    app.ids = ids;
    const at = focusId === undefined ? -1 : ids.indexOf(focusId);
    app.focus = at;
    app.anchor = at;
  } catch (e) {
    app.toast = String(e);
  }
}

const pending = new Set();
/** Rows for ids not yet cached; the table calls this for what is on screen. */
export async function ensureRows(ids) {
  const want = ids.filter((i) => !app.rowCache.has(i) && !pending.has(i));
  if (!want.length) return;
  want.forEach((i) => pending.add(i));
  try {
    const rows = await call("rows", { ids: want });
    const next = new Map(app.rowCache);
    for (const r of rows) next.set(r.id, r);
    app.rowCache = next;
  } finally {
    want.forEach((i) => pending.delete(i));
  }
}

export async function openGame(path) {
  app.loading = true;
  app.error = "";
  try {
    app.summary = await call("open_game", { path: path ?? null });
    app.rowCache = new Map();
    app.selected = new Set();
    app.cats = [];
    app.focus = -1;
    await runQuery();
  } catch (e) {
    app.error = String(e);
  } finally {
    app.loading = false;
  }
}

/** The category tree changed (texture links found, or names added). */
export function setCategories(cats) {
  if (!app.summary) return;
  app.summary = { ...app.summary, cats };
  app.rowCache = new Map();
  if (app.cats.length) runQuery();
}

export async function refreshCategories() {
  try {
    setCategories(await call("categories"));
  } catch {}
}

export async function saveSettings(patch) {
  app.settings = { ...app.settings, ...patch };
  await call("set_settings", { settings: app.settings });
}

/** What an action works on: the checked resources (shown ones first, in table order), or the focused row. */
export function selectedIds() {
  if (app.selected.size) {
    const shown = app.ids.filter((i) => app.selected.has(i));
    if (shown.length === app.selected.size) return shown;
    const seen = new Set(shown);
    return [...shown, ...[...app.selected].filter((i) => !seen.has(i))];
  }
  if (app.focus >= 0 && app.ids[app.focus] !== undefined) return [app.ids[app.focus]];
  return [];
}

export function focusedId() {
  return app.focus >= 0 ? app.ids[app.focus] : undefined;
}

export function hiddenSelected() {
  if (!app.selected.size) return 0;
  let shown = 0;
  for (const i of app.ids) if (app.selected.has(i)) shown++;
  return app.selected.size - shown;
}
