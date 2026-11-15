//! The Tauri shell: commands the UI calls, the `md://` preview protocol, and background extraction.
//! Parsing lives in `minediver-core`; this file only wires it to the window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use minediver::archive::{self, Archive};
use minediver::category::Categories;
use minediver::export::{self, Exporter, Kind, Options};
use minediver::filter::{Filter, Query};
use minediver::hash::{self, NameDb};
use minediver::links::Linker;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

struct Loaded {
    ar: Archive,
    linker: Linker,
    /// Rebuilt when names are added, since most categories come from names.
    cats: RwLock<Arc<Categories>>,
}

impl Loaded {
    fn cats(&self) -> Arc<Categories> {
        self.cats.read().unwrap().clone()
    }
}

struct App {
    loaded: RwLock<Option<Arc<Loaded>>>,
    names: Arc<NameDb>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    settings: Mutex<Settings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    game_dir: Option<String>,
    output_dir: Option<String>,
    ffmpeg: Option<String>,
    theme: String,
    options: Options,
    name_lists: Vec<String>,
    show_patches: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { game_dir: None, output_dir: None, ffmpeg: None, theme: "dark".into(), options: Options::default(), name_lists: vec![], show_patches: false }
    }
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The data folder of a portable copy (a `portable.txt` beside the exe): `data` beside the exe. None for an
/// installed copy, which keeps the settings in the user's app config folder.
fn portable_data() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable.txt").is_file().then(|| dir.join("data"))
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    match portable_data() {
        Some(d) => Some(d.join("settings.json")),
        None => app.path().app_config_dir().ok().map(|d| d.join("settings.json")),
    }
}

/// A portable copy updates by downloading the new zip, not through the installer.
#[tauri::command]
fn is_portable() -> bool {
    portable_data().is_some()
}

fn save_settings(app: &AppHandle, s: &Settings) {
    if let Some(p) = settings_path(app) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(s).unwrap_or_default());
    }
}

fn loaded(state: &App) -> Res<Arc<Loaded>> {
    state.loaded.read().unwrap().clone().ok_or_else(|| "No game folder is open.".to_string())
}

#[tauri::command]
fn get_settings(state: State<App>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<App>, settings: Settings) {
    for l in &settings.name_lists {
        let _ = state.names.add_file(std::path::Path::new(l));
    }
    *state.settings.lock().unwrap() = settings.clone();
    save_settings(&app, &settings);
}

#[derive(Serialize)]
struct Summary {
    contents: String,
    total: usize,
    named: usize,
    kinds: Vec<(Kind, usize)>,
    types: Vec<(String, usize)>,
    bundles: Vec<(String, Option<String>, usize)>,
    errors: Vec<(String, String)>,
    ms: u128,
    ffmpeg: Option<String>,
    cats: Vec<(Kind, String, usize)>,
}

fn summary(l: &Loaded, names: &NameDb, ms: u128, ffmpeg: Option<String>) -> Summary {
    use std::collections::BTreeMap;
    let mut kinds: BTreeMap<String, (Kind, usize)> = BTreeMap::new();
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut bundles: BTreeMap<String, usize> = BTreeMap::new();
    let mut named = 0;
    for a in l.ar.assets.iter().filter(|a| a.patch.is_none()) {
        let k = export::kind_of(a.type_hash);
        kinds.entry(format!("{k:?}")).or_insert((k, 0)).1 += 1;
        *types.entry(hash::type_label(a.type_hash)).or_default() += 1;
        *bundles.entry(hash::hex(a.bundle)).or_default() += 1;
        named += names.contains(a.name_hash) as usize;
    }
    let order = [Kind::Model, Kind::Texture, Kind::Audio, Kind::Video, Kind::Script, Kind::Text, Kind::Material, Kind::Animation, Kind::Level, Kind::Other];
    let mut kinds: Vec<(Kind, usize)> = kinds.into_values().collect();
    kinds.sort_by_key(|(k, _)| order.iter().position(|o| o == k));
    let mut types: Vec<_> = types.into_iter().collect();
    types.sort_by(|a, b| b.1.cmp(&a.1));
    Summary {
        contents: l.ar.contents.display().to_string(),
        total: l.ar.assets.iter().filter(|a| a.patch.is_none()).count(),
        named,
        kinds,
        types,
        bundles: bundles.into_iter().map(|(b, n)| (b.clone(), u64::from_str_radix(&b, 16).ok().and_then(|h| names.get(h)), n)).collect(),
        errors: l.ar.errors.clone(),
        ms,
        ffmpeg,
        cats: l.cats().counts(&l.ar),
    }
}

