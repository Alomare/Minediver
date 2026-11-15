# Changelog

## 0.1.0

- New: reads every bundle in the game folder and lists the files inside, with names where they are known.
- New: search, filters by kind, type and bundle, and include and exclude patterns.
- New: previews for textures, models in 3D, and sound bank clips.
- New: extraction of models to GLB with textures, textures to PNG or DDS, sound clips to WAV, OGG or MP3, videos to MP4, and game text to a tab-separated list.
- New: characters and enemies export with their skeleton, bone names and skin weights.
- New: details for each model: parts, vertex counts, material slots and the textures each slot uses.
- New: name lists: load, save, and check guessed names against the game.
- New: mod builder (experimental): edited models, with or without bones, and edited textures become an Obsidian mod folder. Textures keep the compression of the game texture they replace.
- New: mod projects can add new models and textures under new names next to the game's, instead of replacing them.
- New: command line tool, `minediver-cli`, for listing, extracting and building mods.
- New: categories on the left for every kind (Weapons, Enemies, Helldivers, Vehicles, Stratagems and more), with weapon classes from the game loadout. Textures take the category of the model that uses them.
- New: check boxes on every row. Ticked rows stay ticked across searches, for one extraction from several searches.
- New: panels resize by dragging the borders, and the sizes are kept.
- New: large preview in place of the list, with stepping through the list.
- New: extract everything shown, filter to one bundle, copy hashes, save the list as a table, sort by category, size filter.
- New: portable Windows zip that keeps settings next to the program.
