import { call } from "./api.js";

/** Everything the window shares. One object, so components read it directly. */
export const app = $state({
  summary: null,
  loading: false,
  scan: [0, 0],
  error: "",
  settings: null,
  // query
  text: "",
  kinds: [],
  types: [],
  bundles: [],
  include: "",
  exclude: "",
  namedOnly: false,
  sort: { key: "name", desc: false },
  // results
  ids: [],
  rowCache: new Map(),
  selected: new Set(),
  anchor: -1,
  focus: -1,
  // panels
  sidebar: true,
  inspector: true,
  dialog: "",
  job: null,
  toast: "",
  modSeed: [],
  modDir: "",
});

let queryToken = 0;

export async function runQuery() {
  if (!app.summary) return;
  const token = ++queryToken;
  const q = {
    text: app.text,
    kinds: app.kinds,
    types: app.types,
    bundles: app.bundles,
    include: app.include.split(/[,\n]/).map((s) => s.trim()).filter(Boolean),
    exclude: app.exclude.split(/[,\n]/).map((s) => s.trim()).filter(Boolean),
    named_only: app.namedOnly,
    patches: !!app.settings?.show_patches,
  };
  try {
    const ids = await call("query", { q, sort: app.sort });
    if (token !== queryToken) return;
    app.ids = ids;
    app.selected = new Set([...app.selected].filter((i) => ids.includes(i)));
    if (app.focus >= ids.length) app.focus = ids.length - 1;
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
    app.focus = -1;
    await runQuery();
  } catch (e) {
    app.error = String(e);
  } finally {
    app.loading = false;
  }
}

export async function saveSettings(patch) {
  app.settings = { ...app.settings, ...patch };
  await call("set_settings", { settings: app.settings });
}

export function selectedIds() {
  if (app.selected.size) return app.ids.filter((i) => app.selected.has(i));
  if (app.focus >= 0 && app.ids[app.focus] !== undefined) return [app.ids[app.focus]];
  return [];
}

export function focusedId() {
  return app.focus >= 0 ? app.ids[app.focus] : undefined;
}
