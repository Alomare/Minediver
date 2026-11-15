<script>
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { Search, Download, Settings as Gear, BookA, Hammer, FolderOpen, X, Minimize2 } from "@lucide/svelte";
  import { app, runQuery, openGame, selectedIds, saveLayout, setCategories, focusedId } from "./lib/state.svelte.js";
  import { call, on } from "./lib/api.js";
  import Sidebar from "./components/Sidebar.svelte";
  import Table from "./components/Table.svelte";
  import Inspector from "./components/Inspector.svelte";
  import Preview from "./components/Preview.svelte";
  import Splitter from "./components/Splitter.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import Settings from "./components/Settings.svelte";
  import Names from "./components/Names.svelte";
  import ModBuilder from "./components/ModBuilder.svelte";
  import Logo from "./components/Logo.svelte";

  let searchEl = $state();
  let width = $state(1360);
  let timer;

  onMount(async () => {
    app.settings = await call("get_settings");
    app.portable = await call("is_portable");
    applyTheme();
    on("scan", (p) => {
      app.scan = p;
    });
    on("extract", (p) => {
      app.job = p;
    });
    on("categories", setCategories);
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
    } else if (e.key === "Escape") {
      if (app.dialog) app.dialog = "";
      else if (app.big) app.big = false;
    } else if (app.big && !app.dialog && (e.key === "ArrowDown" || e.key === "ArrowUp") && !e.target.closest("input, textarea, select")) {
      // Step through the list without leaving the large preview.
      e.preventDefault();
      const next = Math.max(0, Math.min(app.ids.length - 1, app.focus + (e.key === "ArrowDown" ? 1 : -1)));
      app.focus = next;
      app.anchor = next;
    }
  }

  // Panels never squeeze the table below a usable width.
  let sideMax = $derived(Math.max(180, Math.min(520, width * 0.32)));
  let inspMax = $derived(Math.max(280, width - app.layout.side - 380));
  let side = $derived(Math.min(app.layout.side, sideMax));
  let insp = $derived(Math.min(app.layout.insp, inspMax));
  let bigInfo = $derived(app.big && app.info && ((app.info.kind === "texture" && app.info.texture) || (app.info.kind === "model" && app.info.model)) ? app.info : null);
</script>

<svelte:window onkeydown={onKey} bind:innerWidth={width} />

<div class="shell" style="grid-template-columns: {app.summary ? `${side}px 0 minmax(0, 1fr) 0 ${insp}px` : '1fr'}">
  <header class="top">
    <div class="brand"><Logo size={20} /><span>Minediver</span></div>
    <div class="search">
      <Search size={15} />
      <input bind:this={searchEl} bind:value={app.text} oninput={onSearch} placeholder="Search names, types, hashes" spellcheck="false" />
      {#if app.text}<button class="ghost icon clear" onclick={() => { app.text = ""; runQuery(); }} aria-label="Clear search"><X size={14} /></button>{/if}
      <span class="kbd">Ctrl F</span>
    </div>
    <div class="spacer"></div>
    <button class="ghost modbtn" data-tip="Turn edited files into an Obsidian mod" onclick={() => (app.dialog = "mod")}><Hammer size={15} />Mod builder</button>
    <button class="ghost icon" data-tip="Names" onclick={() => (app.dialog = "names")}><BookA size={16} /></button>
    <button class="ghost icon tip-left" data-tip="Settings (Ctrl ,)" onclick={() => (app.dialog = "settings")}><Gear size={16} /></button>
    <button class="primary" disabled={!app.summary || (!app.selected.size && app.focus < 0)} onclick={() => extractSelected(false)}>
      <Download size={15} />Extract{app.selected.size ? ` ${app.selected.size.toLocaleString()}` : ""}
    </button>
  </header>

  {#if app.summary}
    <div class="side"><Sidebar /></div>
    <Splitter bind:value={app.layout.side} min={160} max={sideMax} reset={240} onend={saveLayout} label="Resize the sidebar" />
    <main class="center">
      {#if bigInfo}
        <div class="big">
          <div class="bighead">
            <span class="bigname" class:mono={!bigInfo.named}>{bigInfo.name}</span>
            <button class="ghost" onclick={() => (app.big = false)}><Minimize2 size={14} />Back to the list<span class="kbd">Esc</span></button>
          </div>
          <div class="bigbody"><Preview info={bigInfo} id={focusedId()} big /></div>
        </div>
      {/if}
      <div class="tablewrap" class:hidden={!!bigInfo}><Table onextract={extractSelected} /></div>
    </main>
    <Splitter bind:value={app.layout.insp} min={260} max={inspMax} reset={380} invert onend={saveLayout} label="Resize the details panel" />
    <div class="insp"><Inspector /></div>
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
  .shell { display: grid; height: 100vh; grid-template-rows: 44px minmax(0, 1fr) 30px; }
  .top {
    grid-column: 1 / -1; display: flex; align-items: center; gap: 6px; padding: 0 8px;
    background: var(--panel); border-bottom: 1px solid var(--line);
  }
  .brand { display: flex; align-items: center; gap: 8px; font-weight: 750; letter-spacing: 0.01em; padding: 0 10px 0 2px; }
  .search {
    display: flex; align-items: center; gap: 6px; height: 30px; padding: 0 6px 0 9px; width: min(520px, 42vw);
    background: var(--bg); border: 1px solid var(--line); border-radius: var(--radius); color: var(--faint);
  }
  .search:focus-within { border-color: var(--accent); }
  .search input { flex: 1; border: 0; background: transparent; height: 26px; padding: 0; color: var(--text); outline: none; }
  .search .clear { height: 22px; width: 22px; }
  .spacer { flex: 1; }
  .modbtn :global(svg) { color: var(--mod); }
  .side { min-height: 0; border-right: 1px solid var(--line); }
  .insp { min-height: 0; min-width: 0; border-left: 1px solid var(--line); }
  .center { min-width: 0; min-height: 0; background: var(--panel); container: center / inline-size; position: relative; }
  .tablewrap { height: 100%; }
  .tablewrap.hidden { display: none; }
  .big { height: 100%; display: flex; flex-direction: column; }
  .bighead { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 10px; height: 36px; padding: 0 6px 0 12px; border-bottom: 1px solid var(--line); }
  .bigname { font-weight: 650; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bigbody { flex: 1; min-height: 0; }
  .start { grid-column: 1 / -1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; }
  .start h1 { margin: 6px 0 0; font-size: 20px; font-weight: 700; }
  .start p { margin: 0 0 8px; max-width: 460px; text-align: center; }
  .bar { width: 280px; height: 4px; background: var(--raised); border-radius: 2px; overflow: hidden; }
  .bar div { height: 100%; background: var(--accent); }
</style>
