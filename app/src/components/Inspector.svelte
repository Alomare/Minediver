<script>
  import { Play, Square, Copy } from "@lucide/svelte";
  import { app, focusedId } from "../lib/state.svelte.js";
  import { call, previewUrl, formatSize, kindLabel } from "../lib/api.js";
  import ModelView from "./ModelView.svelte";

  let info = $state(null);
  let error = $state("");
  let playing = $state(-1);
  let audio;
  let timer;

  let id = $derived(focusedId());

  $effect(() => {
    const cur = id;
    clearTimeout(timer);
    stop();
    if (cur === undefined) {
      info = null;
      return;
    }
    // A short wait, so holding an arrow key doesn't load every row on the way.
    timer = setTimeout(async () => {
      try {
        const r = await call("inspect", { id: cur });
        if (cur === id) {
          info = r;
          error = "";
        }
      } catch (e) {
        error = String(e);
      }
    }, 60);
  });

  function play(n) {
    stop();
    audio = new Audio(previewUrl(`clip/${id}/${n}`));
    audio.onended = () => (playing = -1);
    audio.play().catch(() => (app.toast = "This clip could not be played here. Extract it instead."));
    playing = n;
  }
  function stop() {
    audio?.pause();
    audio = null;
    playing = -1;
  }

  const copy = (t) => {
    navigator.clipboard.writeText(t);
    app.toast = "Copied";
  };
</script>

<aside class="insp">
  {#if !info}
    <p class="hint">{error || "Select a resource to see it here."}</p>
  {:else}
    {#key info.hash + info.bundle}
      <div class="preview">
        {#if info.kind === "texture" && info.texture}
          <img src={previewUrl(`texture/${id}`)} alt={info.name} />
        {:else if info.kind === "model" && info.model}
          <ModelView url={previewUrl(`model/${id}`)} />
        {:else}
          <span class="kind">{kindLabel(info.kind)}</span>
        {/if}
      </div>
    {/key}

    <div class="meta">
      <h2 class:mono={!info.named} title={info.name}>{info.name}</h2>
      <dl>
        <dt>Type</dt><dd>{info.type}</dd>
        <dt>Hash</dt><dd class="mono copyable" onclick={() => copy(info.hash)} role="presentation">{info.hash}<Copy size={12} /></dd>
        <dt>Bundle</dt><dd class="mono">{info.bundle_name ?? info.bundle}</dd>
        <dt>Size</dt><dd>{formatSize(info.size)}{info.variants.length > 1 ? `, ${info.variants.length} language variants` : ""}</dd>
        {#if info.copies > 1}<dt>Copies</dt><dd>In {info.copies} bundles</dd>{/if}
        {#if info.texture}
          <dt>Image</dt><dd>{info.texture.width} x {info.texture.height}, {info.texture.mips} mips</dd>
          <dt>Format</dt><dd>{info.texture.format}{info.texture.cube ? ", cube map" : ""}</dd>
        {/if}
        {#if info.model}
          <dt>Mesh</dt><dd>{info.model.geometries.length} parts, {info.model.vertices.toLocaleString()} vertices, {info.model.triangles.toLocaleString()} triangles</dd>
        {/if}
        {#if info.error}<dt>Problem</dt><dd class="bad">{info.error}</dd>{/if}
      </dl>

      {#if info.model}
        <h3 class="section-title">Parts</h3>
        <table>
          <tbody>
            {#each info.model.geometries as g, i}
              <tr>
                <td class="mono">geometry_{i}</td>
                <td>{g.vertices.toLocaleString()} v</td>
                <td>{g.slots.join(", ")}</td>
                <td>{g.skinned ? "skinned" : ""}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if info.model.textures.length}
          <h3 class="section-title">Textures</h3>
          <table>
            <tbody>
              {#each info.model.textures as t}
                <tr><td>{t.slot}</td><td>{t.param}</td><td class="mono">{t.name ?? t.texture}</td></tr>
              {/each}
            </tbody>
          </table>
        {/if}
      {/if}

      {#if info.audio}
        <h3 class="section-title">{info.audio.clips.toLocaleString()} clips</h3>
        <div class="clips">
          {#each info.audio.list as c, n}
            <div class="clip" class:on={playing === n}>
              <button class="ghost icon" onclick={() => (playing === n ? stop() : play(n))} aria-label={playing === n ? "Stop" : "Play"}>
                {#if playing === n}<Square size={13} />{:else}<Play size={13} />{/if}
              </button>
              <span class="mono">{c.id}</span>
              <span class="faint">{c.codec}, {c.rate / 1000} kHz{c.seconds ? `, ${c.seconds.toFixed(1)} s` : ""}</span>
            </div>
          {/each}
        </div>
      {/if}
      {#if info.kind === "video"}<p class="faint note">Extract to get an MP4 (ffmpeg converts the Bink file).</p>{/if}
    </div>
  {/if}
</aside>

<style>
  .insp { grid-area: insp; overflow: hidden auto; scrollbar-gutter: stable; background: var(--bg); border-left: 1px solid var(--line); display: flex; flex-direction: column; }
  .hint { color: var(--faint); text-align: center; margin: 40px 16px; }
  .preview {
    flex: none; height: 260px; display: grid; place-items: center; border-bottom: 1px solid var(--line);
    background: repeating-conic-gradient(var(--checker-a) 0 25%, var(--checker-b) 0 50%) 0 0 / 16px 16px;
  }
  .preview img { max-width: 100%; max-height: 100%; object-fit: contain; image-rendering: auto; }
  .kind { color: var(--faint); font-weight: 600; background: var(--bg); padding: 4px 10px; border-radius: 4px; }
  .meta { padding: 12px 14px 20px; user-select: text; }
  h2 { font-size: 14px; font-weight: 700; margin: 0 0 10px; word-break: break-all; }
  dl { display: grid; grid-template-columns: 64px 1fr; gap: 4px 10px; margin: 0 0 14px; }
  dt { color: var(--faint); }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; }
  .copyable { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
  .copyable:hover { color: var(--accent); }
  .bad { color: var(--bad); }
  h3 { margin: 14px 0 6px; }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  td { padding: 3px 6px 3px 0; border-bottom: 1px solid var(--line-soft); color: var(--muted); vertical-align: top; overflow-wrap: anywhere; }
  .clips { display: flex; flex-direction: column; }
  .clip { display: flex; align-items: center; gap: 8px; height: 28px; font-size: 12px; border-radius: 4px; }
  .clip.on { background: var(--accent-soft); }
  .clip .icon { height: 24px; width: 24px; }
  .note { margin-top: 10px; }
</style>
