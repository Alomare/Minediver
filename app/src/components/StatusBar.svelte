<script>
  import { FolderOpen, X } from "@lucide/svelte";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { app, hiddenSelected } from "../lib/state.svelte.js";
  import { call } from "../lib/api.js";

  let job = $derived(app.job);
  let pct = $derived(job && job.total ? (job.done / job.total) * 100 : 0);
  let toastTimer;
  $effect(() => {
    if (app.toast) {
      clearTimeout(toastTimer);
      toastTimer = setTimeout(() => (app.toast = ""), 3500);
    }
  });
  let showErrors = $state(false);
</script>

<footer class="status">
  {#if job && !job.finished}
    <span>Extracting {job.done.toLocaleString()} of {job.total.toLocaleString()}</span>
    <div class="bar"><div style="width: {pct}%"></div></div>
    <span class="cur faint">{job.current}</span>
    <button class="ghost small" onclick={() => call("cancel_extract")}><X size={13} />Cancel</button>
  {:else if job}
    <span>
      {job.cancelled ? "Stopped" : "Done"}: {job.files.toLocaleString()} files from {job.done.toLocaleString()} resources{#if job.failed}, <button class="link bad" onclick={() => (showErrors = !showErrors)}>{job.failed} failed</button>{/if}
    </span>
    <button class="ghost small" onclick={() => openPath(job.out)}><FolderOpen size={13} />Show folder</button>
    <button class="ghost small icon" onclick={() => { app.job = null; showErrors = false; }} aria-label="Dismiss"><X size={13} /></button>
  {:else if app.summary}
    <span class="faint">
      {app.ids.length.toLocaleString()} shown of {app.summary.total.toLocaleString()}. {app.summary.named.toLocaleString()} named. Read in {app.summary.ms} ms.
    </span>
  {/if}
  <span class="spacer"></span>
  {#if app.selected.size}
    {@const hidden = hiddenSelected()}
    <span class="checked">{app.selected.size.toLocaleString()} checked{hidden ? `, ${hidden.toLocaleString()} not in this list` : ""}</span>
    <button class="ghost small" onclick={() => (app.selected = new Set())}><X size={13} />Clear</button>
  {/if}
  {#if app.toast}<span class="toast">{app.toast}</span>{/if}
  {#if app.summary && !app.summary.ffmpeg}<span class="faint">ffmpeg not found: MP4 and MP3 need it (Settings)</span>{/if}
</footer>

{#if showErrors && job?.errors?.length}
  <div class="errors">
    <div class="eh"><b>Failed</b><button class="ghost icon" onclick={() => (showErrors = false)} aria-label="Close"><X size={14} /></button></div>
    <pre>{job.errors.join("\n")}</pre>
  </div>
{/if}

<style>
  .status { grid-column: 1 / -1; display: flex; align-items: center; gap: 10px; padding: 0 10px; font-size: 12px; background: var(--panel); border-top: 1px solid var(--line); min-width: 0; }
  .bar { width: 180px; height: 4px; background: var(--raised); border-radius: 2px; overflow: hidden; flex: none; }
  .bar div { height: 100%; background: var(--accent); transition: width 80ms linear; }
  .cur { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; max-width: 40vw; }
  .small { height: 22px; font-size: 12px; padding: 0 6px; }
  .spacer { flex: 1; }
  .toast { color: var(--text); }
  .checked { color: var(--accent); }
  .small.icon { width: 22px; padding: 0; justify-content: center; }
  .link { border: 0; background: none; padding: 0; height: auto; text-decoration: underline; cursor: pointer; }
  .bad { color: var(--bad); }
  .errors { position: fixed; left: 10px; bottom: 36px; width: min(720px, 90vw); max-height: 40vh; display: flex; flex-direction: column; background: var(--raised); border: 1px solid var(--line); border-radius: 6px; box-shadow: var(--shadow); z-index: 30; }
  .eh { display: flex; justify-content: space-between; align-items: center; padding: 4px 4px 4px 10px; border-bottom: 1px solid var(--line); }
  pre { margin: 0; padding: 8px 10px; overflow: auto; font: 12px/1.5 var(--mono); user-select: text; }
</style>
