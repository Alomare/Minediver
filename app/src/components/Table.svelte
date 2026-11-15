<script>
  import { ArrowUp, ArrowDown, Download, FolderDown, Hammer, Copy, Hash, ListChecks, Maximize2, FileText, Filter } from "@lucide/svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { app, runQuery, ensureRows, selectedIds } from "../lib/state.svelte.js";
  import { call, formatSize } from "../lib/api.js";

  let { onextract } = $props();
  const ROW = 26;
  let scroller = $state();
  let scrollTop = $state(0);
  let height = $state(400);
  let menu = $state(null);

  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 8));
  let last = $derived(Math.min(app.ids.length, Math.ceil((scrollTop + height) / ROW) + 8));
  let visible = $derived(app.ids.slice(first, last));
  let shownChecked = $derived.by(() => {
    if (!app.selected.size) return 0;
    let n = 0;
    for (const i of app.ids) if (app.selected.has(i)) n++;
    return n;
  });
  let allChecked = $derived(app.ids.length > 0 && shownChecked === app.ids.length);

  $effect(() => {
    ensureRows(visible);
  });

  function sortBy(key) {
    app.sort = { key, desc: app.sort.key === key ? !app.sort.desc : false };
    runQuery();
  }

  function checkRange(a, b, on = true) {
    const next = new Set(app.selected);
    for (let i = Math.min(a, b); i <= Math.max(a, b); i++) on ? next.add(app.ids[i]) : next.delete(app.ids[i]);
    app.selected = next;
  }
  function toggleCheck(index) {
    const id = app.ids[index];
    const next = new Set(app.selected);
    next.has(id) ? next.delete(id) : next.add(id);
    app.selected = next;
    app.anchor = index;
  }

  // A plain click shows the row; Ctrl toggles its check, Shift checks a range. Checks stay across searches.
  function click(e, index) {
    if (e.shiftKey && app.anchor >= 0) checkRange(app.anchor, index);
    else if (e.ctrlKey || e.metaKey) toggleCheck(index);
    else app.anchor = index;
    app.focus = index;
  }

  function toggleAll() {
    const next = new Set(app.selected);
    if (allChecked) app.ids.forEach((i) => next.delete(i));
    else app.ids.forEach((i) => next.add(i));
    app.selected = next;
  }

  function moveTo(index, extend) {
    index = Math.max(0, Math.min(app.ids.length - 1, index));
    if (extend) {
      if (app.anchor < 0) app.anchor = Math.max(0, app.focus);
      checkRange(app.anchor, index);
    } else {
      app.anchor = index;
    }
    app.focus = index;
    const top = index * ROW;
    if (top < scroller.scrollTop) scroller.scrollTop = top;
    else if (top + ROW > scroller.scrollTop + height) scroller.scrollTop = top + ROW - height;
  }

  function key(e) {
    if (!app.ids.length) return;
    const page = Math.max(1, Math.floor(height / ROW) - 1);
    const map = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    if (e.key in map) {
      e.preventDefault();
      moveTo((app.focus < 0 ? -1 : app.focus) + map[e.key], e.shiftKey);
    } else if (e.key === "Home") {
      e.preventDefault();
      moveTo(0, e.shiftKey);
    } else if (e.key === "End") {
      e.preventDefault();
      moveTo(app.ids.length - 1, e.shiftKey);
    } else if (e.key === " " && app.focus >= 0) {
      e.preventDefault();
      toggleCheck(app.focus);
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      if (!allChecked) toggleAll();
    } else if (e.key === "Enter" && app.focus >= 0) {
      e.preventDefault();
      app.big = true;
    }
  }

  function context(e, index) {
    e.preventDefault();
    app.focus = index;
    menu = { x: Math.min(e.clientX, innerWidth - 250), y: Math.min(e.clientY, innerHeight - 330), index };
  }

  async function copy(what) {
    menu = null;
    const ids = selectedIds();
    const rows = ids.map((i) => app.rowCache.get(i));
    if (rows.some((r) => !r)) await ensureRows(ids);
    const list = ids.map((i) => app.rowCache.get(i)).filter(Boolean);
    const text = list.map((r) => (what === "hash" ? r.hash : r.name)).join("\n");
    await navigator.clipboard.writeText(text);
    app.toast = `Copied ${list.length === 1 ? "1 line" : `${list.length.toLocaleString()} lines`}`;
  }

  async function exportList() {
    menu = null;
    const ids = app.selected.size ? selectedIds() : app.ids;
    const path = await save({ title: "Save the list", defaultPath: "minediver-list.tsv", filters: [{ name: "Tab-separated", extensions: ["tsv", "txt"] }] });
    if (!path) return;
    try {
      const n = await call("export_list", { ids, path });
      app.toast = `Saved ${n.toLocaleString()} rows`;
    } catch (e) {
      app.toast = String(e);
    }
  }

  function startMod() {
    menu = null;
    app.dialog = "mod";
    app.modSeed = selectedIds();
  }

  function sameBundle() {
    const r = app.rowCache.get(app.ids[menu.index]);
    menu = null;
    if (!r) return;
    app.bundles = [r.bundle.replace(/\..*$/, "")];
    runQuery();
  }

  async function extractShown() {
    menu = null;
    const keep = app.selected;
    app.selected = new Set(app.ids);
    await onextract(false);
    app.selected = keep;
  }

  const cols = [
    ["name", "Name"],
    ["type", "Type"],
    ["category", "Category"],
    ["size", "Size"],
    ["bundle", "Bundle"],
  ];
