# Steam store assets

The store and library images for Penguin Pass are in
`packaging/steam/assets/`. Sizes are from Valve's
[Graphical Assets](https://partner.steamgames.com/doc/store/assets)
page. That page says the larger capsules have been required since August
2024. The same size table is in `docs/steam-release.md`, section 7.
Screenshots are not part of this set.

## Regenerate

From the repo root:

```bash
./packaging/steam/render-assets.sh
```

The script reads the SVGs in `packaging/steam/assets/src/`, renders them
with `rsvg-convert`, and writes the nine files below into
`packaging/steam/assets/`. It then checks each file's pixel size and
type, and exits non-zero if one does not match.

It needs `rsvg-convert`, `python3`, and GdkPixbuf
(`gi.repository.GdkPixbuf`). It does not install packages. Wordmarks use
Barlow Semi Condensed Bold, the game UI face at
`godot/assets/fonts/barlow-semi-condensed/BarlowSemiCondensed-Bold.ttf`
(weight 700). The script points fontconfig at that file for the run.

The shortcut icon and the app icon are rendered from `docs/img/logo.svg`.
The app-icon step drops the logo's rounded badge clip, renders a
184×184 PNG, and saves `app-icon-184x184.jpg` at JPEG quality 90. The
square is full-bleed sky and meadow. A rounded transparent corner would
turn black in a JPEG.

`library-hero.svg` is scenery only. The script stops if that SVG
contains a `<text>` or `<image>` element. Steam composites the library
logo on top of the hero.

## Files

One header file fills two Steamworks slots. Header capsule and Library
header are both 920×430.

| File | Steamworks slot | Pixels | Format |
| --- | --- | --- | --- |
| `packaging/steam/assets/header-capsule-920x430.png` | Header capsule, and Library header | 920 × 430 | PNG |
| `packaging/steam/assets/small-capsule-462x174.png` | Small capsule | 462 × 174 | PNG |
| `packaging/steam/assets/main-capsule-1232x706.png` | Main capsule | 1232 × 706 | PNG |
| `packaging/steam/assets/vertical-capsule-748x896.png` | Vertical capsule | 748 × 896 | PNG |
| `packaging/steam/assets/library-capsule-600x900.png` | Library capsule | 600 × 900 | PNG |
| `packaging/steam/assets/library-hero-3840x1240.png` | Library hero | 3840 × 1240 | PNG, opaque, no text and no logo |
| `packaging/steam/assets/library-logo-1280x720.png` | Library logo | 1280 × 720 | PNG, transparent background |
| `packaging/steam/assets/shortcut-icon-256x256.png` | Shortcut icon | 256 × 256 | PNG (Valve also accepts ICO) |
| `packaging/steam/assets/app-icon-184x184.jpg` | App icon | 184 × 184 | JPEG |

Source drawings for the capsules, the hero, and the library logo are the
SVGs in `packaging/steam/assets/src/`. The shared penguin is
`mascot.svg` in that directory, the same head as
`godot/assets/logo-mark.svg`.
