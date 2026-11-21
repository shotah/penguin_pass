# Music

Penguin Pass plays one bundled title track on the menu, and a shuffled
rotation of ride tracks during the ride. A folder of the rider's own files
can replace the ride rotation. The title track stays the bundled one.

Nobody on the machine that added the starter set has listened to it. The
lengths below are what Godot 4.7.2 reports from the files. Judge the tracks
by ear before shipping.

## What plays

`MusicPlayer` is a `Node` with two `AudioStreamPlayer`s. They crossfade on a
bus named `Music`, created in code if it is missing, and sent to Master. The
bus is the volume control, so a later effects bus can sit beside it.

The title track loops. Ride tracks play once, then hand off with a short
crossfade (half a second). Changing between the title and the ride takes
about 1.5 seconds. Setting the same scene again does not restart the stream.

The ride order is a shuffle. Every track plays once before any repeat, and
the next cycle does not open on the track that just finished when there are
at least two. A missing or unreadable file is skipped with one stderr line.
An empty library is silent.

Volume is a 0–100 slider. The bus gain is the square of that fraction,
converted with `20 log10`, so 100 is unity (0 dB) and the steps feel closer
to equal loudness than a straight gain. 60 is the default, about −8.9 dB.
0 mutes the bus and pauses the streams so they stop decoding. The pause
menu ducks the bus by another 12 dB without pausing.

The disk read for a track runs on a worker thread when the file is on disk
(`res://` in the editor, or an absolute path). The main thread only turns
the bytes into a stream. Inside an exported pack the file is not a loose
file, so Godot's `load_from_file` reads it from the PCK on the main thread.

## Formats

Godot loads these at run time, with no import step:

| Extension | Loader |
| --- | --- |
| mp3 | `AudioStreamMP3.load_from_file` |
| ogg | `AudioStreamOggVorbis.load_from_file` |
| wav | `AudioStreamWAV.load_from_file` |

Anything else is skipped. The reason names the extension, for example
`FLAC is not supported`. Hidden files are ignored. Names are matched
without caring about case.

A rider folder is an absolute path. It is used for the ride only when it
contains at least one playable file. Otherwise the bundled rides play.
`set_user_folder` returns a line such as
`3 tracks found, 2 skipped (FLAC is not supported)`.
"Now playing" for those files is the file name without the extension,
with hyphens and underscores turned into spaces.

## Bundled tracks

The starter set is in `godot/assets/music/`. `music.json` is the manifest.
`CREDITS.md` is the same list as a table.

Every bundled track is CC0 1.0. That is the rule for this folder: no CC-BY,
no "royalty free" site licence, nothing whose author is unclear. CC0 does
not require credit. The credits file is there so the Steam page can name
the authors anyway.

| Title | Artist | Role | Length |
| --- | --- | --- | --- |
| Zen Theme | beardalaxy | title | 2:32 |
| Cyber March | iamoneabe | ride | 4:04 |
| Bathing in the sun electronic | Pro Sensory | ride | 3:44 |
| Pro Sensory | Pro Sensory | ride | 2:56 |
| Rocking the Synth Machine | Pro Sensory | ride | 2:16 |
| Forever | Pro Sensory | ride | 1:59 |
| Intense Fight | Pro Sensory | ride | 1:46 |

24,670,800 bytes of audio. Intense Fight is a little under two minutes.
The others sit between two and four.

## Adding a track

1. Confirm CC0 on the track's own page. If the page is not CC0, do not add it.
2. Save the original mp3 or ogg. Do not re-encode. Use a lowercase hyphenated file name.
3. Keep the whole `godot/assets/music/` folder under 25 MB.
4. Add a row to `music.json` (`file`, `title`, `artist`, `role` of `title` or `ride`, `duration_secs`, `source_page`, `download_url`, `licence`, `retrieved`, `sha256`) and the same facts to `CREDITS.md`.
5. `duration_secs` is Godot's `get_length()` for that file, not a guess.
6. One `title` row. Extra title rows are ignored. `ride` rows join the shuffle.
7. Run `godot/tests/music_check.sh`. It checks the manifest, that each file decodes, and that the rotation does not repeat inside a cycle.

The export preset already includes `assets/*`, which covers this folder.
No preset edit is required for the files to land in the PCK as raw bytes,
the same way the glTF and images do. Do not add `.import` files beside the
audio. Godot would then pack its imported stream instead of the file
`load_from_file` expects.

## Checks

`pp-view` holds the playlist rules and the unit tests. The Godot class is
`MusicPlayer` in `pp-godot`.

```text
CRATE=pp-view ./x test
godot/tests/music_check.sh
```

The script prints `PENGUIN_PASS_MUSIC_OK tracks=<n>` and exits 0.
