# Stylized Nature MegaKit, standard edition

- Author: Quaternius
- Source page: https://quaternius.com/packs/stylizednaturemegakit.html
- Licence: CC0 1.0 Universal, https://creativecommons.org/publicdomain/zero/1.0/
- The pack's own notice is `LICENSE.txt` in this directory.
- Retrieved: 2026-11-20

The download button on quaternius.com and the itch.io page are a
pay-what-you-want checkout, so they were not used. The file here is the
standard edition the author posted on OpenGameArt (68 of 116 models).

- OpenGameArt page: https://opengameart.org/content/stylized-nature-megakit
- Download: https://opengameart.org/sites/default/files/stylized_nature_megakitstandard.zip
- Archive SHA-256: `298f6732b872e4cf7b30e6e7abf9641c7f6dc6b326df37ac089533ed7e3d58c9`
- Archive size: 104,088,529 bytes

## Kept

29 glTF models and the 11 textures they reference. `Bush_Common` was
exported and then removed: its only leaf card averages red (about 166, 22, 22),
and the rest of the kept set is green. Mesh bytes are the
author's. glTF JSON was rewritten only so each model lives in `models/`
under a snake_case name and its image URIs point at `../textures/`.

## Dropped

FBX, FBX (Unity), OBJ, MTL, preview photos, `desktop.ini`, and the
duplicate `Textures/` folder. Models left out of the alpine set: the five
twisted trees (about 9,000–10,000 triangles each, plus a second bark set),
`CommonTree_1` and `CommonTree_2` (the two heaviest broadleaf trees),
`DeadTree_1` through `DeadTree_3`, the heavier rock-path pieces, most
pebbles, petals, both mushrooms, and the plants and flowers that duplicate
a kept one. Textures with no remaining user: twisted-tree bark, `Leaves_TwistedTree_C.png`
(only used by the removed red bush), the non-color leaf variants,
`Leaves_GiantPine_C.png` (no giant-pine model in the free edition),
`Mushrooms.png`, and `Rocks_Desert_Diffuse.png` (no desert model in the
free edition).

Bark materials are exported as alpha mask with cutoff 0.2. The bark images
are fully opaque, so the mask does not punch holes. Leaf cards are a real
alpha mask.
