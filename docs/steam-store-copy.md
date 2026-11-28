# Steam store copy (Penguin Pass)

Ready-to-paste sections for Steamworks. Pricing: **Free**. Licence: **MIT** (see `LICENSE`).

<!-- Short description character count: 215 (Steam limit 300) -->

## Short Description (paste block)

```text
Native indoor cycling game for smart trainers. Free & open source (MIT)—no subscription, no account. Ride offline with real hill resistance, structured ERG workouts, and simulated power when no trainer is connected.
```

## About This Game

```text
[h2]Free indoor cycling—no subscription, no account[/h2]
Turn your smart trainer into a native, offline-first cycling game. Penguin Pass is MIT-licensed and free: no monthly fee, no cloud lock-in, and no account required to ride.

[h2]Real smart-trainer resistance[/h2]
Penguin Pass reads power and cadence and can drive climb resistance over Bluetooth Low Energy:

[list]
[*] FTMS (Fitness Machine Service)
[*] Garmin / Tacx FE-C
[*] Wahoo legacy trainer control (WCPS)
[*] Zwift trainer service (0xFC82), including 24-speed virtual shifting on Cog / Ride setups
[*] Cycling Power and CSC cadence (read-only when no control point is present)
[*] Heart-rate straps over standard BLE heart-rate service
[/list]

Ridden and confirmed on a Garmin Tacx Neo 2T (power and hill resistance) with real Zwift Play, Click, and Ride controllers. Other trainers in the families listed in the repo ([code]docs/trainers.md[/code]) are supported via those same protocols—community BLE traces from [code]./x play --trace[/code] help the most.

[h2]Eight courses (+ reverse)[/h2]
[list]
[*] [b]Penguin Pass[/b] — built-in 5 km alpine loop
[*] [b]Rolling 5 km[/b], [b]Rolling 10 km[/b], [b]Rolling 20 km[/b]
[*] [b]Mountain 10 km[/b]
[*] [b]Desert 10 km[/b], [b]Desert 20 km[/b]
[*] [b]Flat 10 km[/b]
[/list]

Generated routes can be rerolled for a new seed. Every course can be ridden forward or in reverse (separate personal bests and ghosts).

Along the road: dynamic time of day (dawn, day, sunset, night with bike headlight), live weather (clear, cloudy, rain, snow or desert dust, fog), computer pace riders and drafting, timed sprint and climb segments, personal-best ghosts, lane fish and powerups (Feather, Aero, Burst), and three unlockable kits.

[h2]15 structured workouts + .zwo import[/h2]
Built-in plans from easy spins through sweet spot, threshold, VO₂-style intervals, sprints, and an FTP ramp test—15 workouts total with ERG target control, live ±25% difficulty bias, and training-load stats (TSS, IF, kJ).

Drop custom Zwift [code].zwo[/code] files into the [code]workouts[/code] folder beside your saved settings (see [code]docs/workouts.md[/code]).

[h2]Ride your way[/h2]
[list]
[*] Each ride saves as an offline [code].fit[/code] activity file
[*] Optional Strava upload with your own API keys
[*] Built-in LAN companion dashboard—open live metrics in a phone or tablet browser on the same Wi-Fi; no extra app to install
[*] Zwift Play / Click / Ride controller support for lanes, power, and menus
[*] Full keyboard, gamepad, and Steam Deck controls; chase and handlebar cameras
[/list]

Without a trainer, Up and Down (or triggers) step simulated power in 25 W increments.

[h2]Free & open source—contributors welcome[/h2]
Source, issues, and traces: [url=https://github.com/shotah/penguin_pass]https://github.com/shotah/penguin_pass[/url]

We welcome developers, 3D artists, Windows and macOS testers, and riders with Wahoo KICKR, Zwift Hub / Ride, JetBlack Victory, Elite, Saris, or other FTMS trainers—especially BLE traces from [code]./x scan[/code] and [code]./x play --trace[/code].
```

## Store Tags & Categories

Suggested Steam store tagging (adjust in Steamworks as needed):

| Kind | Values |
| --- | --- |
| **Genre** | Sports, Racing, Simulation, Indie |
| **Features** | Free To Play, Single-player, Partial Controller Support, Full Controller Support, Steam Cloud |
| **Themes** | Cycling |

Categories to enable where applicable: **Sports**, **Simulation**, **Free To Play**, **Single-player**, controller support flags, **Steam Cloud** (settings, records, arcade progress, and `rides/*.fit`—not `strava.toml`; see `docs/steam-release.md`).

## System Requirements

**Platform:** Linux / SteamOS (x86_64), built for **Steam Linux Runtime 4.0**.

### Minimum

| | |
| --- | --- |
| **OS** | SteamOS 3+ or Linux with Steam Linux Runtime 4.0 |
| **Processor** | 64-bit x86 CPU |
| **Memory** | 4 GB RAM |
| **Graphics** | Vulkan 1.2+ capable GPU |
| **Storage** | ~500 MB available space |
| **Network** | Optional (offline play; LAN dashboard and Strava need network when used) |
| **Additional** | Bluetooth LE adapter for real smart trainers and heart-rate straps (not required for simulated-rider mode) |

### Recommended

| | |
| --- | --- |
| **OS** | SteamOS or desktop Linux with current GPU drivers |
| **Graphics** | Dedicated or strong integrated GPU with Vulkan 1.2+ |
| **Additional** | Smart trainer with FTMS, FE-C, Wahoo control, or Zwift trainer service; BLE heart-rate strap optional |

**Note:** Simulated-rider mode works with no trainer—keyboard or gamepad only.

## Store Asset Checklist Reference

Capsules and library art (generated, Steam-required sizes):

- `packaging/steam/assets/` — header, main, small, vertical, library capsule, library hero, library logo, shortcut icon, app icon
- Source SVGs: `packaging/steam/assets/src/`
- Capsule size table: `docs/steam-assets.md`

Screenshots (1920×1080 for Steam store):

- `docs/img/shots/steam/` — title, alpine peloton, workout HUD, and other 1080p shots
- Additional ride shots: `docs/img/shots/`

## Credits / Notices

| Component | Licence | Where |
| --- | --- | --- |
| Penguin Pass source code | MIT | `LICENSE` (shipped in export as `licenses/LICENSE`) |
| Godot Engine 4.7.2 | MIT | `licenses/GODOT-LICENSE.txt`, `licenses/GODOT-COPYRIGHT.txt` after `./x fetch` + `./x export` |
| Bundled 3D art, textures, sky, music, SFX | CC0 1.0 (interface font: SIL OFL 1.1) | `docs/assets.md` — full author table |

CC0 art does not require attribution, but the credits table in `docs/assets.md` is the line to paste into the store credits section (Quaternius, Kenney, Poly Haven / ambientCG, OpenGameArt music authors, and sound credits listed there).