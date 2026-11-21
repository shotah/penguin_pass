# Art assets

Runtime files for the alpine course: glTF models, the textures those models
reference, tileable ground and road textures, and one sky HDRI. Another
worker loads them. Nothing in this directory is imported yet, and there is
no scene.

The credits, the load path, and what was rejected are in `docs/assets.md`.
`manifest.json` is the index the scatterer should read. Paths in it are
`res://` paths from the Godot project in `godot/`.

## Rules for adding a pack

1. The licence must allow a commercial Steam build. CC0 is the default.
   MIT, Apache-2.0, and CC-BY 4.0 are fine if the attribution line is copied
   into `docs/assets.md` exactly as the author asks. No NC, ND, SA, GPL,
   "personal use", or a custom store licence.
2. Put the pack in `third_party/<pack-slug>/` with the pack's own
   `LICENSE.txt`, a `SOURCE.md` (author, source page, download URL, licence
   URL, date, SHA-256 of the archive, what was kept), and only the files the
   game will load.
3. Prefer one glTF or GLB. Do not also commit the FBX, blend, or OBJ of the
   same mesh. Keep textures at 1K or 2K.
4. Add one `manifest.json` entry per file the game can scatter or apply.
   Every `path` must exist. Models need a triangle count and a measured
   bounding box from Godot, not a guess.
5. Stay near the size of the current library. The budget for this directory
   is about 80 MB.
6. Do not generate `.import` files here unless the load path in
   `docs/assets.md` has changed to the import pipeline.

## Layout

```text
third_party/<pack-slug>/
  LICENSE.txt
  SOURCE.md
  models/       glTF, bin, and any textures the glTF points at
  textures/     tileable ground and road maps
  sky/          HDRI, if the pack has one
```

Model textures live in `textures/` next to `models/`. The glTF image URIs
are `../textures/<file>`.
