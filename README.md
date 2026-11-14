# Minediver

Minediver reads the game files of Helldivers (2015), shows what is inside, and converts it to formats other tools open: models to GLB for Blender, textures to PNG, sounds to WAV or OGG, videos to MP4. It also works the other way: edited models and textures go back into an Obsidian mod folder.

The game stores every file under a hash instead of a name. Minediver ships the names found inside the game files themselves, and takes more from shared name lists.

## Install

On Windows, download `Minediver_<version>_x64-setup.exe` from the Releases page and run it. A portable zip (`Minediver-<version>-portable-windows.zip`) holds the same program and the command line tool and needs no install.

On Linux, download the `.AppImage`, mark it executable (`chmod +x Minediver_*.AppImage`) and run it. A `.deb` is there too.

Minediver checks for updates on request (Settings, Check for updates) and installs them in place.

Optional: [ffmpeg](https://ffmpeg.org/download.html) for MP4 videos and MP3 audio. Minediver finds it on the PATH, or set the path in Settings. WAV, OGG, PNG, DDS and GLB need nothing else.

## First run

Minediver looks for Helldivers in the Steam libraries. If the game is somewhere else, pick the game folder (the one that holds `contents`). Reading the index of every bundle takes a few seconds the first time and well under a second after that.

## Finding things

- Type in the search box (Ctrl F) to match names, types or hashes.
- The chips next to the search box and the list on the left narrow by kind (models, textures, audio and so on). Ctrl click a kind to combine several.
- Types and Bundles on the left filter further. Patterns take globs such as `equipment/wep_*`.
- Click a column header to sort. Arrow keys move, Shift extends the selection, Ctrl A selects everything shown.
- The panel on the right shows the selected file: textures as images, models in 3D (drag to turn, scroll to zoom), sound banks as a list of clips to play. It also lists the parts, material slots and textures of a model.

## Extracting

Select files and press Extract (Ctrl E). The first time, Minediver asks for an output folder; Ctrl Shift E asks every time. Files keep the folder structure of their game names, and unnamed files go under `_unnamed/<type>/<hash>`.

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

Repacking covers models without bones (weapons, props, vehicle parts that don't bend) and textures.

1. Select the models or textures to change, right-click, **Start a mod from this**, and choose an empty folder. Minediver writes each model as GLB, the textures each one uses as PNG, and a `mod.json` that lists them.
2. Edit the files. Keep the file names. In Blender, edit the `geometry_N` objects and export with File, Export, glTF 2.0 (`.glb`), over the same file. A new part should be joined into one of the existing `geometry_N` objects, and should use one of the existing materials.
3. Open the Mod builder, pick the project folder and press **Build mod**. The result in `build/<mod id>` is an Obsidian mod folder: import it in Obsidian, deploy, and start the game.

What the builder does: a model's parts are rebuilt from the GLB in the game's own vertex layout, with everything else in the unit (nodes, physics, scripts) kept as it was. A PNG becomes a DDS in the same compressed format as the game texture it replaces, with all mip levels. A `.dds` or `.unit` file in the project is used as it is. Remove a line from the list to leave that file alone.

Limits for now: models with bones (characters, enemies) can be extracted but not repacked, a part can hold at most 65,535 vertices, and replacing a model changes every copy of that model in the game. Adding a brand new weapon next to the old ones needs game scripts as well, which is Obsidian mod work outside Minediver.

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
