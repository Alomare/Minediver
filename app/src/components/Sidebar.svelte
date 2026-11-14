<script>
  import { ChevronRight } from "@lucide/svelte";
  import { app, runQuery } from "../lib/state.svelte.js";
  import { kindLabel } from "../lib/api.js";

  let openTypes = $state(false);
  let openBundles = $state(false);

  function toggle(list, v) {
    app[list] = app[list].includes(v) ? app[list].filter((x) => x !== v) : [...app[list], v];
    runQuery();
  }
  function only(list, v) {
    app[list] = app[list].length === 1 && app[list][0] === v ? [] : [v];
    runQuery();
  }
  let filtered = $derived(app.kinds.length || app.types.length || app.bundles.length || app.include || app.exclude || app.namedOnly);
  function clearAll() {
    Object.assign(app, { kinds: [], types: [], bundles: [], include: "", exclude: "", namedOnly: false });
    runQuery();
  }
  let timer;
  const later = () => { clearTimeout(timer); timer = setTimeout(runQuery, 200); };
</script>

<aside class="side">
  <div class="list">
    <button class="item" class:on={!app.kinds.length} onclick={() => { app.kinds = []; runQuery(); }}>
      <span>All resources</span><span class="n">{app.summary.total.toLocaleString()}</span>
    </button>
    {#each app.summary.kinds as [k, n]}
      <button class="item" class:on={app.kinds.includes(k)} onclick={(e) => (e.ctrlKey ? toggle("kinds", k) : only("kinds", k))}>
        <span><i class="dot {k}"></i>{kindLabel(k)}</span><span class="n">{n.toLocaleString()}</span>
      </button>
    {/each}
  </div>

  <button class="group" onclick={() => (openTypes = !openTypes)}><ChevronRight size={14} class={openTypes ? "rot" : ""} />Types</button>
  {#if openTypes}
    <div class="list">
      {#each app.summary.types as [t, n]}
        <button class="item sub" class:on={app.types.includes(t)} onclick={() => toggle("types", t)}><span class="mono">{t}</span><span class="n">{n.toLocaleString()}</span></button>
      {/each}
    </div>
  {/if}

  <button class="group" onclick={() => (openBundles = !openBundles)}><ChevronRight size={14} class={openBundles ? "rot" : ""} />Bundles</button>
  {#if openBundles}
    <div class="list">
      {#each app.summary.bundles as [b, name, n]}
        <button class="item sub" class:on={app.bundles.includes(b)} onclick={() => toggle("bundles", b)} title={b}>
          <span class:mono={!name}>{name ?? b}</span><span class="n">{n.toLocaleString()}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="patterns">
    <span class="section-title">Patterns</span>
    <input bind:value={app.include} oninput={later} placeholder="Include, e.g. equipment/wep_*" spellcheck="false" />
    <input bind:value={app.exclude} oninput={later} placeholder="Exclude" spellcheck="false" />
    <label class="check"><input type="checkbox" bind:checked={app.namedOnly} onchange={runQuery} />Named only</label>
    {#if filtered}<button class="ghost clear" onclick={clearAll}>Clear filters</button>{/if}
  </div>
</aside>

<style>
  .side { grid-area: side; overflow-y: auto; overflow-x: hidden; background: var(--bg); border-right: 1px solid var(--line); padding: 8px 6px; display: flex; flex-direction: column; gap: 4px; }
  .list { display: flex; flex-direction: column; }
  .item { justify-content: space-between; height: 27px; border: 0; background: transparent; padding: 0 8px; border-radius: 4px; color: var(--muted); min-width: 0; }
  .item span:first-child { display: flex; align-items: center; gap: 8px; overflow: hidden; text-overflow: ellipsis; }
  .item:hover { background: var(--hover); color: var(--text); }
  .item.on { background: var(--accent-soft); color: var(--text); }
  .item.sub { height: 24px; font-size: 12px; }
  .n { font-size: 11px; color: var(--faint); font-family: var(--mono); }
  .group { border: 0; background: transparent; color: var(--faint); font-size: 11px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; padding: 0 4px; margin-top: 8px; height: 26px; }
  .group:hover { color: var(--text); }
  .group :global(.rot) { transform: rotate(90deg); }
  .patterns { margin-top: auto; padding: 10px 4px 2px; display: flex; flex-direction: column; gap: 6px; border-top: 1px solid var(--line-soft); }
  .patterns input[type="text"], .patterns input:not([type]) { width: 100%; font-family: var(--mono); font-size: 12px; }
  .clear { align-self: flex-start; color: var(--accent); padding: 0 6px; }
  .dot { width: 7px; height: 7px; border-radius: 2px; flex: none; background: var(--faint); }
  .dot.model { background: var(--accent); }
  .dot.texture { background: oklch(0.7 0.11 230); }
  .dot.audio { background: oklch(0.72 0.12 300); }
  .dot.video { background: oklch(0.7 0.13 15); }
  .dot.script, .dot.text { background: oklch(0.75 0.08 120); }
</style>