</script>

<svelte:window onclick={() => (menu = null)} />

<div class="table" role="grid" tabindex="0" onkeydown={key} aria-rowcount={app.ids.length} aria-multiselectable="true">
  <div class="head">
    <label class="cb" aria-label="Check every row shown">
      <input type="checkbox" checked={allChecked} indeterminate={shownChecked > 0 && !allChecked} onchange={toggleAll} disabled={!app.ids.length} />
    </label>
    {#each cols as [k, label]}
      <button class="col {k}" onclick={() => sortBy(k)}>
        {label}
        {#if app.sort.key === k}{#if app.sort.desc}<ArrowDown size={12} />{:else}<ArrowUp size={12} />{/if}{/if}
      </button>
    {/each}
  </div>
  <div class="body" bind:this={scroller} bind:clientHeight={height} onscroll={() => (scrollTop = scroller.scrollTop)}>
    {#if !app.ids.length}
      <div class="empty">Nothing matches. Clear the search or the filters on the left.</div>
    {/if}
    <div style="height: {app.ids.length * ROW}px; position: relative;">
      {#each visible as id, i (id)}
        {@const index = first + i}
        {@const r = app.rowCache.get(id)}
        {@const checked = app.selected.has(id)}
        <div
          class="row"
          class:sel={checked}
          class:focus={index === app.focus}
          style="top: {index * ROW}px"
          role="row"
          tabindex="-1"
          aria-selected={checked}
          onmousedown={(e) => e.button === 0 && !e.target.closest(".cb") && click(e, index)}
          oncontextmenu={(e) => context(e, index)}
          ondblclick={() => (app.big = true)}
        >
          <label class="cb" aria-label="Check">
            <input type="checkbox" {checked} onchange={(e) => { e.shiftKey && app.anchor >= 0 ? checkRange(app.anchor, index, !checked) : toggleCheck(index); app.focus = index; }} onclick={(e) => e.stopPropagation()} />
          </label>
          {#if r}
            <span class="name" class:hash={!r.named}>{r.name}{#if r.patch}<em class="patch">patch</em>{/if}</span>
            <span class="type"><i class="dot {r.kind}"></i>{r.type}</span>
            <span class="category" class:none={r.category === "Unnamed"}>{r.category === "Unnamed" ? "" : r.category}</span>
            <span class="size mono">{formatSize(r.size)}</span>
            <span class="bundle mono">{r.bundle}</span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

{#if menu}
  {@const n = selectedIds().length}
  <div class="menu" style="left: {menu.x}px; top: {menu.y}px" role="menu">
    <button class="ghost" onclick={() => { menu = null; onextract(false); }}><Download size={14} />Extract {n > 1 ? `${n.toLocaleString()} checked` : ""}<span class="kbd">Ctrl E</span></button>
    <button class="ghost" onclick={() => { menu = null; onextract(true); }}><FolderDown size={14} />Extract to<span class="kbd">Ctrl Shift E</span></button>
    <button class="ghost" onclick={extractShown}><ListChecks size={14} />Extract all {app.ids.length.toLocaleString()} shown</button>
    <button class="ghost" onclick={startMod}><Hammer size={14} />Start a mod from this</button>
    <hr />
    <button class="ghost" onclick={() => { menu = null; app.big = true; }}><Maximize2 size={14} />Large preview<span class="kbd">Enter</span></button>
    <button class="ghost" onclick={sameBundle}><Filter size={14} />Show only this bundle</button>
    <hr />
    <button class="ghost" onclick={() => copy("name")}><Copy size={14} />Copy name</button>
    <button class="ghost" onclick={() => copy("hash")}><Hash size={14} />Copy hash</button>
    <button class="ghost" onclick={exportList}><FileText size={14} />Save list as a table</button>
  </div>
{/if}

<style>
  .table { display: flex; flex-direction: column; height: 100%; outline: none; }
  .head, .row { display: grid; grid-template-columns: 30px minmax(0, 1fr) 128px 150px 86px 150px; align-items: center; }
  .head { height: 30px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .col { background: transparent; border: 0; border-radius: 0; height: 30px; padding: 0 10px; color: var(--faint); font-size: 12px; font-weight: 650; display: flex; align-items: center; gap: 6px; }
  button.col:hover { color: var(--text); background: transparent; }
  .col.size { justify-content: flex-end; }
  .cb { display: grid; place-items: center; height: 100%; cursor: pointer; }
  .body { flex: 1; overflow: auto; position: relative; }
  .row { position: absolute; left: 0; right: 0; height: 26px; padding: 0; cursor: default; border-bottom: 1px solid var(--line-soft); }
  .row:hover { background: var(--hover); }
  .row.sel { background: var(--select); }
  .row.focus { box-shadow: inset 2px 0 0 var(--accent); }
  .table:focus-within .row.focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  .row span { padding: 0 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name.hash { font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .patch { font-style: normal; margin-left: 8px; font-size: 11px; color: var(--mod); }
  .type { color: var(--muted); display: flex; align-items: center; gap: 7px; }
  .category { color: var(--muted); font-size: 12px; }
  .size { text-align: right; color: var(--muted); }
  .bundle { color: var(--faint); }
  .empty { position: absolute; inset: 40px 0 auto; text-align: center; color: var(--faint); }
  .menu {
    position: fixed; z-index: 40; min-width: 240px; padding: 4px; display: flex; flex-direction: column;
    background: var(--raised); border: 1px solid var(--line); border-radius: 6px; box-shadow: var(--shadow);
  }
  .menu button { justify-content: flex-start; width: 100%; }
  .menu .kbd { margin-left: auto; }
  .menu hr { border: 0; border-top: 1px solid var(--line); margin: 4px 2px; }
  @container center (max-width: 820px) { .head, .row { grid-template-columns: 30px minmax(0, 1fr) 120px 86px; } .bundle, .col.bundle, .category, .col.category { display: none; } }
  @container center (min-width: 821px) and (max-width: 1040px) { .head, .row { grid-template-columns: 30px minmax(0, 1fr) 120px 140px 86px; } .bundle, .col.bundle { display: none; } }
</style>
