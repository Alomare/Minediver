<script>
  import { ChevronRight } from "@lucide/svelte";
  import { app, runQuery } from "../lib/state.svelte.js";
  import { kindLabel } from "../lib/api.js";

  let openTypes = $state(false);
  let openBundles = $state(false);
  let open = $state(new Set(["model"]));

  /** kind -> { count, tops: [{ name, count, subs: [{ name, count }] }] }, in the order the backend gives the kinds. */
  let tree = $derived.by(() => {
    const out = [];
    const cats = app.summary?.cats ?? [];
    for (const [kind, total] of app.summary?.kinds ?? []) {
      const tops = new Map();
      for (const [k, label, n] of cats) {
        if (k !== kind) continue;
        const [top, sub] = label.split(/\/(.*)/s);
        const t = tops.get(top) ?? { name: top, count: 0, subs: [], own: 0 };
        t.count += n;
        if (sub) t.subs.push({ name: sub, count: n });
        else t.own += n;
        tops.set(top, t);
      }
      const list = [...tops.values()].sort((a, b) => (a.name === "Unnamed") - (b.name === "Unnamed") || a.name.localeCompare(b.name));
      for (const t of list) {
        t.subs.sort((a, b) => a.name.localeCompare(b.name));
        // A category with only one way in needs no second level.
        if (t.subs.length === 1 && !t.own) t.subs = [];
      }
      out.push({ kind, count: total, tops: list.length === 1 && list[0].name === "Unnamed" ? [] : list });
    }
    return out;
  });

  function pick(e, key) {
    if (e.ctrlKey || e.metaKey) {
      app.cats = app.cats.includes(key) ? app.cats.filter((x) => x !== key) : [...app.cats, key];
    } else {
      app.cats = app.cats.length === 1 && app.cats[0] === key ? [] : [key];
    }
    runQuery();
  }
  function flip(key) {
    const next = new Set(open);
    next.has(key) ? next.delete(key) : next.add(key);
    open = next;
  }
  function toggle(list, v) {
    app[list] = app[list].includes(v) ? app[list].filter((x) => x !== v) : [...app[list], v];
    runQuery();
  }
  let filtered = $derived(app.cats.length || app.types.length || app.bundles.length || app.include || app.exclude || app.namedOnly || app.minSize);
  function clearAll() {
    Object.assign(app, { cats: [], types: [], bundles: [], include: "", exclude: "", namedOnly: false, minSize: 0 });
    runQuery();
  }
  let timer;
  const later = () => {
    clearTimeout(timer);
    timer = setTimeout(runQuery, 200);
  };
  const on = (key) => app.cats.includes(key);
</script>

