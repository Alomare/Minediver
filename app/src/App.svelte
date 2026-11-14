<script>
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { Search, Download, Settings as Gear, PanelLeft, PanelRight, BookA, Hammer, FolderOpen, X } from "@lucide/svelte";
  import { app, runQuery, openGame, selectedIds } from "./lib/state.svelte.js";
  import { call, on, KINDS } from "./lib/api.js";
  import Sidebar from "./components/Sidebar.svelte";
  import Table from "./components/Table.svelte";
  import Inspector from "./components/Inspector.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import Settings from "./components/Settings.svelte";
  import Names from "./components/Names.svelte";
  import ModBuilder from "./components/ModBuilder.svelte";
  import Logo from "./components/Logo.svelte";

  let searchEl = $state();
  let timer;

  onMount(async () => {
    app.settings = await call("get_settings");
    applyTheme();
    on("scan", (p) => (app.scan = p));
    on("extract", (p) => {
      app.job = p;
    });
    openGame();
  });

  function applyTheme() {
    const t = app.settings?.theme ?? "dark";
    const dark = t === "system" ? matchMedia("(prefers-color-scheme: dark)").matches : t === "dark";
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }
  $effect(() => {
    app.settings?.theme;
    applyTheme();
  });

  function onSearch() {
    clearTimeout(timer);
    timer = setTimeout(runQuery, 90);
  }

  function toggleKind(k) {
    app.kinds = app.kinds.includes(k) ? app.kinds.filter((x) => x !== k) : [...app.kinds, k];
    runQuery();
  }

  export async function extractSelected(ask = false) {
    const ids = selectedIds();
    if (!ids.length) return;
    let out = app.settings?.output_dir;
    if (ask || !out) {
      out = await open({ directory: true, title: "Extract to" });
      if (!out) return;
      if (!app.settings.output_dir) {
        app.settings = { ...app.settings, output_dir: out };
        await call("set_settings", { settings: app.settings });
      }
    }
    try {
      await call("extract", { ids, out, options: app.settings.options });
    } catch (e) {
      app.toast = String(e);
    }
  }

  async function pickGame() {
    const dir = await open({ directory: true, title: "Helldivers folder" });
    if (dir) openGame(dir);
  }

  function onKey(e) {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === "f") {
      e.preventDefault();
      searchEl?.focus();
      searchEl?.select();
    } else if (mod && e.key.toLowerCase() === "e") {
      e.preventDefault();
      extractSelected(e.shiftKey);
    } else if (mod && e.key === ",") {
      e.preventDefault();
      app.dialog = "settings";
    } else if (e.key === "Escape" && app.dialog) {
      app.dialog = "";
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell" class:no-side={!app.sidebar} class:no-insp={!app.inspector}>
  <header class="top">
    <div class="brand"><Logo size={20} /><span>Minediver</span></div>
    <button class="ghost icon" data-tip="Sidebar" onclick={() => (app.sidebar = !app.sidebar)}><PanelLeft size={16} /></button>
    <div class="search">
      <Search size={15} />
      <input bind:this={searchEl} bind:value={app.text} oninput={onSearch} placeholder="Search names, types, hashes" spellcheck="false" />
      {#if app.text}<button class="ghost icon clear" onclick={() => { app.text = ""; runQuery(); }} aria-label="Clear search"><X size={14} /></button>{/if}
      <span class="kbd">Ctrl F</span>
    </div>
    <div class="chips">
      {#each KINDS.slice(0, 6) as [k, label]}
        <button class="chip" class:on={app.kinds.includes(k)} onclick={() => toggleKind(k)}>{label}</button>
      {/each}
    </div>
    <div class="spacer"></div>
    <button class="ghost" data-tip="Turn edited files into an Obsidian mod" onclick={() => (app.dialog = "mod")}><Hammer size={15} />Mod builder</button>
    <button class="ghost icon" data-tip="Names" onclick={() => (app.dialog = "names")}><BookA size={16} /></button>
    <button class="ghost icon tip-left" data-tip="Settings (Ctrl ,)" onclick={() => (app.dialog = "settings")}><Gear size={16} /></button>
    <button class="primary" disabled={!app.summary || (!app.selected.size && app.focus < 0)} onclick={() => extractSelected(false)}>
      <Download size={15} />Extract{app.selected.size > 1 ? ` ${app.selected.size.toLocaleString()}` : ""}
    </button>
    <button class="ghost icon tip-left" data-tip="Details panel" onclick={() => (app.inspector = !app.inspector)}><PanelRight size={16} /></button>
  </header>

  {#if app.summary}
    {#if app.sidebar}<Sidebar />{/if}
    <main class="center"><Table onextract={extractSelected} /></main>
    {#if app.inspector}<Inspector />{/if}
  {:else}
    <section class="start">
      <Logo size={56} />
      {#if app.loading}
        <p>Reading the game files{app.scan[1] ? ` (${app.scan[0]} of ${app.scan[1]} bundles)` : ""}</p>
        <div class="bar"><div style="width: {app.scan[1] ? (app.scan[0] / app.scan[1]) * 100 : 5}%"></div></div>
      {:else}
        <h1>Open Helldivers</h1>
        <p class="muted">{app.error || "Pick the game folder, the one that holds contents."}</p>
        <button class="primary" onclick={pickGame}><FolderOpen size={15} />Choose game folder</button>
      {/if}
    </section>
  {/if}

  <StatusBar />
</div>

{#if app.dialog === "settings"}<Settings onpickgame={pickGame} />{/if}
{#if app.dialog === "names"}<Names />{/if}
{#if app.dialog === "mod"}<ModBuilder />{/if}

<style>
  .shell {
    display: grid; height: 100vh;
    grid-template-columns: 220px minmax(0, 1fr) 340px;
    grid-template-rows: 44px minmax(0, 1fr) 30px;
    grid-template-areas: "top top top" "side center insp" "status status status";
  }
  .shell.no-side { grid-template-columns: 0 minmax(0, 1fr) 340px; }
  .shell.no-insp { grid-template-columns: 220px minmax(0, 1fr) 0; }
  .shell.no-side.no-insp { grid-template-columns: 0 minmax(0, 1fr) 0; }
  .top {
    grid-area: top; display: flex; align-items: center; gap: 6px; padding: 0 8px;
    background: var(--panel); border-bottom: 1px solid var(--line);
  }
  .brand { display: flex; align-items: center; gap: 8px; font-weight: 750; letter-spacing: 0.01em; padding: 0 6px 0 2px; }
  .search {
    display: flex; align-items: center; gap: 6px; height: 30px; padding: 0 6px 0 9px; width: min(380px, 32vw);
    background: var(--bg); border: 1px solid var(--line); border-radius: var(--radius); color: var(--faint);
  }
  .search:focus-within { border-color: var(--accent); }
  .search input { flex: 1; border: 0; background: transparent; height: 26px; padding: 0; color: var(--text); outline: none; }
  .search .clear { height: 22px; width: 22px; }
  .chips { display: flex; gap: 4px; margin-left: 4px; }
  .chip { height: 26px; padding: 0 9px; border-radius: 13px; background: transparent; color: var(--muted); }
  .chip.on { background: var(--accent-soft); border-color: var(--accent); color: var(--text); }
  .spacer { flex: 1; }
  .center { grid-area: center; min-width: 0; min-height: 0; background: var(--panel); }
  .start { grid-column: 1 / -1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; }
  .start h1 { margin: 6px 0 0; font-size: 20px; font-weight: 700; }
  .start p { margin: 0 0 8px; max-width: 460px; text-align: center; }
  .bar { width: 280px; height: 4px; background: var(--raised); border-radius: 2px; overflow: hidden; }
  .bar div { height: 100%; background: var(--accent); }
  @media (max-width: 1150px) { .chips { display: none; } }
</style>
