# TV dashboard

Penguin Pass serves a live ride page on the home network. A TV, tablet, or
phone on the same Wi-Fi opens it in a browser. The page is ride telemetry:
power, speed, cadence, heart rate, grade, and where you are on the lap.
It is not the game picture.

## Open it on a TV

1. Start a ride, or start the demo below. The game prints one or more
   `http://` addresses.
2. Put the TV on the same Wi-Fi as the computer running the game.
3. Open the TV browser and type one of those addresses. The default port
   is 7878, so a typical address looks like `http://192.168.1.20:7878/`.

The numbers update a few times a second. If the stream drops, the page
says `reconnecting` and tries again on its own. A missing sensor is an
em dash, not a zero.

## What the page shows

Large tiles for power (W), speed (km/h), cadence (rpm), and heart rate
(bpm). A second row shows grade (%), distance (km), lap, elapsed time
(h:mm:ss), and elevation (m). Grade color tracks how steep the road is.

Under the numbers is the lap elevation profile. A marker sits at your
position on the lap and moves as you ride.

## Try it without the game

Build the demo in the container (SteamOS has no compiler on the host),
then run the binary on the host. The container network cannot see the TV.
From the repo root:

```bash
podman run --rm \
  -v "$PWD:/src" \
  -v penguin-pass-cargo:/usr/local/cargo/registry \
  -w /src \
  -e CARGO_HOME=/usr/local/cargo \
  -e CARGO_TERM_COLOR=always \
  penguin-pass-toolbox \
  cargo build -p pp-cast --example demo
target/debug/examples/demo
```

The demo rides a synthetic 5 km hilly loop and prints the addresses.
Ctrl-C stops it.

To open the page on this computer instead of a TV, bind loopback:

```bash
target/debug/examples/demo 127.0.0.1:7878
```

Then open `http://127.0.0.1:7878/`.

## Limits

The browser shows ride numbers and the lap profile. It does not show the
game view. Casting that video is a later milestone.

There is no password. Anyone on the same LAN who has the address can open
the page. Do not forward the port onto the internet.
