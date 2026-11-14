<script>
  import { ArrowUp, ArrowDown, Download, FolderDown, Hammer, Copy } from "@lucide/svelte";
  import { app, runQuery, ensureRows, selectedIds } from "../lib/state.svelte.js";
  import { formatSize, kindLabel } from "../lib/api.js";

  let { onextract } = $props();
  const ROW = 26;
  let scroller = $state();
  let scrollTop = $state(0);
  let height = $state(400);
  let menu = $state(null);

  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 8));
  let last = $derived(Math.min(app.ids.length, Math.ceil((scrollTop + height) / ROW) + 8));
  let visible = $derived(app.ids.slice(first, last));

  $effect(() => {
    ensureRows(visible);
  });

  function sortBy(key) {
    app.sort = { key, desc: app.sort.key === key ? !app.sort.desc : false };
    runQuery();
  }

  function click(e, index) {
    const id = app.ids[index];
    const next = new Set(e.ctrlKey || e.metaKey || e.shiftKey ? app.selected : []);
    if (e.shiftKey && app.anchor >= 0) {
      const [a, b] = [Math.min(app.anchor, index), Math.max(app.anchor, index)];
      for (let i = a; i <= b; i++) next.add(app.ids[i]);
    } else if (e.ctrlKey || e.metaKey) {
      next.has(id) ? next.delete(id) : next.add(id);
      app.anchor = index;
    } else {
      next.add(id);
      app.anchor = index;
    }
    app.selected = next;
    app.focus = index;
  }

  function moveTo(index, extend) {
    index = Math.max(0, Math.min(app.ids.length - 1, index));
    if (extend && app.anchor >= 0) {
      const next = new Set();
      const [a, b] = [Math.min(app.anchor, index), Math.max(app.anchor, index)];
      for (let i = a; i <= b; i++) next.add(app.ids[i]);
      app.selected = next;
    } else {
      app.selected = new Set([app.ids[index]]);
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
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      app.selected = new Set(app.ids);
    } else if (e.key === "Enter") {
      e.preventDefault();
      app.inspector = true;
    }
  }

  function context(e, index) {
    e.preventDefault();
    if (!app.selected.has(app.ids[index])) click({}, index);
    menu = { x: Math.min(e.clientX, innerWidth - 230), y: Math.min(e.clientY, innerHeight - 190) };
  }

  async function copy(what) {
    const rows = selectedIds().map((i) => app.rowCache.get(i)).filter(Boolean);
    await navigator.clipboard.writeText(rows.map((r) => (what === "name" ? r.name : r.id)).join("\n"));
    app.toast = `Copied ${rows.length === 1 ? "1 name" : `${rows.length} names`}`;
    menu = null;
  }

  function startMod() {
    menu = null;
    app.dialog = "mod";
    app.modSeed = selectedIds();
  }

  const cols = [
    ["name", "Name"],
    ["type", "Type"],
    ["size", "Size"],
    ["bundle", "Bundle"],
  ];
</script>

<svelte:window onclick={() => (menu = null)} />

<div class="table" role="grid" tabindex="0" onkeydown={key} aria-rowcount={app.ids.length}>
  <div class="head">
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
        <div
          class="row"
          class:sel={app.selected.has(id)}
          class:focus={index === app.focus}
          style="top: {index * ROW}px"
          role="row"
          tabindex="-1"
          aria-selected={app.selected.has(id)}
          onmousedown={(e) => e.button === 0 && click(e, index)}
          oncontextmenu={(e) => context(e, index)}
          ondblclick={() => onextract(false)}
        >
          {#if r}
            <span class="name" class:hash={!r.named}>{r.name}{#if r.patch}<em class="patch">patch</em>{/if}</span>
            <span class="type"><i class="dot {r.kind}"></i>{r.type}</span>
            <span class="size mono">{formatSize(r.size)}</span>
            <span class="bundle mono">{r.bundle}</span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

{#if menu}
  <div class="menu" style="left: {menu.x}px; top: {menu.y}px" role="menu">
    <button class="ghost" onclick={() => { menu = null; onextract(false); }}><Download size={14} />Extract<span class="kbd">Ctrl E</span></button>
    <button class="ghost" onclick={() => { menu = null; onextract(true); }}><FolderDown size={14} />Extract to...<span class="kbd">Ctrl Shift E</span></button>
    <button class="ghost" onclick={startMod}><Hammer size={14} />Start a mod from this</button>
    <hr />
    <button class="ghost" onclick={() => copy("name")}><Copy size={14} />Copy name</button>
  </div>
{/if}

<style>
  .table { display: flex; flex-direction: column; height: 100%; outline: none; }
  .head, .row { display: grid; grid-template-columns: minmax(0, 1fr) 150px 84px 170px; align-items: center; }
  .head { height: 30px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .col { background: transparent; border: 0; border-radius: 0; height: 30px; padding: 0 10px; color: var(--faint); font-size: 12px; font-weight: 650; }
  .col:hover { color: var(--text); }
  .col.size { justify-content: flex-end; }
  .body { flex: 1; overflow: auto; position: relative; }
  .row { position: absolute; left: 0; right: 0; height: 26px; padding: 0; cursor: default; border-bottom: 1px solid var(--line-soft); }
  .row:hover { background: var(--hover); }
  .row.sel { background: var(--select); }
  .table:focus-within .row.focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  .row span { padding: 0 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name.hash { font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .patch { font-style: normal; margin-left: 8px; font-size: 11px; color: var(--mod); }
  .type { color: var(--muted); display: flex; align-items: center; gap: 7px; }
  .size { text-align: right; color: var(--muted); }
  .bundle { color: var(--faint); }
  .dot { width: 7px; height: 7px; border-radius: 2px; flex: none; background: var(--faint); }
  .dot.model { background: var(--accent); }
  .dot.texture { background: oklch(0.7 0.11 230); }
  .dot.audio { background: oklch(0.72 0.12 300); }
  .dot.video { background: oklch(0.7 0.13 15); }
  .dot.script, .dot.text { background: oklch(0.75 0.08 120); }
  .empty { position: absolute; inset: 40px 0 auto; text-align: center; color: var(--faint); }
  .menu {
    position: fixed; z-index: 40; min-width: 220px; padding: 4px; display: flex; flex-direction: column;
    background: var(--raised); border: 1px solid var(--line); border-radius: 6px; box-shadow: var(--shadow);
  }
  .menu button { justify-content: flex-start; width: 100%; }
  .menu .kbd { margin-left: auto; }
  .menu hr { border: 0; border-top: 1px solid var(--line); margin: 4px 2px; }
  @media (max-width: 1100px) { .head, .row { grid-template-columns: minmax(0, 1fr) 130px 80px; } .bundle, .col.bundle { display: none; } }
</style>
