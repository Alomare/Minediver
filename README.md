# Minediver

Minediver reads the game files of Helldivers (2015), shows what is inside, and converts it to formats other tools open: models to GLB for Blender, textures to PNG, sounds to WAV or OGG, videos to MP4. It also works the other way: edited models and textures go back into an Obsidian mod folder.

The game stores every file under a hash instead of a name. Minediver ships the names found inside the game files themselves, and takes more from shared name lists.

## Install

On Windows, download `Minediver_<version>_x64-setup.exe` from the Releases page and run it. A portable zip (`Minediver-<version>-portable-windows.zip`) holds the same program and the command line tool and needs no install: unpack it anywhere. The portable copy keeps settings and cache in a `data` folder next to the program and leaves nothing in the user folders.

On Linux, download the `.AppImage`, mark it executable (`chmod +x Minediver_*.AppImage`) and run it. A `.deb` is there too.

Minediver checks for updates on request (Settings, Check for updates). The installed copy downloads and installs the update in place; a portable copy links to the new zip instead.

Optional: [ffmpeg](https://ffmpeg.org/download.html) for MP4 videos and MP3 audio. Minediver finds it on the PATH, or set the path in Settings. WAV, OGG, PNG, DDS and GLB need nothing else.

## First run

Minediver looks for Helldivers in the Steam libraries. If the game is somewhere else, pick the game folder (the one that holds `contents`). Reading the index of every bundle takes a few seconds the first time and well under a second after that.

## Finding things

- Type in the search box (Ctrl F) to match names, types or hashes.
- The tree on the left sorts every kind into categories read from the game names: Models, Weapons, Primary; Textures, Enemies, Terminids; and so on. Weapon classes follow the game loadout (primary, sidearm, support weapon). Textures rarely have names, so a texture takes the category of the model that uses it; those fill in a few seconds after the game opens. Ctrl click to combine several categories.
- Types and Bundles on the left filter further. Patterns take globs such as `equipment/wep_*`, and the size list hides small files.
- Click a column header to sort. Arrow keys move, Home and End jump.
- Tick the box of a row to add it to the selection (Space, Ctrl click, or Shift click for a range; the box in the header ticks every row shown). Ticked rows stay ticked when the search or the filters change, so one extraction can collect files from several searches. The status bar counts them and has Clear.
- The panel on the right shows the focused file: textures as images, models in 3D (drag to turn, scroll to zoom, Reset view), sound banks as a list of clips to play. It also lists the parts, material slots and textures of a model; click a texture to find it in the list.
- Enter, a double click or the button in the corner of the preview opens a large preview in place of the list. Up and Down step through the list from there, and Esc goes back. Click a texture in the large preview for actual size.
- Drag the borders between the panels, and between the preview and the details, to resize them. A double click on a border resets it. Sizes are kept for the next start.
- Right-click a row for more: extract everything shown, show only that bundle, copy names or hashes, or save the list as a tab-separated table.

## Extracting

Tick files, or focus one, and press Extract (Ctrl E). The first time, Minediver asks for an output folder; Ctrl Shift E asks every time. Files keep the folder structure of their game names, and unnamed files go under `_unnamed/<type>/<hash>`.

Formats (Settings):

| Kind | Default | Other choices |
|---|---|---|
| Models | GLB with textures inside, plus the textures as PNG next to it | raw `.unit` |
| Textures | PNG | DDS as stored, or both |
| Audio | WAV for PCM clips, OGG for Vorbis clips | WAV, MP3, OGG (ffmpeg) |
| Video | MP4 (ffmpeg) | Bink as stored |
| Game text (`strings`) | a text file, one entry per line: key hash, a tab, the text | |
| Everything else | the resource as stored | |

## A model in Blender

1. Find the model, for example search `wep_assaultrifle_01`, and extract it.
2. In Blender: File, Import, glTF 2.0, and pick the `.glb`. The model comes in at the game scale, standing the way the game has it, with the diffuse texture applied.
3. Each part of the model is an object named `geometry_0`, `geometry_1` and so on, and each material is named after the game's material slot (`m_ar` for the assault rifle). Keep these names if the model is going back into the game.

## Making a mod (experimental)

Repacking covers models, with or without bones, and textures.

1. Select the models or textures to change, right-click, **Start a mod from this**, and choose an empty folder. Minediver writes each model as GLB, the textures each one uses as PNG, and a `mod.json` that lists them.
2. Edit the files. Keep the file names. In Blender, edit the `geometry_N` objects and export with File, Export, glTF 2.0 (`.glb`), over the same file. A new part should be joined into one of the existing `geometry_N` objects, and should use one of the existing materials. For characters, keep the armature and its bone names, and export with skinning on.
3. Open the Mod builder, pick the project folder and press **Build mod**. The result in `build/<mod id>` is an Obsidian mod folder: import it in Obsidian, deploy, and start the game.

What the builder does: a model's parts are rebuilt from the GLB in the game's own vertex layout, with everything else in the unit (nodes, physics, scripts) kept as it was. A PNG becomes a DDS in the same compressed format as the game texture it replaces, with all mip levels. A `.dds` or `.unit` file in the project is used as it is. Remove a line from the list to leave that file alone.

Models with bones (characters, enemies) export with their skeleton and skin weights, so they come into Blender rigged and in the pose the game binds them in. Limits for now: a skinned part keeps the bone sets of the game (a triangle can use only bones one of its sets holds; weights on other bones are left out, and the builder says how many), a part can hold at most 65,535 vertices (more where the game part already uses 32-bit indices), and replacing a model changes every copy of that model in the game.

### A new model next to the game's

Tick **Add as new resources and keep the game's** when starting the project (or pass `--new-prefix mymod/` to `minediver-cli start`). Each entry then gets a new name and a `base`, the game resource it comes from:

```json
{ "type": "unit", "name": "mymod/rifle_long", "file": "rifle_long.glb", "base": "equipment/wep_assaultrifle_01" }
```

The build adds the new unit to the bundles of the base, with a copy of the base material under the new name. Edited textures that name a base become new textures too, and the copied material points at them; textures left untouched stay the game's. The game's own rifle does not change. Where the new model shows up (a new weapon, a pickup) is up to game scripts, which is Obsidian mod work: `World.spawn_unit(world, "mymod/rifle_long", position)` places it.

## Names

The Names window (book icon) shows how many files have names. **Load a name list** takes a text file with one name per line, or `<16 hex digits> <name>` per line; Minediver keeps loading that list on later starts. **Try names** checks guessed paths against the game and keeps the ones that match. **Save unnamed hashes** writes the hashes still missing a name, for sharing.

## Command line

`minediver-cli` does the same work without the window:

```
minediver-cli census
minediver-cli list --kind model --include "equipment/wep_*"
minediver-cli extract --kind texture --text rifle -o out --texture both
minediver-cli info equipment/wep_assaultrifle_01 --type unit
minediver-cli start --type unit --include "equipment/wep_assaultrifle_01" <project folder> --id me.rifle --name "My rifle"
minediver-cli pack <project folder> -o <mod folder>
minediver-cli harvest names.txt
```

`--game <folder>` points at the game when Steam doesn't know it. `--names <file>` loads extra name lists.

## Building from source

Needs Rust (stable) and Node 22 or newer. On Linux also the WebKitGTK 4.1 development packages (see `.github/workflows/ci.yml`).

```
cd app
npm install
npx tauri dev            # development window
npx tauri build          # installer or AppImage
cargo test -p minediver-core
```

## License

Not decided yet. All rights reserved until a license file is added.