/// Textures take the category of the model that uses them, which needs every named material read. That
/// runs in the background; the finished tree arrives as a `categories` event.
fn fill_texture_categories(app: &AppHandle, l: Arc<Loaded>) {
    let app = app.clone();
    std::thread::spawn(move || {
        let names = app.state::<App>().names.clone();
        let c = Arc::new(Categories::build(&l.ar, &names, &l.linker));
        // A game folder opened since then has its own tree.
        let current = app.state::<App>().loaded.read().unwrap().as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &l));
        if current {
            *l.cats.write().unwrap() = c.clone();
            let _ = app.emit("categories", c.counts(&l.ar));
        }
    });
}

/// The category tree again, after names were added: from names at once, textures later by event.
#[tauri::command]
fn categories(app: AppHandle, state: State<App>) -> Res<Vec<(Kind, String, usize)>> {
    let l = loaded(&state)?;
    let c = Arc::new(Categories::from_names(&l.ar, &state.names));
    *l.cats.write().unwrap() = c.clone();
    fill_texture_categories(&app, l.clone());
    Ok(c.counts(&l.ar))
}

/// Opens a game folder (or finds one through Steam) and indexes every bundle.
#[tauri::command]
async fn open_game(app: AppHandle, path: Option<String>) -> Res<Summary> {
    let state = app.state::<App>();
    let dir = match path.or_else(|| state.settings.lock().unwrap().game_dir.clone()) {
        Some(p) => PathBuf::from(p),
        None => archive::detect_game().ok_or("Helldivers was not found in the Steam libraries. Pick the game folder.")?,
    };
    let contents = archive::resolve_contents(&dir).ok_or_else(|| format!("No game bundles in {}. Pick the Helldivers folder (the one with contents inside).", dir.display()))?;
    let t = std::time::Instant::now();
    let app2 = app.clone();
    let names = state.names.clone();
    let l = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Loaded> {
        let ar = Archive::scan(&contents, &|n, total| {
            let _ = app2.emit("scan", (n, total));
        })?;
        let linker = Linker::new(&ar);
        let cats = Categories::from_names(&ar, &names);
        Ok(Loaded { ar, linker, cats: RwLock::new(Arc::new(cats)) })
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let l = Arc::new(l);
    *state.loaded.write().unwrap() = Some(l.clone());
    fill_texture_categories(&app, l.clone());
    let ffmpeg = {
        let mut s = state.settings.lock().unwrap();
        s.game_dir = Some(dir.display().to_string());
        save_settings(&app, &s);
        export::find_ffmpeg(s.ffmpeg.as_deref().map(std::path::Path::new)).map(|p| p.display().to_string())
    };
    Ok(summary(&l, &state.names, t.elapsed().as_millis(), ffmpeg))
}

#[derive(Deserialize)]
struct Sort {
    key: String,
    desc: bool,
}

/// The ids of every resource the query picks, sorted. The table asks for rows by id as it scrolls.
#[tauri::command]
fn query(state: State<App>, q: Query, sort: Sort) -> Res<Vec<u32>> {
    let l = loaded(&state)?;
    let f = Filter::new(&q).map_err(err)?;
    let names = &state.names;
    let cats = l.cats();
    let mut picked: Vec<&archive::Asset> = l.ar.assets.par_iter().filter(|a| f.matches_in(names, a, cats.label(a.id))).collect();
    match sort.key.as_str() {
        "size" => picked.par_sort_by_key(|a| a.size()),
        "type" => picked.par_sort_by_cached_key(|a| (hash::type_label(a.type_hash), export::display_name(names, a))),
        "bundle" => picked.par_sort_by_key(|a| (a.bundle, a.patch)),
        "category" => picked.par_sort_by_cached_key(|a| {
            let c = cats.label(a.id);
            (c == minediver::category::UNNAMED, c.to_string(), export::display_name(names, a))
        }),
        _ => picked.par_sort_by_cached_key(|a| {
            let n = names.get(a.name_hash);
            (n.is_none(), n.unwrap_or_else(|| hash::hex(a.name_hash)), hash::type_label(a.type_hash))
        }),
    }
    if sort.desc {
        picked.reverse();
    }
    Ok(picked.iter().map(|a| a.id).collect())
}

#[derive(Serialize)]
struct Row {
    id: u32,
    name: String,
    named: bool,
    r#type: String,
    kind: Kind,
    bundle: String,
    size: u64,
    variants: usize,
    patch: bool,
    category: String,
    hash: String,
}

#[tauri::command]
fn rows(state: State<App>, ids: Vec<u32>) -> Res<Vec<Row>> {
    let l = loaded(&state)?;
    let cats = l.cats();
    Ok(ids
        .iter()
        .filter_map(|&i| l.ar.assets.get(i as usize))
        .map(|a| {
            let n = state.names.get(a.name_hash);
            Row {
                id: a.id,
                named: n.is_some(),
                name: n.unwrap_or_else(|| hash::hex(a.name_hash)),
                r#type: hash::type_label(a.type_hash),
                kind: export::kind_of(a.type_hash),
                bundle: a.file_name(),
                size: a.size(),
                variants: a.variants.len(),
                patch: a.patch.is_some(),
                category: cats.label(a.id).to_string(),
                hash: hash::hex(a.name_hash),
            }
        })
        .collect())
}

/// Writes the listed resources as a tab-separated table: name, hash, type, category, size, bundle.
#[tauri::command]
fn export_list(state: State<App>, ids: Vec<u32>, path: String) -> Res<usize> {
    let l = loaded(&state)?;
    let cats = l.cats();
    let mut out = String::from("name\thash\ttype\tcategory\tsize\tbundle\n");
    let mut n = 0;
    for a in ids.iter().filter_map(|&i| l.ar.assets.get(i as usize)) {
        out += &format!("{}\t{:016x}\t{}\t{}\t{}\t{}\n", export::display_name(&state.names, a), a.name_hash, hash::type_label(a.type_hash), cats.label(a.id), a.size(), a.file_name());
        n += 1;
    }
    std::fs::write(&path, out).map_err(err)?;
    Ok(n)
}

#[tauri::command]
async fn inspect(app: AppHandle, id: u32) -> Res<Value> {
    let state = app.state::<App>();
    let l = loaded(&state)?;
    let names = state.names.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let a = l.ar.assets.get(id as usize).ok_or("no such resource")?;
        let ex = Exporter::new(&l.ar, &names, &l.linker);
        minediver::inspect::inspect(&ex, a).map_err(err)
    })
    .await
    .map_err(err)?
}

