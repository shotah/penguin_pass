# Multiplayer

This is a design. None of it is built. Penguin Pass is single-player
today: one `Session`, one `Ride`, one rider, and computer riders from
`pp-bots`. This note says how other people get onto the same road
without Penguin Pass running any server.

Facts about Steam, crates, and other services were read from their own
pages on 27 November 2026. The links are at the end. Anything that was
not checked on a page or in this tree is listed under
[Not verified](#not-verified).

## The short version

- **Rides are rooms, not a world.** Two to sixteen riders share one
  course for one ride. An always-on world with thousands of riders
  (the Zwift model) needs servers. This does not.
- **Steam finds the riders and connects them.** Steam lobbies do
  invites, friend joins, and a public ride list. Steam Datagram Relay
  carries the traffic between players, through home routers, without
  showing anyone's IP address. Both come with the Steamworks account
  the store release already needs. There is nothing to host.
- **Every game simulates only its own rider.** Power comes from that
  rider's trainer, so nobody else can simulate it anyway. Each game
  sends its position ten times a second. The lobby owner's game is the
  host: it passes those positions on and owns the few shared things
  (course, start time, bots, results).
- **The network never stops a ride.** If a connection drops, the other
  riders fade out and the workout, the trainer, and the FIT recording
  carry on.
- **Builds without Steam connect directly.** The Flatpak cannot reach
  Steam. It gets "host on this network" and "join an address". That
  works on a home LAN and over any VPN, which is where Tailscale fits.
  Tailscale is a way to make an address reachable. It is not a way to
  find a game.
- **Mostly imported.** `steamworks` for lobbies, `renet` with
  `renet_steam` and `renet_netcode` for the connection, `postcard` for
  the bytes, `mdns-sd` for LAN discovery.

## Why this game is cheap to network

Most multiplayer work goes into making several machines agree on one
simulation. This game hardly has a shared simulation.

| What the code does today | Why it helps |
| --- | --- |
| A rider is a distance along the course, a speed, and a lane fraction. `Ride::step` in `crates/pp-course/src/ride.rs` turns that rider's watts into speed. | A remote rider is about 20 bytes. Nothing has to be re-simulated on another machine. |
| Riders do not collide. Bots keep apart with lane rules, and the player is never pushed. | No shared physics, so no rollback and no lockstep. |
| The only way one rider changes another is the draft. `pp_bots::draft(gap_m, slot_offset)` is a pure, public function. | Each game works out its own draft from where it sees the others. |
| `pp-session` does not link Godot. `Session::tick` is the whole ride loop. | Two sessions can be run in one test process and wired together. |
| `PlayView.bots` is a `Vec<BotState>`. `BotField::sync` in `crates/pp-godot/src/bots.rs` draws whatever is in it, keyed by id, with a name tag. | `BotState` already has every field a remote human needs: id, name, preset, kit, distance, speed, power, cadence, slot. The front end can draw humans before it is touched. |
| A course is a few kilobytes of JSON. Penguin Pass is 5.7 kB. The published 10 km routes are 3.7 kB and 5.2 kB. | The host can send the road itself instead of hoping every machine generates the same one. |
| The ride does not pause while the menu is open (`docs/game.md`). | There is no pause to synchronise. |
| In `Ready`, `tick` still runs the trainer and the dashboard while the bike is held at the start line. | That phase is already a lobby: riders can spin on the line while others arrive. |

What the code assumes that has to change:

- `Peloton::tick` takes one `PlayerInput`. Company bots follow that
  player's power, and bots draft and avoid only that player.
- `Ride::new` starts at distance zero. There is no way to place a rider
  part-way round the lap.
- `Settings` has no display name. `rider` is the on-screen preset
  (`nils`, `sigrid`).
- Fish, powerups, segments, and the ghost are one rider's state inside
  one session. That is fine. They stay per-rider (see
  [Shared and per-rider](#shared-and-per-rider)).

## What "no servers" allows

| Need | Who provides it | Cost to us |
| --- | --- | --- |
| Find a ride, invite a friend, join a friend | Steam lobbies (`ISteamMatchmaking`). Lobbies live on Steam's servers. Up to 250 members. Metadata, search filters, automatic owner hand-over. | None beyond the $100 app fee already in `docs/steam-release.md` |
| Get packets between two homes | Steam Datagram Relay through `ISteamNetworkingSockets`. Valve's page: "IP addresses are never revealed" and "All traffic you receive is authenticated, encrypted, and rate-limited." | None |
| Decide shared state | The lobby owner's game | A player's uplink, well under 1 Mbit/s at the cap |
| Player names and friends | Steam personas and the friends list | None |
| Times to beat while nobody is online | Steam leaderboards (see [Asynchronous play](#asynchronous-play)) | None |

Valve's pages do not print a price for lobbies or relay traffic.
Steamworks describes itself as "a free suite of tools available to any
developer", and the relay page says that for peer-to-peer on Steam "all
you need to do" is call the P2P connect functions. No usage fee is
documented.

What it does not allow:

- A persistent world that exists when no player is hosting.
- Results nobody can fake. The host is a player's machine.
- Rides of hundreds. The limits are the host's uplink and how many
  riders the game can draw, not Steam.

## Options considered

| Option | Verdict | Why |
| --- | --- | --- |
| **Steam lobbies + Steam Datagram Relay** | **Chosen for Steam builds** | Discovery, invites, NAT traversal, and relay in one free service. Invites need no typing, which matters on a Deck. |
| **Direct connect over LAN or VPN** | **Chosen for non-Steam builds, and for development** | No third party. `renet_netcode` is plain UDP. Covers the same house, Tailscale, ZeroTier, or a forwarded port. |
| iroh 1.5.0 (Rust, QUIC, dial by public key) | Later, optional | The best way to join with a code and no VPN. But n0's free public relays are "suitable for development and testing"; production is the $19 a month plan or a relay we host. |
| Tailscale as the transport | Not built in | See below. |
| Epic Online Services | Rejected for now | Free lobbies and relays, and it accepts Steam identities. But it is a C SDK, the Rust crates (`eos-sys` 0.1.1) are young, and it needs an Epic developer portal setup. Revisit only for a store other than Steam. |
| GodotSteam and Godot's `MultiplayerAPI` | Rejected | The game logic is Rust and `pp-session` is tested without Godot. Netcode in the scene tree would move logic out of the tested crates, and Rust could only reach GodotSteam through `Object.call` with no types. |
| `matchbox`, `libp2p` | Rejected | Both need a signalling or relay server from someone. |
| Steam Remote Play Together | Does not apply | It streams one copy of a local-multiplayer game to friends. Every rider here needs their own trainer on their own machine. |
| The old `ISteamNetworking` P2P API | Rejected | Valve marks it deprecated and may remove it. |

### Tailscale

Tailscale gives each machine an address the others can reach. That is
all the game needs from it, and it needs nothing from the game. A
"join an address" screen works over a tailnet exactly as it does over a
LAN.

It is not a good default:

- Every rider installs Tailscale and signs in. On SteamOS that is an
  installer script, not a store app, because the system image is
  read-only.
- Riders must be in one tailnet, or share machines with each other. The
  free Personal plan is up to 6 users.
- Tailscale does not carry multicast or broadcast, so LAN discovery
  does not cross it. Someone reads out an address and the others type
  it.
- Embedding it is not practical. `tsnet` is Go, `libtailscale` wraps
  that in C, and the Rust library is an experimental preview. Each
  still needs the player to have an account.

So: support it for free through direct connect, write one paragraph in
`docs/play.md` when that ships, and do not build on it.

## Architecture

```text
            Steam lobby (Steam back end): members, owner, metadata
                 |                 |                  |
              Rider A           Rider B            Rider C
              (owner = host)    (client)           (client)
                 ^  ^              |                  |
                 |  +--------------+   renet over     |
                 +--------------------- Steam relay --+

Inside each game:

Trainer -> pp-session::Session::tick -> own Ride (physics, as today)
                 |        ^
        own state|        |remote riders, shared clock, rules
                 v        |
              pp-net::Room  (protocol, roster, clock, smoothing)
                 |
        renet channels (reliable, unreliable)
                 |
   +-------------+--------------+----------------+
   | renet_steam | renet_netcode| in-memory pipe |
   | Steam builds| LAN / VPN    | tests          |
   +-------------+--------------+----------------+
```

### Topology: a star through the lobby owner

Every client holds one connection, to the host. The host forwards rider
states and sends the shared messages.

A full mesh (every game connected to every other) was the alternative.
With a mesh, a host leaving breaks nothing. But a mesh needs
N×(N−1)/2 connections to succeed, a rider can end up visible to some
and not others, and the direct-connect transport would need its own
mesh code. With a star there is one code path for Steam and for direct
connect, joining is one connection, and everyone sees the same roster.

The usual cost of a star is that the host leaving ends the game. Here
it does not, because nobody depends on the host to keep riding. See
[Host hand-over](#host-hand-over).

Latency through the host does not matter much for this game. States
carry a timestamp and are projected forward along the road (see
[Showing other riders](#showing-other-riders)).

### Who decides what

| Thing | Decided by | Notes |
| --- | --- | --- |
| A rider's distance, speed, lane, power | That rider's game | Sent as-is. The host only range-checks it. |
| A rider's draft saving | That rider's game | From where it sees everyone else. |
| Course, direction, mode, laps, rules | Host | Changed only before the start. |
| Ride clock | Host | Clients estimate the offset. |
| Bots | Host | Simulated once, sent like riders. |
| Start countdown | Host | Sent as a clock time, not as "now". |
| A rider's finish time | That rider's game | Measured on its own clock from its own start. |
| Results table | Host | Collected and re-sent to all. |
| Fish, powerups, segments, records, workout, FIT file | Each rider's game | Never sent, except a few flag bits for looks. |

### Crates

| Crate | Change |
| --- | --- |
| `pp-net` (new) | No Godot, no Steam. Message types and `postcard` encoding. `Room`: the host and client state machines over `renet`. Roster, clock sync, remote-rider smoothing, results. The `Lobby` trait below. An in-memory pipe for tests. |
| `pp-steam` (new) | The only crate that links `libsteam_api`. Wraps `steamworks`: init, callback pump, lobby create/join/list, invites, friends, rich presence. Implements `Lobby` and builds the `renet_steam` transports. Kept thin, because it cannot run in CI. |
| `pp-session` | Owns an optional `Room`. Each tick: poll the room, send own state, merge remote riders into `PlayView.bots`, fold their draft into the draft factor. New calls for the menus and new `RideEvent` variants. |
| `pp-bots` | A pace-only field. Later: tick against several humans, and restore from a snapshot. |
| `pp-course` | Later: start a `Ride` at a distance other than zero. |
| `pp-view` | "Ride together" rows, lobby and rider-list screens, the nearby-riders list, launch flags. |
| `pp-godot` | Builds the Steam backend when the `steam` feature is on and hands it to the session, the same way it hands over the trainer and heart-rate factories. A different tag for humans. |

`pp-session` depends on `pp-net` only. It never names Steam. That keeps
the Flatpak build free of the Steam library and keeps the ride logic
testable.

```text
trait Lobby {
    fn poll(&mut self) -> Vec<LobbyEvent>;   // joined, left, owner changed, invite accepted
    fn host(&mut self, rules: &Rules, visibility: Visibility);
    fn join(&mut self, target: JoinTarget);  // lobby id, friend, or address
    fn leave(&mut self);
    fn list(&mut self, filter: &RideFilter); // public rides; result arrives as an event
    fn invite(&mut self, friend: PeerId);
    fn friends(&self) -> Vec<Friend>;        // for an in-game invite list
    fn publish(&mut self, summary: &RideSummary); // metadata others can see
    fn transport(&mut self, role: Role) -> Transport; // renet server or client
}
```

Steam implements all of it. Direct connect implements `host`, `join`
by address, `list` through mDNS, and leaves `invite` and `friends`
empty.

## Steam lobbies and joining

### The lobby

One Steam lobby is one ride. The member limit is 16 (Steam allows 250).

| Visibility in the game | Steam lobby type | Who can join |
| --- | --- | --- |
| Invite only | Private | Invited riders |
| Friends (the default) | FriendsOnly | Friends and invited riders. Not in the public list. |
| Open | Public | Anyone. Returned by search. |

The owner writes lobby metadata. Steam allows keys up to 255 characters
and values up to 8192 bytes. Only the owner can set it.

| Key | Example | Used for |
| --- | --- | --- |
| `pp` | `1` | Marks a Penguin Pass lobby. While testing on app 480 this filters out every other developer's lobbies. |
| `proto` | `1` | Protocol version. Search filters on it. |
| `build` | `0.2.0` | Shown when versions differ. |
| `mode` | `ride` or `race` | Ride list |
| `state` | `gathering`, `riding`, `finished` | Ride list. A started race is also set not joinable. |
| `course` | `Desert 10 km` | Ride list |
| `km`, `ascent` | `10`, `110` | Ride list and sort |
| `fp` | `ccb3998fbd118f27` | Course fingerprint |
| `pace` | `2.5` | The host's stated pace in W/kg, so a searcher can sort by nearness |

The course itself is not put in metadata. It goes over the connection
(see [The course](#the-course)).

### Ways in

1. **Host a ride.** Create the lobby, write the metadata, start the
   `renet_steam` server with access limited to lobby members
   (`AccessPermission::InLobby`), and wait on the start line.
2. **Invite.** Two doors. The Steam overlay invite dialog
   (`ActivateGameOverlayInviteDialog`), and an in-game friends list
   that calls `InviteUserToLobby`. The in-game list is needed because
   the overlay is not there when the game was not started by Steam.
3. **Accept an invite or press Join Game on a friend.** Steam posts
   `GameLobbyJoinRequested_t`. The game joins that lobby.
4. **The same, with the game closed.** Steam starts the game with
   `+connect_lobby <64-bit lobby id>`. `parse_launch_in` in
   `crates/pp-view/src/launch.rs` learns that argument and the game
   joins after start-up.
5. **Find a ride.** `RequestLobbyList` with a string filter on `pp` and
   `proto`, a slots-available filter, and near-value sorts on `km` and
   `pace`. Steam returns at most 50, from the same or nearby regions by
   default.
6. **Ride now.** Take the best open lobby from that list. If there is
   none, host an open one. This is the closest thing to a populated
   world that needs no server.

After any join: read the lobby owner, connect to the owner, send
`Hello`, receive `Welcome`, load the course, appear on the road.

Rich presence is set while in a ride: `status` ("Desert 10 km, 4
riders"), and `steam_player_group` with `steam_player_group_size` so
the friends list groups the riders. `steam_display` needs a
localisation file uploaded in Steamworks.

## Protocol

`renet` gives channels over whichever transport is underneath. Three
are used.

| Channel | Kind | Carries |
| --- | --- | --- |
| 0 | Reliable, ordered | Hello, Welcome, Refuse, Roster, Rules, Countdown, Finish, Results, Bye |
| 1 | Unreliable | Rider state, snapshots |
| 2 | Unreliable | Ping, Pong |

Messages are Rust enums encoded with `postcard`, which has a documented
stable wire format. The first byte of `Hello` and `Welcome` is the
protocol version. A host refuses a different version with a sentence
the menu can show. Lobby search already filters on it, so this is the
second line of defence.

| Message | Direction | Content |
| --- | --- | --- |
| `Hello` | client → host | Protocol, build, name, rider preset, kit, whether power is from a real trainer |
| `Welcome` | host → client | The client's rider id, rules, course, host clock, ride state, roster, results so far |
| `Refuse` | host → client | Reason: version, full, race already started, removed by host |
| `Roster` | host → all | Rider id, peer id, name, preset, kit, trainer flag, for everyone. Sent on any join or leave. |
| `Rules` | host → all | Mode, laps, bots, fish and powerups allowed, trainers only. Before the start only. |
| `Countdown` | host → all | Host clock time of the start |
| `State` | client → host | Own rider state. 10 Hz. |
| `Snapshot` | host → all | Host clock, then every rider and bot. 10 Hz. |
| `Ping`, `Pong` | both | Clock offset and latency |
| `Finish` | client → host | Elapsed time and distance |
| `Results` | host → all | Table so far |
| `Bye` | either | Leaving on purpose |

### Rider state

```text
seq          u16   wraps; older than the last one seen is dropped
clock_ms     u32   host ride clock when sampled
distance_cm  u32   odometer from the start line
speed_cm_s   u16
power_w      u16
cadence_rpm  u8
heart_bpm    u8    0 = not shared
lane_q       u8    lane fraction x 16 (0.0 to 8.0)
flags        u8    standing, coasting, powerup active, finished, trainer link down
```

Eighteen bytes. A snapshot adds one byte of rider id to each.

### Rates and traffic

Ten states a second is enough because bicycles change speed slowly.
Payload only, before UDP and relay framing:

| Ride | Client sends | Host sends, per client | Host sends, total |
| --- | --- | --- | --- |
| 4 riders, 4 bots | 0.2 kB/s | 1.6 kB/s | 4.7 kB/s |
| 16 riders, 8 bots | 0.2 kB/s | 4.6 kB/s | 69 kB/s |

With framing, a full ride costs the host roughly 0.6 to 0.7 Mbit/s up.
A four-rider ride is about 50 to 60 kbit/s. A full snapshot is about
460 bytes, inside one packet.

Riders plus bots never exceed 24, which is the number `pp-bots` already
caps at and the front end already draws. Bots give up places to humans.

## The course

The route generator promises the same bytes for the same spec, and
`docs/route-generator.md` pins fingerprints. The Windows and macOS
workflows run those fingerprint tests (`cargo test --workspace`), and
`docs/cross-platform.md` records the macOS tests as passing in CI. So
the same road on every platform is likely. The design still does not
rely on it. The generator calls `sin` and `cos` from each platform's
maths library, two riders may be on different game versions, and a
custom route exists on one machine only. Sending the road costs a few
kilobytes and covers all three.

`Welcome` carries the `CourseChoice`, the direction, the route JSON,
and its FNV-1a fingerprint (the same hash the generator tests use).

1. The client builds the course from the `CourseChoice` the normal way
   and fingerprints it.
2. If the fingerprint matches, it uses its own. Records, ghosts, and
   badges use the normal keys, so a personal best set in a group ride
   is a personal best.
3. If it does not match, or the route is a custom file the client does
   not have, it parses the host's JSON with `Course::from_str`. Records
   for that ride are keyed by the fingerprint.
4. If neither works, the join fails with a sentence. The rider stays on
   the title screen.

A mismatch in step 2 is also worth a log line. It is the evidence
whether generation really is the same across platforms.

The world, scatter, and fish all derive from the course and its seed,
so once the course matches, every rider sees the same road, trees, and
fish layout. Time of day and weather go in `Rules` so the ride looks
the same for everyone. `auto` is resolved by the host.

## Showing other riders

### One odometer

`BotState.total_distance_m` is "on the player's odometer". Riders who
joined at different times have different lap counts, so a remote
distance is first moved onto the local rider's scale: add or subtract
whole laps until it is within half a lap of the local rider.

```text
d = remote_distance + k * lap_length,  k chosen so |d - own_distance| is smallest
```

Drawing and draft use `d`. Race standings use the raw distance.

### Projecting forward

Remote riders are drawn where they are now, not where they were when
the packet left. For each remote rider:

```text
d(now) = d_sample + speed_sample * (clock_now - clock_sample)
```

A bicycle accelerates at about 1 m/s² at the very most, so the error is
small: 1 cm after 150 ms, 12 cm after 500 ms. When a new sample
disagrees with the projection, the difference is blended in over a
quarter of a second, so nobody snaps. Lane and cadence ease toward the
latest value.

The usual alternative draws everyone a fixed 100 ms in the past. That
is smoother for games with sudden movement. Here it would put a rider
at 40 km/h more than a metre behind the truth. That is enough to change
who is on whose wheel and who wins a sprint.

| No sample for | What the rider's game shows |
| --- | --- |
| Up to 1 s | Keeps projecting |
| 1 s to 10 s | Holds the last speed, greys the name tag |
| 10 s | Removes the rider and shows "name lost connection" |

### Into the existing view

The session turns each remote rider into a `BotState` with an id from a
reserved range and appends it to `PlayView.bots`. `BotField::sync`
draws it with the right preset, kit, and name. The first milestone
needs no drawing code.

After that, humans get their own tag. `bot_tag` prints a pace rider's
W/kg, and a human's tag should show the name and a marker for keyboard
power.

### Draft

`Group::draft_factor` today asks the peloton. It becomes the smaller of
that and the best shelter from any remote rider:

```text
factor = min(peloton.draft_factor(player),
             min over remote riders of draft(d - own_distance, |slot - own_slot|))
```

`draft` is already public in `pp-bots`. Each game computes only its own
factor. Nobody is told what their draft is.

### No collisions

Riders pass through each other, as bots and the player can today and as
other trainer games do. Sixteen riders on a start line overlap until
start rows exist (see [Later](#later)).

## Clock, modes, and results

### Clock

The ride clock is the host's. A client sends `Ping` with its own time,
the host answers with that and its clock, and the client keeps the
answer with the shortest round trip out of the last several. That is
good to about 10 to 20 ms on a home connection, or about 25 cm of road
at 45 km/h. It is refreshed every few seconds.

### Group ride

The default. No winner and no end.

- The lobby starts in `gathering`. Riders are held on the line in the
  session's `Ready` phase, pedalling, visible to each other.
- The host presses Start. `Countdown` names a clock time ten seconds
  out. Every game calls `begin_ride` at that time.
- A rider who joins later starts from the line when they arrive. On a
  lap course they meet the others within a lap.
- Anyone can leave. The ride continues.

### Race

- Joining closes at the start (`SetLobbyJoinable(false)`). A late
  arrival is refused with "race already started".
- The distance is a number of laps from `Rules`.
- Each game measures its own elapsed time from its own `begin_ride` to
  its own finish and sends `Finish`. Network delay cannot change a
  time. The clock error only shifts when "go" appears, by a few
  hundredths of a second.
- The host sends `Results` after each finish. Riders who drop out are
  listed without a time.
- Fish and powerups are off unless the host turns them on, because
  powerups change drag and mass.

### Results

The summary screen gets a table: place, name, time, average power. The
existing summary, FIT file, and Strava upload are unchanged and stay
per rider.

## Shared and per-rider

| System | In a shared ride |
| --- | --- |
| Course, direction, time of day, weather | Shared. From the host. |
| Bots | Shared. The host simulates them. Pace riders only, because company bots follow one player's power and would cluster round the host. Bots react to the host and ignore other humans until `pp-bots` learns about several. |
| Fish | Same layout for everyone, since it is seeded by the course. Each rider collects their own copy. Nobody takes a fish from anyone. |
| Powerups | Change only the user's own physics, as today. A flag bit lets others see one is active. Host rule in a race. |
| Segments, records, ghost | Per rider, as today. |
| Workouts and ERG | Per rider. Two riders can do different workouts on the same road. |
| Rider weight, FTP, trainer difficulty | Per rider. Never sent. |
| FIT recording, Strava | Per rider. Unchanged. |
| TV dashboard | Per rider. Unchanged at first. |
| Restart, change course | Host only, and only while gathering. For a client, "End ride" leaves the group. |
| Pause | There is none, as today. |

## When things go wrong

The rule: the ride outlives the network.

| Event | Behaviour |
| --- | --- |
| A client loses the host | Its own ride continues. Other riders grey, then go. It retries the lobby owner for 30 s, then shows "Riding solo" and keeps going. |
| A client drops | Others see the tag grey, then the rider removed. If the same peer returns, it gets its rider id back. |
| Steam is not running or is offline | "Ride together" says so. Solo play is untouched. |
| Versions differ | Refused with both versions in the message. |
| Course cannot be built | Join fails with a sentence. |
| A rider's trainer drops | Their power is 0 W, as in solo. Others see them coast. A flag bit marks it. |
| A Deck goes to sleep | Same as a dropped client. On wake it rejoins the lobby if it still exists. |
| Packet loss | States are unreliable and replaced by the next one. Shared messages are on the reliable channel. |

### Host hand-over

Steam picks a new lobby owner when the owner leaves, and guarantees
there is always exactly one.

1. Every game sees the owner change.
2. The new owner starts a server. It already has the course, the rules,
   the roster, the results so far, and a clock estimate, because the
   host sends all of those to everyone. Its clock becomes the clock.
3. The others connect to the new owner and send `Hello` with their old
   rider id.
4. For those few seconds each rider keeps riding and sees the others
   projected forward, then held.

Bots restart from a fresh field at hand-over, because a `Peloton`
cannot yet be rebuilt from a snapshot. They are faded out and in so
they do not jump.

A direct-connect ride has no lobby service to name a new host. If the
host leaves, everyone carries on solo.

## Trust, privacy, and safety

**Cheating.** Every game reports its own position, so a modified game
can report anything. The same is true of weight and trainer calibration
in every trainer game. This design does not try to prevent it. It does
three cheap things: the host drops states that are outside the physics
(the game caps descent speed and simulated power already), keyboard
power is flagged and shown on the name tag, and a race can be set to
real trainers only. Rides default to friends.

**What is sent.** Name, rider preset, kit, distance, speed, lane,
power, cadence. Weight and FTP are not sent. Heart rate is sent only if
the rider turns on a new setting, off by default.

**Strangers.** Open rides are opt-in by the host. There is no text or
voice chat in this design, so there is nothing to moderate. Names are
Steam personas. The host can remove a rider, which closes that
connection and refuses that peer until the ride ends.

**IP addresses.** On Steam, the relay hides them. On direct connect,
the host's address is what the others type, so it is for people who
already trust each other.

## Builds without Steam

Valve's page says a running Steam client is required, in the same user
session, and the account must own the app. The Flatpak sandbox grants
no access to the Steam client, and a GitHub download may be on a
machine with no Steam at all. Those builds are compiled without the
`steam` feature and get direct connect.

| Piece | How |
| --- | --- |
| Transport | `renet_netcode`, UDP, one port (default 7979) |
| Host | "Host on this network" shows the machine's addresses |
| Find on a LAN | The host advertises `_penguinpass._udp` with `mdns-sd`. Others see a list. No typing. |
| Join elsewhere | "Join an address". A VPN address, or a forwarded port. |
| Names | A new `display_name` setting |
| Sandbox | The Flatpak already has `--share=network` for the TV dashboard |

The Steam build includes direct connect too. That is what lets a Steam
rider and a Flatpak rider share a ride, and it is how two copies are
tested on one machine.

Direct connect is not encrypted by the game. A LAN is trusted and a VPN
encrypts for itself.

**Join codes without a VPN** would be a third transport on iroh: the
host shows a code, the other rider enters it, and iroh punches through
or relays. It is a real option, in Rust, with LAN discovery built in.
It waits because of the relay question in
[Options considered](#options-considered), and because a code has to be
typed. Build it if non-Steam riders ask for it.

## Build, packaging, and licences

| Item | Plan |
| --- | --- |
| Feature flag | `steam` on `pp-godot`, off by default. Steam depots build with it. The Flatpak and plain builds do not. |
| Steam library | `steamworks-sys` ships pre-generated bindings and the redistributable library, so the toolbox image needs no libclang. `libsteam_api.so`, `steam_api64.dll`, or `libsteam_api.dylib` must sit beside the game library in each depot. |
| Export | `tools/export-player.sh` copies only `pp_godot` today. Add the Steam library per platform, list it in a `[dependencies]` section of `godot/penguin_pass.gdextension`, and give the Linux library an `$ORIGIN` rpath so the loader finds it. |
| App id | Until Penguin Pass has one, develop against Valve's test app 480 with a `steam_appid.txt` beside the binary. That file must not be in an uploaded depot. |
| Start-up | Initialise Steam if it is there. If it is not, carry on. Do not call `SteamAPI_RestartAppIfNecessary`: the game is MIT and should start from a folder. |
| Callbacks | `Client::run_callbacks()` once a frame, from the room poll inside `Session::tick`. |
| Versions | `renet_steam` 4.0.0 requires `steamworks ^0.13.1`. Only one copy of the Steam library can be linked, so the two are pinned together and bumped together. |
| Licence, code | `steamworks`, `renet`, `renet_steam`, `renet_netcode`, `postcard`, `mdns-sd` are MIT or Apache-2.0. Run `./x licenses` after adding them. |
| Licence, SDK | The Steamworks SDK files inside `steamworks-sys` are Valve's, not MIT. Valve's agreement allows shipping the redistributable binaries with the game. Nothing from the SDK is committed to this repo. Valve's open-source page says permissive licences such as MIT are fine and copyleft is the problem. |
| CI | The workspace glob builds `pp-steam` everywhere, so clippy and typecheck cover it. Nothing that calls Steam can run in CI. |

## Menus and HUD

Everything is reachable with a controller. On Steam no screen needs
typing, which keeps the Deck review items in `docs/steam-release.md`
unchanged.

| Place | Addition |
| --- | --- |
| Title | A "Ride together" row. It opens: Host a ride, Friends riding (join), Find a ride, Ride now, and on non-Steam builds Join an address. |
| Lobby (the start line) | Rider list, course, mode, visibility, Invite, Start (host only) |
| Pause | "Riders" (names, gap, connection), "Invite", "Leave group" |
| HUD | A riders chip beside the trainer chip. A short list of the nearest riders ahead and behind with the gap. Name tags as bots have now. The draft chip is unchanged. |
| Toasts | Joined, left, lost connection, new host, countdown, finished |
| Summary | The results table |

"Join an address" is the one text field. On a Steam build it uses
`ShowFloatingGamepadTextInput`. An address is digits and dots, so a
D-pad digit entry avoids a keyboard on other builds.

## Testing

| Layer | How | Needs |
| --- | --- | --- |
| Message encoding | Round-trip property tests with `proptest`, already a dev-dependency | Nothing |
| `Room` | Host and clients in one process over the in-memory pipe, with scripted delay, loss, and reordering and a hand-stepped clock | Nothing |
| Session | Two or three `Session`s with `SimTrainer` joined by the pipe. Assert each sees the others within a set distance, that draft applies, that a host hand-over loses nobody. | Nothing |
| One game, fake company | A `--net-fake-riders <n>` flag runs extra sessions in-process and feeds them in as remote riders, the same idea as `--fake-trainer`. Screenshots and the smoke test can use it. | Nothing |
| Two games, one machine | `./x play -- --net-host` and `./x play -- --net-join 127.0.0.1` over `renet_netcode` | Nothing |
| Steam | Two machines, two Steam accounts, app 480. Invite, friend join, cold-start join, list, hand-over. | A second account and machine |
| Deck | Game Mode: overlay invite, and Bluetooth trainer plus Wi-Fi together for a full ride | A trainer |

Most of the logic is in the first four rows and runs in `./x test`.

## Order of work

Each step ends in something that can be ridden or tested.

1. **`pp-net` and the session seam.** Messages, `Room`, the pipe,
   remote riders in `PlayView.bots`, draft from remote riders, the
   fake-riders flag. Done when two sessions in a test see and draft
   each other.
2. **Direct connect.** `renet_netcode`, host and join flags, then the
   menu rows. Done when two copies on one machine ride together. This
   comes before Steam because it needs no account, no second machine,
   and proves the protocol.
3. **Steam.** `pp-steam`, the feature flag, packaging the library,
   lobbies, invites, friend join, cold-start join. Done when a Deck and
   a desktop on two accounts ride together through app 480.
4. **Rides people can find.** The ride list, Ride now, rich presence,
   host hand-over, the rider list and nearby-riders HUD.
5. **Race.** Countdown, laps, finish, results table, race rules.
6. **LAN discovery and the non-Steam menu.** mDNS, Join an address,
   `display_name`, a Tailscale paragraph in `docs/play.md`.

### Later

- **Join beside a friend.** Needs `Ride` to start at a distance. Also
  gives start rows for a race.
- **Keep together.** A group-ride rule that scales each rider's speed
  toward the group's so different fitness levels stay on the same
  stretch of road. This is the feature that makes a social ride work.
- **Bots that see every human**, and bots that survive a hand-over.
- **More than sixteen.** Send far-away riders less often, draw them
  simpler. The star holds for a few dozen. Beyond that it needs a
  different design.
- **Group workouts** with a shared plan.
- **The TV dashboard** showing the group.
- **iroh join codes.**

## Asynchronous play

Racing people who are not online is cheaper than everything above and
uses only the Steam init from step 3.

- A Steam leaderboard per segment and per course lap. Steam allows
  10,000 leaderboards per game, one entry per player on each, with up
  to 64 extra integers.
- The friends filter (`k_ELeaderboardDataRequestFriends`) gives "your
  friends' times on this climb" with no server.
- A ghost can be attached to a score with `AttachLeaderboardUGC`, so a
  friend's best lap could be downloaded and ridden against. The game
  already records and replays a ghost (`docs/segments.md`).

Scores are written by the client and can be faked. Showing friends
only, by default, keeps that from mattering.

## Costs

| Item | Cost |
| --- | --- |
| Steam lobbies, relay, leaderboards | No fee documented. Part of Steamworks. |
| Steam app fee | $100, already planned |
| Direct connect, LAN discovery | None |
| Tailscale, for players who choose it | Free Personal plan, up to 6 users |
| iroh, if built | Public relays free for development. $19 a month for production relays, or a relay we host. |
| Servers | None |

## Decisions for the owner

1. **Rider cap.** 16 is proposed. Lower is easier to test.
2. **Open rides at launch**, or friends and invites only at first.
3. **Does the Flatpak get multiplayer in the first release**, or does
   step 6 wait.
4. **Race rules.** Whether keyboard power may enter a race, and whether
   powerups are ever allowed.
5. **Bots in shared rides.** Pace riders only, as proposed, or none
   until bots can see every human.
6. **When to apply for the app id.** App 480 is enough to develop on.
   Rich presence text, leaderboards, and a real friends-list entry need
   the real id.

## Not verified

- Nothing here has been built or run.
- `steamworks` 0.13.1 building and linking inside the Steam Runtime 4
  toolbox image, and on Windows and macOS.
- Godot loading a GDExtension that itself needs `libsteam_api`: the
  `[dependencies]` section of the `.gdextension` file, the `$ORIGIN`
  rpath, and the macOS bundle path are from memory of Godot's
  documentation and were not tried.
- The Steam overlay over this game's Vulkan renderer, in desktop mode
  and in Game Mode.
- Whether `+connect_lobby <id>` reaches the game untouched. Godot may
  treat an argument without `--` as a scene path. If so, turn on the
  Steamworks setting that delivers the launch line through
  `ISteamApps::GetLaunchCommandLine` and read it there.
- That two game copies under one Steam account cannot test a lobby
  together. Valve's pages do not say so. It is assumed, which is why
  step 2 exists.
- The exact `renet_netcode` 2.0.1 calls for a connection without
  tokens. The crate versions and dependencies were checked. The API
  was not read.
- `AttachLeaderboardUGC` was not found in the `steamworks` 0.13.1
  documentation. Ghost sharing may need a raw call or a change
  upstream.
- mDNS from inside the Flatpak sandbox.
- Bluetooth to a trainer and Wi-Fi traffic at once on a Deck. Both can
  be on 2.4 GHz. Watch for dropped trainer samples in the ride trace.
- Whether a Flatpak build that vendors crates would pull
  `steamworks-sys`, and Valve's files with it, into its sources even
  with the feature off. If so, the Flatpak needs a lock file or a
  workspace without `pp-steam`.
- The traffic figures are arithmetic on the message layout, not
  measurements. Framing overhead was estimated.
- Route generation producing identical bytes on Windows and macOS. The
  design does not depend on it.

## Sources

Read on 27 November 2026.

- [Steam Matchmaking and Lobbies](https://partner.steamgames.com/doc/features/multiplayer/matchmaking)
  and [ISteamMatchmaking](https://partner.steamgames.com/doc/api/ISteamMatchmaking):
  lobby types, the 250 member limit, metadata, search filters, owner
  hand-over, `+connect_lobby`.
- [Steam Datagram Relay](https://partner.steamgames.com/doc/features/multiplayer/steamdatagramrelay)
  and [Steam Networking](https://partner.steamgames.com/doc/features/multiplayer/networking):
  relay, hidden addresses, the deprecated older API.
- [ISteamNetworkingMessages](https://partner.steamgames.com/doc/api/ISteamNetworkingMessages)
  and [ISteamNetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets).
- [ISteamFriends](https://partner.steamgames.com/doc/api/ISteamFriends)
  and [Enhanced Rich Presence](https://partner.steamgames.com/doc/features/enhancedrichpresence):
  invite dialog, join callbacks, rich presence keys.
- [Steamworks API overview](https://partner.steamgames.com/doc/api/steam_api):
  what init needs, `steam_appid.txt`.
- [Leaderboards](https://partner.steamgames.com/doc/features/leaderboards)
  and [ISteamUserStats](https://partner.steamgames.com/doc/api/isteamuserstats).
- [ISteamUtils](https://partner.steamgames.com/doc/api/ISteamUtils):
  gamepad text input.
- [Remote Play](https://partner.steamgames.com/doc/features/remoteplay).
- [Distributing open source applications on Steam](https://partner.steamgames.com/doc/sdk/uploading/distributing_opensource)
  and the [SDK access agreement](https://partner.steamgames.com/documentation/sdk_access_agreement):
  permissive licences, and shipping the redistributable binaries.
- [Getting started with Steamworks](https://partner.steamgames.com/doc/gettingstarted):
  "a free suite of tools".
- Crates: [`steamworks` 0.13.1](https://crates.io/crates/steamworks),
  [`renet` 2.0.1](https://crates.io/crates/renet),
  [`renet_steam` 4.0.0](https://crates.io/crates/renet_steam),
  [`renet_netcode` 2.0.1](https://crates.io/crates/renet_netcode),
  [`postcard` 1.2.0](https://crates.io/crates/postcard),
  [`mdns-sd` 0.22.0](https://crates.io/crates/mdns-sd),
  [`iroh` 1.5.0](https://crates.io/crates/iroh).
- [iroh relays](https://iroh.computer/docs/concepts/relay) and
  [iroh pricing](https://www.iroh.computer/pricing).
- [Tailscale pricing](https://tailscale.com/pricing),
  [sharing](https://tailscale.com/kb/1084/sharing), and
  [Tailscale on a Steam Deck](https://tailscale.com/blog/steam-deck).
- [Epic Online Services overview](https://dev.epicgames.com/docs/en-US/epic-online-services/eos-overview).
