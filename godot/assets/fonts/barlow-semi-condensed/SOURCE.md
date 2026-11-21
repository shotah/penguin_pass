# Barlow Semi Condensed

The game's one type family. Used for every label and number in the game
UI and in the mockups under `docs/img/ui/`.

| | |
| --- | --- |
| Family | Barlow Semi Condensed, version 1.408 |
| Designer | Jeremy Tribby (The Barlow Project Authors) |
| Licence | SIL Open Font License 1.1 (`OFL.txt` in this folder) |
| Publisher's source | https://github.com/jpt/barlow |
| Release | tag `v1.408`, commit `b4726ddf91525818e85e5fce111c285b9273d764` |
| Fetched | 2026-11-20 |

## Files

Static TrueType files, copied unmodified from `fonts/ttf/` of that commit.
`OFL.txt` is the repository's licence file from the same commit.

| File | Weight | SHA-256 |
| --- | --- | --- |
| `BarlowSemiCondensed-Medium.ttf` | 500 | `4998b693a190b76f2b11315844345a8546c1e2e6a320905507a9eed04edf8b48` |
| `BarlowSemiCondensed-SemiBold.ttf` | 600 | `bd299f4bc5b44d30be8d42e9bad3a5df7d66af1cd55d0ed72a8b8916360a1424` |
| `BarlowSemiCondensed-Bold.ttf` | 700 | `5bd6757e459a110da81b44532f0f43058e2dab2401688abca84f2fdbde193cbb` |
| `OFL.txt` | | `186d750eb496a4c17a76385f82be6aea2ac1cf2de074a811d63786cf374ea73f` |

Download URL pattern:

```text
https://raw.githubusercontent.com/jpt/barlow/b4726ddf91525818e85e5fce111c285b9273d764/fonts/ttf/<file>
https://raw.githubusercontent.com/jpt/barlow/b4726ddf91525818e85e5fce111c285b9273d764/OFL.txt
```

## Licence obligations

OFL 1.1 allows bundling and selling the font inside a commercial game. The
font files may not be sold on their own. Keep `OFL.txt` beside the files
in every build. The files here are unmodified. If a file is ever modified
(subset, renamed tables), the result must stay under OFL 1.1 and should
not be presented as the original; this font declares no Reserved Font
Name, so the family name may be kept.

## Tabular figures

The default digits are proportional. The fixed-width digits are the
OpenType feature `tnum`. Every live number in the game must turn it on,
or the digits shift as values change. In Godot, wrap the `FontFile` in a
`FontVariation` and set `opentype_features` to
`{ TextServerManager.get_primary_interface().name_to_tag("tnum"): 1 }`.

With `tnum` on, one digit advances 0.495 em in Medium, 0.507 em in
SemiBold, and 0.521 em in Bold. `docs/img/ui/tools/pp_ui.py` reads these
from the files to lay out the mockups.
