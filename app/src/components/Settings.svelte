<script>
  import { open } from "@tauri-apps/plugin-dialog";
  import { check } from "@tauri-apps/plugin-updater";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { getVersion } from "@tauri-apps/api/app";
  import { FolderOpen, RefreshCw, ExternalLink } from "@lucide/svelte";
  import { app, saveSettings, openGame } from "../lib/state.svelte.js";
  import { call } from "../lib/api.js";
  import Dialog from "./Dialog.svelte";

  const RELEASES = "https://github.com/Alomare/Minediver/releases/latest";
  let { onpickgame } = $props();
  let s = $state(structuredClone($state.snapshot(app.settings)));
  let ffmpegFound = $state("");
  let update = $state("");
  let download = $state(false);
  let version = $state("");
  getVersion().then((v) => (version = v));

  $effect(() => {
    call("find_ffmpeg").then((p) => (ffmpegFound = p ?? ""));
  });

  async function pick(field, title) {
    const d = await open({ directory: true, title });
    if (d) s[field] = d;
  }
  async function pickFfmpeg() {
    const f = await open({ title: "ffmpeg program" });
    if (f) s.ffmpeg = f;
  }
  async function save() {
    const gameChanged = s.game_dir !== app.settings.game_dir;
    await saveSettings(s);
    app.dialog = "";
    if (gameChanged) openGame(s.game_dir);
  }
  async function checkUpdate() {
    update = "Checking";
    try {
      const u = await check();
      if (!u) return (update = "Minediver is up to date.");
      if (app.portable) {
        // A portable copy replaces itself by hand: the installer would add a second, installed copy.
        update = `Version ${u.version} is out. Download the portable zip and unpack it over this folder.`;
        download = true;
        return;
      }
      update = `Installing ${u.version}`;
      await u.downloadAndInstall();
      update = "Installed. Restart Minediver to use it.";
    } catch (e) {
      update = `No update found (${e}).`;
    }
  }
</script>

<Dialog title="Settings" width={600}>
  <section>
    <span class="section-title">Folders</span>
    <label class="row"><span>Game</span><input bind:value={s.game_dir} placeholder="Found through Steam" spellcheck="false" /><button class="icon" onclick={() => pick("game_dir", "Helldivers folder")} aria-label="Choose"><FolderOpen size={14} /></button></label>
    <label class="row"><span>Output</span><input bind:value={s.output_dir} placeholder="Asked on the first extraction" spellcheck="false" /><button class="icon" onclick={() => pick("output_dir", "Output folder")} aria-label="Choose"><FolderOpen size={14} /></button></label>
    <label class="row"><span>ffmpeg</span><input bind:value={s.ffmpeg} placeholder={ffmpegFound || "Not found"} spellcheck="false" /><button class="icon" onclick={pickFfmpeg} aria-label="Choose"><FolderOpen size={14} /></button></label>
  </section>

  <section>
    <span class="section-title">Formats</span>
    <label class="row"><span>Models</span><select bind:value={s.options.model}><option value="glb">GLB (Blender, glTF)</option><option value="raw">Raw .unit</option></select></label>
    <label class="row"><span>Textures</span><select bind:value={s.options.texture}><option value="png">PNG</option><option value="dds">DDS (as stored)</option><option value="both">PNG and DDS</option></select></label>
    <label class="row"><span>Audio</span><select bind:value={s.options.audio}><option value="native">WAV or OGG (as stored)</option><option value="wav">WAV</option><option value="mp3">MP3</option><option value="ogg">OGG</option></select></label>
    <label class="row"><span>Video</span><select bind:value={s.options.video}><option value="mp4">MP4</option><option value="bik">Bink (as stored)</option></select></label>
    <label class="check"><input type="checkbox" bind:checked={s.options.model_textures} />Write the textures of each model next to the GLB</label>
    <label class="check"><input type="checkbox" bind:checked={s.options.raw} />Also write the raw resource files</label>
    <label class="check"><input type="checkbox" bind:checked={s.options.all_variants} />Every language variant, not only the first</label>
    <label class="check"><input type="checkbox" bind:checked={s.show_patches} />Show resources from mod patches (.patch_N files)</label>
  </section>

  <section>
    <span class="section-title">Look</span>
    <label class="row"><span>Theme</span><select bind:value={s.theme}><option value="dark">Dark</option><option value="light">Light</option><option value="system">Follow the system</option></select></label>
  </section>

  <section class="upd">
    <button onclick={checkUpdate}><RefreshCw size={14} />Check for updates</button>
    {#if download}<button onclick={() => openUrl(RELEASES)}><ExternalLink size={14} />Releases page</button>{/if}
    <span class="faint">{update || `Version ${version}${app.portable ? ", portable" : ""}`}</span>
  </section>

  {#snippet footer()}
    <button onclick={() => (app.dialog = "")}>Cancel</button>
    <button class="primary" onclick={save}>Save</button>
  {/snippet}
</Dialog>

<style>
  section { display: flex; flex-direction: column; gap: 8px; }
  .row { display: grid; grid-template-columns: 70px 1fr auto; align-items: center; gap: 8px; }
  .row select { grid-column: 2 / 4; }
  .row span { color: var(--muted); }
  .upd { flex-direction: row; align-items: center; gap: 10px; }
</style>