<aside class="side">
  <div class="list">
    <button class="item" class:on={!app.cats.length} onclick={() => { app.cats = []; runQuery(); }}>
      <span class="label">All resources</span><span class="n">{app.summary.total.toLocaleString()}</span>
    </button>
    {#each tree as k (k.kind)}
      {@const key = `${k.kind}:`}
      <div class="node">
        {#if k.tops.length}
          <button class="twist" class:open={open.has(k.kind)} onclick={() => flip(k.kind)} aria-label={open.has(k.kind) ? "Collapse" : "Expand"}><ChevronRight size={13} /></button>
        {:else}<span class="twist"></span>{/if}
        <button class="item" class:on={on(key)} onclick={(e) => pick(e, key)}>
          <span class="label"><i class="dot {k.kind}"></i>{kindLabel(k.kind)}</span><span class="n">{k.count.toLocaleString()}</span>
        </button>
      </div>
      {#if open.has(k.kind)}
        {#each k.tops as t (t.name)}
          {@const tkey = `${k.kind}:${t.name}`}
          {@const tid = `${k.kind}/${t.name}`}
          <div class="node d1">
            {#if t.subs.length}
              <button class="twist" class:open={open.has(tid)} onclick={() => flip(tid)} aria-label={open.has(tid) ? "Collapse" : "Expand"}><ChevronRight size={13} /></button>
            {:else}<span class="twist"></span>{/if}
            <button class="item sub" class:on={on(tkey)} class:unnamed={t.name === "Unnamed"} onclick={(e) => pick(e, tkey)}>
              <span class="label">{t.name}</span><span class="n">{t.count.toLocaleString()}</span>
            </button>
          </div>
          {#if open.has(tid)}
            {#each t.subs as s (s.name)}
              {@const skey = `${k.kind}:${t.name}/${s.name}`}
              <div class="node d2">
                <span class="twist"></span>
                <button class="item sub" class:on={on(skey)} onclick={(e) => pick(e, skey)}>
                  <span class="label">{s.name}</span><span class="n">{s.count.toLocaleString()}</span>
                </button>
              </div>
            {/each}
          {/if}
        {/each}
      {/if}
    {/each}
  </div>

  <button class="group" onclick={() => (openTypes = !openTypes)}><ChevronRight size={14} class={openTypes ? "rot" : ""} />Types</button>
  {#if openTypes}
    <div class="list">
      {#each app.summary.types as [t, n]}
        <button class="item sub" class:on={app.types.includes(t)} onclick={() => toggle("types", t)}><span class="label mono">{t}</span><span class="n">{n.toLocaleString()}</span></button>
      {/each}
    </div>
  {/if}

  <button class="group" onclick={() => (openBundles = !openBundles)}><ChevronRight size={14} class={openBundles ? "rot" : ""} />Bundles</button>
  {#if openBundles}
    <div class="list">
      {#each app.summary.bundles as [b, name, n]}
        <button class="item sub" class:on={app.bundles.includes(b)} onclick={() => toggle("bundles", b)} title={b}>
          <span class="label" class:mono={!name}>{name ?? b}</span><span class="n">{n.toLocaleString()}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="patterns">
    <span class="section-title">Narrow down</span>
    <input bind:value={app.include} oninput={later} placeholder="Include, e.g. equipment/wep_*" spellcheck="false" />
    <input bind:value={app.exclude} oninput={later} placeholder="Exclude" spellcheck="false" />
    <select bind:value={app.minSize} onchange={runQuery} aria-label="Size">
      <option value={0}>Any size</option>
      <option value={10240}>10 KB and up</option>
      <option value={102400}>100 KB and up</option>
      <option value={1048576}>1 MB and up</option>
      <option value={10485760}>10 MB and up</option>
    </select>
    <label class="check"><input type="checkbox" bind:checked={app.namedOnly} onchange={runQuery} />Named only</label>
    {#if filtered}<button class="ghost clear" onclick={clearAll}>Clear filters</button>{/if}
  </div>
</aside>

<style>
  .side { height: 100%; overflow-y: auto; overflow-x: hidden; background: var(--bg); padding: 8px 6px; display: flex; flex-direction: column; gap: 4px; }
  .list { display: flex; flex-direction: column; }
  .node { display: flex; align-items: center; min-width: 0; }
  .node.d1 { padding-left: 14px; }
  .node.d2 { padding-left: 28px; }
  .twist { flex: none; width: 18px; height: 22px; padding: 0; border: 0; background: transparent; color: var(--faint); display: grid; place-items: center; }
  button.twist:hover { color: var(--text); background: transparent; }
  .twist :global(svg) { transition: transform 120ms; }
  .twist.open :global(svg) { transform: rotate(90deg); }
  .item { flex: 1; justify-content: space-between; height: 27px; border: 0; background: transparent; padding: 0 8px; border-radius: 4px; color: var(--muted); min-width: 0; }
  .item .label { display: flex; align-items: center; gap: 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .item:hover { background: var(--hover); color: var(--text); }
  .item.on { background: var(--accent-soft); color: var(--text); }
  .item.sub { height: 24px; font-size: 12px; }
  .item.unnamed { color: var(--faint); }
  .n { font-size: 11px; color: var(--faint); font-family: var(--mono); flex: none; padding-left: 6px; }
  .group { border: 0; background: transparent; color: var(--faint); font-size: 11px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; padding: 0 4px; margin-top: 8px; height: 26px; }
  .group:hover { color: var(--text); }
  .group :global(.rot) { transform: rotate(90deg); }
  .patterns { margin-top: auto; padding: 10px 4px 2px; display: flex; flex-direction: column; gap: 6px; border-top: 1px solid var(--line-soft); }
  .patterns input[type="text"], .patterns input:not([type]) { width: 100%; font-family: var(--mono); font-size: 12px; }
  .clear { align-self: flex-start; color: var(--accent); padding: 0 6px; }
</style>
