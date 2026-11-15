<script>
  import { previewUrl, kindLabel } from "../lib/api.js";
  import ModelView from "./ModelView.svelte";

  let { info, id, big = false } = $props();
  let actual = $state(false);
</script>

{#key info.hash + info.bundle}
  <div class="preview" class:flat={info.kind !== "texture"} class:big class:actual>
    {#if info.kind === "texture" && info.texture}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <img src={previewUrl(`texture/${id}`)} alt={info.name} onclick={() => big && (actual = !actual)} title={big ? (actual ? "Fit to the window" : "Actual size") : ""} />
    {:else if info.kind === "model" && info.model}
      <ModelView url={previewUrl(`model/${id}`)} />
    {:else}
      <span class="kind">{kindLabel(info.kind)}</span>
    {/if}
  </div>
{/key}

<style>
  .preview {
    width: 100%; height: 100%; display: grid; place-items: center; overflow: hidden;
    background: repeating-conic-gradient(var(--checker-a) 0 25%, var(--checker-b) 0 50%) 0 0 / 16px 16px;
  }
  .preview.flat { background: var(--panel); }
  .preview img { max-width: 100%; max-height: 100%; object-fit: contain; }
  .preview.big img { cursor: zoom-in; }
  .preview.actual { overflow: auto; place-items: start; }
  .preview.actual img { max-width: none; max-height: none; cursor: zoom-out; }
  .kind { color: var(--faint); font-weight: 600; background: var(--bg); padding: 4px 10px; border-radius: 4px; }
</style>