#[derive(Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
    failed: usize,
    files: usize,
    current: String,
    finished: bool,
    errors: Vec<String>,
    out: String,
    cancelled: bool,
}

/// Extracts in the background. Progress arrives as `extract` events; one job runs at a time.
#[tauri::command]
fn extract(app: AppHandle, state: State<App>, ids: Vec<u32>, out: String, options: Options) -> Res<usize> {
    let l = loaded(&state)?;
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("An extraction is already running.".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let names = state.names.clone();
    let cancel = state.cancel.clone();
    let busy = state.busy.clone();
    let mut options = options;
    if options.ffmpeg.is_none() {
        options.ffmpeg = state.settings.lock().unwrap().ffmpeg.clone().map(PathBuf::from);
    }
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<u32> = ids.into_iter().filter(|&i| l.ar.assets.get(i as usize).is_some_and(|a| seen.insert((a.type_hash, a.name_hash)))).collect();
    let total = ids.len();
    std::thread::spawn(move || {
        let ex = Exporter::new(&l.ar, &names, &l.linker);
        let out_dir = PathBuf::from(&out);
        let done = AtomicUsize::new(0);
        let failed = AtomicUsize::new(0);
        let files = AtomicUsize::new(0);
        let errors = Mutex::new(Vec::new());
        let last = Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
        let emit = |current: String, finished: bool| {
            let _ = app.emit(
                "extract",
                Progress {
                    done: done.load(Ordering::Relaxed),
                    total,
                    failed: failed.load(Ordering::Relaxed),
                    files: files.load(Ordering::Relaxed),
                    current,
                    finished,
                    errors: if finished { errors.lock().unwrap().clone() } else { vec![] },
                    out: out.clone(),
                    cancelled: cancel.load(Ordering::Relaxed),
                },
            );
        };
        ids.par_iter().for_each(|&id| {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Some(a) = l.ar.assets.get(id as usize) else { return };
            let name = export::display_name(&names, a);
            match ex.export(a, &out_dir, &options) {
                Ok(w) => {
                    files.fetch_add(w.len(), Ordering::Relaxed);
                }
                Err(e) => {
                    failed.fetch_add(1, Ordering::Relaxed);
                    let mut errs = errors.lock().unwrap();
                    if errs.len() < 200 {
                        errs.push(format!("{name}: {e:#}"));
                    }
                }
            }
            done.fetch_add(1, Ordering::Relaxed);
            let mut t = last.lock().unwrap();
            if t.elapsed().as_millis() > 80 {
                *t = std::time::Instant::now();
                drop(t);
                emit(name, false);
            }
        });
        emit(String::new(), true);
        busy.store(false, Ordering::SeqCst);
    });
    Ok(total)
}

#[tauri::command]
fn cancel_extract(state: State<App>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[derive(Serialize)]
struct Coverage {
    total: usize,
    named: usize,
    known: usize,
    by_type: Vec<(String, usize, usize)>,
}

#[tauri::command]
fn coverage(state: State<App>) -> Res<Coverage> {
    let l = loaded(&state)?;
    let mut by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut named = 0;
    let mut total = 0;
    for a in l.ar.assets.iter().filter(|a| a.patch.is_none()) {
        let e = by.entry(hash::type_label(a.type_hash)).or_default();
        e.0 += 1;
        total += 1;
        if state.names.contains(a.name_hash) {
            e.1 += 1;
            named += 1;
        }
    }
    let mut by_type: Vec<_> = by.into_iter().map(|(t, (n, k))| (t, n, k)).collect();
    by_type.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(Coverage { total, named, known: state.names.len(), by_type })
}

#[tauri::command]
fn import_names(app: AppHandle, state: State<App>, path: String) -> Res<usize> {
    let n = state.names.add_file(std::path::Path::new(&path)).map_err(err)?;
    let mut s = state.settings.lock().unwrap();
    if !s.name_lists.contains(&path) {
        s.name_lists.push(path);
    }
    save_settings(&app, &s);
    Ok(n)
}

#[tauri::command]
fn export_names(state: State<App>, path: String, unresolved: bool) -> Res<()> {
    if unresolved {
        let l = loaded(&state)?;
        let mut rows: Vec<String> = l.ar.assets.iter().filter(|a| a.patch.is_none() && !state.names.contains(a.name_hash)).map(|a| format!("{:016x} {}", a.name_hash, hash::type_label(a.type_hash))).collect();
        rows.sort();
        rows.dedup();
        return std::fs::write(&path, rows.join("\n") + "\n").map_err(err);
    }
    std::fs::write(&path, state.names.export()).map_err(err)
}

/// Checks typed names against the archive: returns the ones that name a resource, and adds them.
#[tauri::command]
fn try_names(state: State<App>, text: String) -> Res<Vec<String>> {
    let l = loaded(&state)?;
    let known: std::collections::HashSet<u64> = l.ar.assets.iter().map(|a| a.name_hash).collect();
    let hits: Vec<String> = text.lines().map(str::trim).filter(|n| !n.is_empty() && known.contains(&hash::hash(n))).map(str::to_string).collect();
    state.names.add_list(&hits.join("\n"));
    Ok(hits)
}

#[tauri::command]
async fn mod_start(app: AppHandle, ids: Vec<u32>, dir: String, id: String, name: String, new_prefix: Option<String>) -> Res<Value> {
    let state = app.state::<App>();
    let l = loaded(&state)?;
    let names = state.names.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ex = Exporter::new(&l.ar, &names, &l.linker);
        let assets: Vec<&archive::Asset> = ids.iter().filter_map(|&i| l.ar.assets.get(i as usize)).collect();
        minediver::pack::start(&ex, &assets, std::path::Path::new(&dir), &id, &name, new_prefix.as_deref()).map(|p| json!(p)).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn mod_read(dir: String) -> Res<Value> {
    let t = std::fs::read_to_string(PathBuf::from(&dir).join("mod.json")).map_err(|_| "This folder has no mod.json.".to_string())?;
    serde_json::from_str(&t).map_err(err)
}

#[tauri::command]
fn mod_save(dir: String, project: Value) -> Res<()> {
    let p: minediver::pack::Project = serde_json::from_value(project).map_err(err)?;
    std::fs::write(PathBuf::from(dir).join("mod.json"), serde_json::to_string_pretty(&p).map_err(err)? + "\n").map_err(err)
}

#[tauri::command]
async fn mod_build(app: AppHandle, dir: String, out: String) -> Res<Value> {
    let state = app.state::<App>();
    let l = loaded(&state)?;
    let names = state.names.clone();
    tauri::async_runtime::spawn_blocking(move || minediver::pack::build(&l.ar, &names, std::path::Path::new(&dir), std::path::Path::new(&out)).map(|r| json!(r)).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
fn find_ffmpeg(state: State<App>) -> Option<String> {
    let s = state.settings.lock().unwrap();
    export::find_ffmpeg(s.ffmpeg.as_deref().map(std::path::Path::new)).map(|p| p.display().to_string())
}

/// `md://localhost/<what>/<id>[/<n>]`: preview bytes for the UI. Textures as PNG (capped at 2048 px),
/// models as GLB with textures, sound bank clips as WAV or OGG.
fn preview(app: &AppHandle, path: &str) -> Result<(Vec<u8>, &'static str), String> {
    let state = app.state::<App>();
    let l = loaded(&state)?;
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let id: usize = parts.get(1).and_then(|s| s.parse().ok()).ok_or("bad id")?;
    let a = l.ar.assets.get(id).ok_or("no such resource")?;
    let ex = Exporter::new(&l.ar, &state.names, &l.linker);
    match parts[0] {
        "texture" => {
            let (d, _) = l.ar.read_variant(a, 0).map_err(err)?;
            let (w, h, px) = minediver::texture::thumbnail(&d, 2048).map_err(err)?;
            Ok((minediver::texture::encode_png(w, h, &px).map_err(err)?, "image/png"))
        }
        "model" => Ok((ex.model_glb(a, true).map_err(err)?.0, "model/gltf-binary")),
        "clip" => {
            let n: usize = parts.get(2).and_then(|s| s.parse().ok()).ok_or("bad clip")?;
            let (d, s) = l.ar.read_variant(a, 0).map_err(err)?;
            let clips = minediver::audio::clips(&d).map_err(err)?;
            let c = clips.get(n).ok_or("no such clip")?;
            let mime = if c.codec == minediver::audio::VORBIS { "audio/ogg" } else { "audio/wav" };
            Ok((minediver::audio::clip_file(&d, &s, c).map_err(err)?, mime))
        }
        "text" => {
            let (d, _) = l.ar.read_variant(a, 0).map_err(err)?;
            Ok((d.into_iter().take(256 * 1024).collect(), "application/octet-stream"))
        }
        _ => Err("unknown preview".into()),
    }
}

pub fn run() {
    let portable = portable_data();
    if let Some(d) = &portable {
        // WebView2 keeps the cache beside the exe too, so a portable copy leaves nothing in the user folders.
        let _ = std::fs::create_dir_all(d);
        unsafe { std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", d.join("webview")) };
    }
    let mut builder = tauri::Builder::default();
    if portable.is_none() {
        builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    }
    builder
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(App { loaded: RwLock::new(None), names: Arc::new(NameDb::builtin()), cancel: Default::default(), busy: Default::default(), settings: Mutex::new(Settings::default()) })
        .setup(|app| {
            let handle = app.handle().clone();
            if let Some(p) = settings_path(&handle) {
                if let Ok(t) = std::fs::read_to_string(p) {
                    if let Ok(s) = serde_json::from_str::<Settings>(&t) {
                        let state = handle.state::<App>();
                        for l in &s.name_lists {
                            let _ = state.names.add_file(std::path::Path::new(l));
                        }
                        *state.settings.lock().unwrap() = s;
                    }
                }
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("md", |ctx, req, responder| {
            let app = ctx.app_handle().clone();
            let path = req.uri().path().to_string();
            std::thread::spawn(move || {
                let r = match preview(&app, &path) {
                    Ok((bytes, mime)) => tauri::http::Response::builder().header("Content-Type", mime).header("Access-Control-Allow-Origin", "*").body(bytes),
                    Err(e) => tauri::http::Response::builder().status(404).header("Access-Control-Allow-Origin", "*").body(e.into_bytes()),
                };
                responder.respond(r.unwrap());
            });
        })
        .invoke_handler(tauri::generate_handler![
            get_settings, set_settings, open_game, query, rows, inspect, extract, cancel_extract, coverage, import_names, export_names,
            try_names, mod_start, mod_read, mod_save, mod_build, find_ffmpeg, is_portable, export_list, categories
        ])
        .run(tauri::generate_context!())
        .expect("error while running Minediver");
}
