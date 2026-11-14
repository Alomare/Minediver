<script>
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { FileUp, FileDown } from "@lucide/svelte";
  import { app, runQuery } from "../lib/state.svelte.js";
  import { call } from "../lib/api.js";
  import Dialog from "./Dialog.svelte";

  let cov = $state(null);
  let guesses = $state("");
  let result = $state("");

  async function load() {
    cov = await call("coverage");
  }
  $effect(() => {
    load();
  });

  async function refresh(msg) {
    result = msg;
    app.rowCache = new Map();
    await load();
    runQuery();
  }
  async function importList() {
    const f = await open({ title: "Name list", filters: [{ name: "Text", extensions: ["txt", "csv", "list"] }, { name: "Any", extensions: ["*"] }] });
    if (!f) return;
    try {
      const n = await call("import_names", { path: f });
      refresh(`${n.toLocaleString()} new names loaded. The list loads again on every start.`);
    } catch (e) {
      result = String(e);
    }
  }
  async function exportList(unresolved) {
    const f = await save({ title: unresolved ? "Unnamed hashes" : "Name list", defaultPath: unresolved ? "unnamed.txt" : "names.txt" });
    if (f) {
      await call("export_names", { path: f, unresolved });
      result = `Saved ${f}`;
    }
  }
  async function tryNames() {
    const hits = await call("try_names", { text: guesses });
    refresh(hits.length ? `${hits.length} names matched a resource and were added for this session.` : "None of these names match a resource.");
  }
</script>

<Dialog title="Names" width={620}>
  <p class="muted lead">
    The game stores resources by hash, not by name. Minediver ships the names found in the game files; more come from shared lists or from guessing below.
  </p>
  {#if cov}
    <div class="total">
      <b>{cov.named.toLocaleString()}</b> of {cov.total.toLocaleString()} resources named ({((cov.named / cov.total) * 100).toFixed(1)}%), {cov.known.toLocaleString()} names known
    </div>
    <div class="types">
      {#each cov.by_type.slice(0, 14) as [t, n, k]}
        <div class="t">
          <span class="mono">{t}</span>
          <div class="bar"><div style="width: {(k / n) * 100}%"></div></div>
          <span class="faint mono">{k}/{n}</span>
        </div>
      {/each}
    </div>
  {/if}
  <div class="actions">
    <button onclick={importList}><FileUp size={14} />Load a name list</button>
    <button onclick={() => exportList(false)}><FileDown size={14} />Save known names</button>
    <button onclick={() => exportList(true)}><FileDown size={14} />Save unnamed hashes</button>
  </div>
  <div class="guess">
    <span class="section-title">Try names</span>
    <textarea bind:value={guesses} rows="5" placeholder={"One path per line, for example\nunits/weapons/rifle_01/textures/rifle_01_df"} spellcheck="false"></textarea>
    <div><button disabled={!guesses.trim()} onclick={tryNames}>Check names</button></div>
  </div>
  {#if result}<p class="result">{result}</p>{/if}
</Dialog>

<style>
  .lead { margin: 0; }
  .total b { font-size: 15px; }
  .types { display: grid; grid-template-columns: 1fr 1fr; gap: 4px 18px; }
  .t { display: grid; grid-template-columns: 130px 1fr 80px; align-items: center; gap: 8px; font-size: 12px; }
  .t .faint { text-align: right; }
  .bar { height: 4px; background: var(--raised); border-radius: 2px; overflow: hidden; }
  .bar div { height: 100%; background: var(--accent); }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; }
  .guess { display: flex; flex-direction: column; gap: 6px; }
  .result { margin: 0; color: var(--muted); }
</style>
