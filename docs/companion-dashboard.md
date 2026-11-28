# Companion dashboard

Penguin Pass serves a live ride page on the home network. A phone or
tablet on the same Wi-Fi opens it in a browser. The page is ride telemetry:
power, speed, cadence, heart rate, grade, and where you are on the lap.
It is not the game picture.

## Open it on a phone or tablet

1. Start a ride, or start the demo below. The game prints one or more
   `http://` addresses.
2. Put the phone or tablet on the same Wi-Fi as the computer running the
   game.
3. Open a browser on that device and type one of those addresses. The
   default port is 7878, so a typical address looks like
   `http://192.168.1.20:7878/`.

The numbers update about four times a second. If the browser loses the
stream, the page says `Reconnecting`, dims the numbers, and tries again
on its own. That is separate from the trainer losing its link: the game
can report the trainer as reconnecting while the page itself stays live.
A missing sensor is an em dash, not a zero.

## What the page shows

The layout matches the game UI. Power is the large number on the left,
coloured by zone once the game has sent an FTP. Speed, grade, cadence,
and heart rate sit beside it. Time, distance, elevation, and lap are
across the top on a wide screen and along the bottom on a phone. Under
the numbers is the lap profile, ridden in grey and ahead coloured by
gradient, plus the next kilometre.

The page serves its own copy of Barlow Semi Condensed. Nothing is loaded
from the internet. The font licence is at `/fonts/OFL.txt`.

## Ride status

The game can publish a status alongside each telemetry snapshot. Until
the first one arrives, zones stay off and the trainer chip stays hidden.

`CastServer::publish_status` takes a `RideStatus`:

| Field | JSON | Meaning |
| --- | --- | --- |
| `ftp_w` | number or null | FTP in watts. Null means no zone colours. |
| `source` | `unknown`, `simulated`, or `trainer` | Where the watts come from. |
| `trainer_name` | string or null | Name when the source is a trainer. |
| `link` | `unknown`, `connected`, `reconnecting`, or `searching` | The trainer link, not the browser stream. |
| `resistance` | `controlled`, `reading_only`, or null | Whether the trainer is being told a grade. |
| `power_3s_w` | number or null | Three-second average power. The hero number uses this when it is present. |

`GET /api/status` returns that object. The event stream sends it as an
`event: status` message. `GET /api/telemetry` is unchanged. `GET /api/zones`
is the power-zone and gradient table the page colours from.

## Try it without the game

Build the demo in the container (SteamOS has no compiler on the host),
then run the binary on the host. The container network cannot see other
devices on the LAN. From the repo root:

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

To open the page on this computer instead of a phone or tablet on the
LAN, bind loopback:

```bash
target/debug/examples/demo 127.0.0.1:7878
```

Then open `http://127.0.0.1:7878/`.

To hold the sample numbers from the design mockup (345 W, +10.0 % grade)
instead of a moving ride:

```bash
target/debug/examples/demo --sample 127.0.0.1:7878
```

Open `http://127.0.0.1:7878/?preview=reconnecting` to see the dimmed
page the browser shows when the stream drops.

## Limits

The browser shows ride numbers and the lap profile. It does not show the
game view. Casting that video is a later milestone.

There is no password. Anyone on the same LAN who has the address can open
the page. Do not forward the port onto the internet.