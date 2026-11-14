<script>
  import { open } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { FolderOpen, Hammer, Trash2, FolderPlus } from "@lucide/svelte";
  import { app } from "../lib/state.svelte.js";
  import { call } from "../lib/api.js";
  import Dialog from "./Dialog.svelte";

  let dir = $state(app.modDir ?? "");
  let project = $state(null);
  let busy = $state(false);
  let message = $state("");
  let report = $state(null);
  let seed = app.modSeed ?? [];
  app.modSeed = [];
  let newId = $state("me.my_mod");
  let newName = $state("My mod");

  async function load(d) {
    dir = d;
    app.modDir = d;
    report = null;
    try {
      project = await call("mod_read", { dir: d });
      message = "";
    } catch (e) {
      project = null;
      message = seed.length ? "" : String(e);
    }
  }
  if (dir) load(dir);

  async function pickDir() {
    const d = await open({ directory: true, title: "Mod project folder" });
    if (d) load(d);
  }

  async function start() {
    busy = true;
    try {
      project = await call("mod_start", { ids: seed, dir, id: newId, name: newName });
      message = `${seed.length === 1 ? "The resource was" : "The resources were"} written as editable files. Edit them in Blender or an image editor, keep the file names, then build.`;
      seed = [];
    } catch (e) {
      message = String(e);
    } finally {
      busy = false;
    }
  }

  async function saveProject() {
    await call("mod_save", { dir, project: $state.snapshot(project) });
  }
  async function remove(i) {
    project.replace.splice(i, 1);
    await saveProject();
  }

  async function build() {
    busy = true;
    report = null;
    await saveProject();
    const out = `${dir}${dir.includes("\\") ? "\\" : "/"}build${dir.includes("\\") ? "\\" : "/"}${project.id}`;
    try {
      report = await call("mod_build", { dir, out });
      message = "";
    } catch (e) {
      message = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title="Mod builder" width={680}>
  <p class="muted lead">
    Replace game models and textures with edited files. The result is an Obsidian mod folder: import it in Obsidian, which deploys it and can remove it again. Game files are never changed.
  </p>
  <div class="row">
    <input bind:value={dir} placeholder="Project folder" spellcheck="false" onchange={() => load(dir)} />
    <button onclick={pickDir}><FolderOpen size={14} />Choose</button>
    {#if dir}<button class="ghost icon" onclick={() => openPath(dir)} aria-label="Open folder" data-tip="Open folder"><FolderOpen size={14} /></button>{/if}
  </div>

  {#if seed.length && dir}
    <div class="seed">
      <span class="section-title">Start from {seed.length} selected</span>
      {#if !project}
        <div class="row"><span class="lbl">Mod id</span><input bind:value={newId} spellcheck="false" /></div>
        <div class="row"><span class="lbl">Name</span><input bind:value={newName} /></div>
      {/if}
      <div><button class="primary" disabled={busy} onclick={start}><FolderPlus size={14} />{project ? "Add to this project" : "Create project"}</button></div>
    </div>
  {:else if seed.length}
    <p class="faint">Choose an empty folder for the new project.</p>
  {/if}

  {#if project}
    <div class="fields">
      <label><span class="lbl">Id</span><input bind:value={project.id} onchange={saveProject} spellcheck="false" /></label>
      <label><span class="lbl">Name</span><input bind:value={project.name} onchange={saveProject} /></label>
      <label><span class="lbl">Version</span><input bind:value={project.version} onchange={saveProject} spellcheck="false" /></label>
    </div>
    <div class="list">
      {#each project.replace as r, i}
        <div class="item">
          <span class="ty">{r.type}</span>
          <span class="mono nm" title={r.name}>{r.name}</span>
          <span class="mono file">{r.file}</span>
          <button class="ghost icon" onclick={() => remove(i)} aria-label="Remove"><Trash2 size={13} /></button>
        </div>
      {:else}
        <p class="faint">Nothing to replace yet. Select models or textures in the list, right-click, Start a mod from this.</p>
      {/each}
    </div>
  {/if}

  {#if message}<p class="msg">{message}</p>{/if}
  {#if report}
    <div class="report">
      <b>Built {report.resources} resource entries</b> in <span class="mono">{report.out}</span>
      {#each report.notes as n}<p class="faint">{n}</p>{/each}
      <p class="faint">Repacking is experimental: test the mod in the game before sharing it.</p>
      <div><button onclick={() => openPath(report.out)}><FolderOpen size={14} />Show mod folder</button></div>
    </div>
  {/if}

  {#snippet footer()}
    <button onclick={() => (app.dialog = "")}>Close</button>
    <button class="mod" disabled={!project || !project.replace.length || busy} onclick={build}><Hammer size={14} />{busy ? "Working" : "Build mod"}</button>
  {/snippet}
</Dialog>

<style>
  .lead { margin: 0; }
  .row { display: flex; gap: 8px; align-items: center; }
  .row input { flex: 1; }
  .lbl { color: var(--muted); width: 56px; flex: none; }
  .seed { display: flex; flex-direction: column; gap: 8px; padding: 10px 12px; border: 1px solid var(--mod); border-radius: 6px; background: var(--mod-soft); }
  .fields { display: grid; grid-template-columns: 1fr 1fr 120px; gap: 8px; }
  .fields label { display: flex; flex-direction: column; gap: 4px; }
  .list { display: flex; flex-direction: column; border-top: 1px solid var(--line-soft); }
  .item { display: grid; grid-template-columns: 60px minmax(0, 1.3fr) minmax(0, 1fr) 28px; align-items: center; gap: 8px; height: 30px; border-bottom: 1px solid var(--line-soft); }
  .ty { color: var(--muted); }
  .nm, .file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file { color: var(--faint); }
  .msg { margin: 0; color: var(--muted); }
  .report { display: flex; flex-direction: column; gap: 6px; padding: 10px 12px; border: 1px solid var(--line); border-radius: 6px; }
  .report p { margin: 0; }
</style>
