# Contributing to Penguin Pass

Thanks for looking at Penguin Pass. It is a **free, MIT-licensed** native indoor-cycling game built in **Rust** and **Godot 4.7.2**, aimed first at Linux and SteamOS but open to Windows and macOS ports.

We welcome help with code, smart-trainer testing, desktop packaging on other OSes, courses and workouts, and **CC0** (or cleanly MIT/BSD-compatible) art and audio. See [`docs/assets.md`](docs/assets.md) for how assets are licensed and credited.

## Help needed most

### Real-world smart-trainer traces

The Neo 2T is the trainer ridden in this repo so far. Traces from other hardware speed up compatibility work for everyone on the list in [`docs/trainers.md`](docs/trainers.md), especially:

- Wahoo KICKR / CORE / SNAP
- Zwift Hub, Hub One, Zwift Ride
- JetBlack Victory
- Elite Direto / Suito / Justo
- Saris H3

**Before you start:** wake the trainer, then close Zwift, Zwift Companion, and any other app that might hold the Bluetooth connection (trainers accept one controller at a time).

1. List nearby devices:

   ```bash
   ./x scan
   ```

2. Record a ride with BLE tracing:

   ```bash
   ./x play --trace
   ```

   The terminal prints the output folder, for example `target/traces/20261121-093000/`. The main artifact is **`target/traces/<date-time>/ble.csv`** (plus `ride.csv` and other session logs).

Follow the standing-start and optional ERG checks in [`docs/trainer-trace-plan.md`](docs/trainer-trace-plan.md). Open an issue or PR with what worked, what failed, and attach or describe the trace folder (redact anything personal).

### Windows and macOS testing

CI builds and export presets exist, but hardware testing on those desktops is still open. See [`docs/cross-platform.md`](docs/cross-platform.md) and GitHub Releases artifacts from tagged builds. Reports on Bluetooth pairing, audio, and full-screen export are especially useful.

### Code, UI/UX, and CC0 art/audio

- Gameplay, trainer protocols, and menus: `crates/pp-*` and `godot/`
- New roadside props, kits, music, or SFX: must be **CC0 1.0** (or another licence listed as acceptable in [`docs/assets.md`](docs/assets.md)) with `LICENSE.txt` and `SOURCE.md` in the pack directory
- Do not commit Godot `.import` files or `godot/.godot/` while the project loads assets at runtime (see `docs/assets.md`)

## Development setup

### Requirements

- **Linux + Podman** (SteamOS ships Podman)—recommended; all Cargo builds run in the toolbox container, **or**
- Native **Rust** (via rustup) **+ Godot 4.7.2** on the host (`./x fetch` downloads the official editor and export templates)

SteamOS has no host C toolchain; use the container unless you set `PP_NATIVE=1` on a machine that can link Rust.

### `./x` workflow

One-time:

```bash
./x fetch    # Godot 4.7.2 editor + export templates (checksum-checked)
./x toolbox  # build container image (once)
```

Day to day:

| Command | Purpose |
| --- | --- |
| `./x build` | Build the GDExtension into `godot/bin/` |
| `./x play` | Build if needed and run the game (`--` passes flags to the game) |
| `./x test` | Run Rust tests |
| `./x lint` | `fmt` + `clippy` |
| `./x typecheck` | `cargo check` workspace |
| `./x smoke` | Headless Godot load smoke test |
| `./x export` | Export player build to `dist/penguin-pass-linux-x86_64/` |

Headless or CI-safe play (no Bluetooth, no audio device):

```bash
./x play -- --no-bluetooth --fake-trainer --audio-driver Dummy
```

Narrow tests to one crate: `CRATE=pp-core ./x test`. Release library: `PROFILE=release ./x build`.

Git hooks (optional): `lefthook install` — see [`docs/dev-workflow.md`](docs/dev-workflow.md).

### Workspace map (brief)

| Path | Role |
| --- | --- |
| `crates/pp-ftms` | BLE GATT codecs (FTMS, FE-C, Wahoo, Zwift, power, HR, CSC)—no I/O |
| `crates/pp-trainer` | Trainer trait, BlueZ BLE backend, simulated trainer |
| `crates/pp-course` / `pp-routegen` / `pp-world` | Course data, generated routes, world chunks |
| `crates/pp-session` | Ride loop, settings, workouts, FIT export |
| `crates/pp-cast` | LAN TV dashboard |
| `crates/pp-godot` | GDExtension: scene, HUD, input, rendering glue |
| `godot/` | Godot 4 project, scenes, export presets |

## Pull request checklist

1. **Keep changes focused** — smallest diff that solves the problem.
2. **Run checks before opening a PR:**

   ```bash
   ./x typecheck
   ./x lint
   ./x test
   ./x smoke
   ```

3. **No real Bluetooth in automated tests** — use `--no-bluetooth`, `--fake-trainer`, and dummy audio as above.
4. **Update docs** when you change behaviour, trainer protocols, or player-facing options (`docs/*.md`, and `README.md` when appropriate).
5. **Art/audio** — confirm licence files and add rows to `docs/assets.md` when you add packs.

By contributing, you agree your code is licensed under the same **MIT** licence as the project (`LICENSE`). Art you submit should match the CC0 (or stated compatible) rules in `docs/assets.md`.

Questions and ideas: [GitHub Issues](https://github.com/shotah/penguin_pass/issues).