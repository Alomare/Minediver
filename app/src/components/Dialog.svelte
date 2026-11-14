<script>
  import { X } from "@lucide/svelte";
  import { app } from "../lib/state.svelte.js";

  let { title, width = 560, children, footer } = $props();
</script>

<div class="backdrop" role="presentation" onmousedown={(e) => e.target === e.currentTarget && (app.dialog = "")}>
  <div class="dialog" role="dialog" aria-label={title} style="width: min({width}px, 94vw)">
    <header>
      <h2>{title}</h2>
      <button class="ghost icon" onclick={() => (app.dialog = "")} aria-label="Close"><X size={16} /></button>
    </header>
    <div class="body">{@render children()}</div>
    {#if footer}<footer>{@render footer()}</footer>{/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: oklch(0 0 0 / 0.4); display: grid; place-items: center; z-index: 20; }
  .dialog { max-height: 86vh; display: flex; flex-direction: column; background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: var(--shadow); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 10px 10px 10px 16px; border-bottom: 1px solid var(--line); }
  h2 { margin: 0; font-size: 15px; font-weight: 700; }
  .body { padding: 14px 16px; overflow: auto; display: flex; flex-direction: column; gap: 14px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding: 10px 16px; border-top: 1px solid var(--line); }
</style>
