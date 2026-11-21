# Bots

`pp-bots` puts other riders on the road. It does not draw, talk to a trainer, or link Godot. The session ticks it. The front end draws the slice from `riders`.

Distance is the same odometer as the player: metres from the start line, still increasing after each lap. Grade comes from the course. Slot `0` is nearest the centre line and slot `4` is the edge, the same five positions as the player's lane. The player starts in slot 2.

The bike model is `pp-course`'s. Each bot has a body mass, a 9 kg bike, rolling resistance 0.004, and a drag area near the player's 0.32 m². Power goes in, acceleration comes out, speed stays between a stop and the same descent cap as the player. A bot slows on the climb and runs on the descent because the forces say so, not because a script told it to.

## What the session calls

Build the field when the course loads. `seed` fixes names, kits, and the small differences between riders. The same seed, course, and player samples always produce the same ride.

```text
peloton = Peloton::new(&course, seed, BotConfig::default())
```

Each frame, in this order:

```text
factor = peloton.draft_factor(&player)     # player still at the start of the frame
profile.cda_m2 = settings.cda_m2 * factor  # a copy, never written back
frame  = ride.step(dt, &sample)
player = PlayerInput {
    total_distance_m: frame.distance_m,
    speed_mps: frame.speed_mps,
    power_w: the watts just applied,
    slot: eased lane fraction, 0.0..=4.0,
    mass_kg: body mass, not including the bike,
}
events = peloton.tick(dt, &player)
```

`draft_factor` is the number to multiply the player's drag area by. It is 1 when nobody ahead is close enough to shelter them. Apply it to a fresh copy of the settings profile every frame, the same way a powerup's drag scale is applied.

`tick` ignores a zero, negative, or non-finite `dt`. A hitch longer than 60 s is clamped. Inside the tick the step is split into pieces of at most 1/120 s, and the player's distance is walked from `end - speed * dt` up to the sample, so a 30 Hz frame and a 120 Hz frame of the same motion stay close. A 90 s flat ride at a steady 8 m/s and 200 W agrees within 8 m and 0.2 of a slot between those two rates.

`count: 0` builds an empty field. `tick` returns nothing and `draft_factor` stays 1.

## Who is on the road

The default is 8 riders. Four are pace riders. Four are company. `pace_riders: false` makes all of them company. A count above 24 is treated as 24.

Before the ride moves, the company is already rolling, a few tens of metres ahead of the line and a few tens behind, in different slots, at a little over 1 m/s. They are not stacked on the start. Pace riders are spread around the lap, so the opening group is the company.

| | What they do |
| --- | --- |
| Pace | Hold 1.5, 2.2, 3.0, or 3.8 W per kilogram of body mass for the whole ride. They lap the course. They are not removed. |
| Company | Pedal a personal scale of the player's smoothed power. They are replaced when they drift too far. |

Watts per kilogram uses body mass. The 9 kg bike is extra in the force balance, same as the player.

The player's power is smoothed with a 15 s time constant (`smooth_s` on the config). The first sample sets the average, so the company is already near the player's opening effort instead of creeping up from zero. After that it follows.

Each company rider then multiplies that average by a factor. The default four sit at 0.985, 0.995, 1.012, and 1.030. A slow sine wave of about ±2.8%, with its own period between about a minute and two, keeps the number moving. On a climb some push a few percent and some fade a few percent. The product of the factor, the wave, and the gap adjustment below is kept inside 0.85 to 1.15. The climb push is allowed to stick out of that band a little. Nothing in here sets a bot's speed from the player's speed.

Past 100 m of gap the rider who is ahead eases, and the rider who is behind pushes, up to about 10% by 240 m. That is a person noticing the group, not a lock. Inside 100 m the factor is left alone, and the gap is whatever the bike model and the draft produce.

When the smoothed power is under 40 W, a company rider will not sit there at 0 W. They soft-pedal at 1.2 W/kg of body mass, about 20–25 km/h on the flat for a normal weight. A stopped player watches them roll up the road. They are not held in place. Once they are 400 m away they are replaced by someone new, still out of sight.

A company rider more than `retire_m` away (default 400 m, and never under 360 m) is removed. A new rider appears at least `spawn_m` away (default 320 m). The config is pulled in so a new rider is at least 300 m away and at least 40 m inside the retire distance, plus up to 50 m of jitter, so they are not deleted on the next tick. Someone who rode off the front is replaced from behind, and the other way around, with a factor that points them back toward the player. The new body mass is near the player's. No rider is introduced or removed within 250 m. The opening field does not count as an introduction.

Pace riders keep their watts. On the rides below, each one's power stayed on the value `wkg * body mass`.

## Draft

`draft(gap_m, slot_offset)` is the follower's drag area as a fraction of their normal drag area. `gap_m` is how far ahead the leader is, on the odometer. `slot_offset` is the absolute difference of the two eased slots.

A rider directly on a wheel in a real paceline keeps roughly two thirds to three quarters of their air resistance. The effect is mostly gone by about ten metres, and a sideways shift of about a metre takes away the shelter. The curve here is that shape, with the close value set at a 27% saving:

| | Drag-area factor |
| --- | --- |
| 0 m to 3 m, same slot | 0.73 |
| Same slot, fading from 3 m to 12 m | 0.73 up to 1, on a smoothstep |
| 12 m or more, or level, or ahead | 1 |
| One slot to the side | half of the saving that the gap would have given |
| Two slots to the side | 1 |

One slot is the neighbouring line. Two slots is about a metre.

Several riders ahead do not multiply. The follower keeps the smallest factor, which is the single best shelter. Bots draft each other and the player with this same function. `Peloton::draft_factor` is that minimum for the player.

## Lanes

A bot has a home slot and eases toward a target. The ease is an exponential with a 0.34 s time constant, so a one-slot move settles in about a second.

Passing moves toward the centre line. The move starts while the gap is still a dozen metres or more, longer when the closing speed is high, so the rider has left the slot before the bikes overlap. If every slot toward the centre is taken, they use the edge rather than riding through someone. Once the wheel ahead is clear they ease back out toward home, one slot at a time, and only into a slot that is free.

Two riders in the same slot stay at least two bike lengths apart. A bike length here is 1.8 m, so the gap is 3.6 m. "Same slot" means the eased fractions differ by less than 0.5, so a rider halfway across has left the old slot. Side by side in neighbouring slots is allowed, even when the odometers match. If a rider is boxed in, they are held 3.8 m back rather than put on the same piece of road. A bot that is on top of the player steps off the player's slot.

## What a rider looks like

Each rider has a fixed id, a given name, a body mass, a preset (`nils` or `sigrid`), and a kit (`default`, `kit-sunrise`, `kit-glacier`, or `kit-kom-dots`). Those are strings. This crate does not depend on `pp-rider`. The id, name, preset, kit, mass, and pace effort stay put for as long as that rider is on the road. A replacement is a new id and a new name.

Cadence is `76 + 4 * W/kg`, minus `1.7` rpm per percent of uphill grade, plus a personal offset of a few rpm, and clamped to 55–110. An 8% grade at the same power is about 14 rpm lower than the flat. Standing is a 5.5 s flag every 32 s, and only when the grade is at least 6%, or when the rider's own wave is near a peak and the grade is at least 2%. It is a short effort, not the whole climb.

`pace_wkg` is `Some` for a pace rider and `None` for company.

## Events

| Event | When |
| --- | --- |
| `Passed { id, name }` | The player was behind this bot and is now ahead. They were within 60 m. |
| `PassedBy { id, name }` | This bot was behind the player and is now ahead. Same 60 m window. |
| `Joined { id }` | A company rider was introduced, already at least 300 m away. |
| `Left { id }` | A company rider was removed, already far enough that the gap was past the retire distance. |

The same bot will not raise another pass for 4 s. `Joined` and `Left` are bookkeeping. `Passed` and `PassedBy` are the ones worth a line of text. Catching a pace rider, or being caught by one, comes through these.

`nearest_ahead` and `nearest_behind` return the bot and a positive gap in metres. The slice from `riders` is in id order, not road order.

## What the game draws

For each entry in `riders()`:

- Place the bike with `course.position_at(total_distance_m)`. The course wraps negative distances and distances past the lap.
- Set the lateral offset from `slot` the same way the player's eased slot is turned into metres. Do not snap to an integer. The fraction is the move.
- Build the body from `preset` and `kit`.
- Drive the legs from `cadence_rpm`, the speed, and `standing`.
- `name` is there for a plate or a pass banner. `power_w` and `speed_mps` are there if a plate wants them.

Draw every rider in the slice, including ones hundreds of metres away. The crate does not hide them. A rider who is about to be removed is still in the slice until `tick` returns `Left`.

## What a 20 minute ride did

Default config, seed 21, 30 Hz. The player is the default 75 kg rider, drag area scaled by `draft_factor` each frame, holding a constant power. "In the group" means 2 to 6 bots whose odometer is within 200 m of the player.

| Course | Power | Share of time in the group | Nearby count |
| --- | ---: | ---: | --- |
| Penguin Pass | 100 W | 100% | 4 to 6 |
| Penguin Pass | 200 W | 97.4% | 2 to 7 |
| Penguin Pass | 350 W | 93.0% | 1 to 6 |
| Flat 5 km | 100 W | 100% | 4 to 5 |
| Flat 5 km | 200 W | 100% | 4 to 6 |
| Flat 5 km | 350 W | 100% | 3 to 5 |

The requirement was 80%. The short dips on Penguin Pass are the long climb and the descent pulling the edges of the group apart. At 200 W the count touches 7 while a pace rider is coming through. At 350 W it touches 1 for a minority of the ride. Nobody appeared or disappeared within 250 m, and no two riders shared a slot inside 3.6 m.

## Config

```text
BotConfig {
    count: 8,            // 0 turns the field off; values above 24 become 24
    pace_riders: true,
    smooth_s: 15.0,      // clamped to 0.5..=120
    retire_m: 400.0,     // clamped to 360..=2000
    spawn_m: 320.0,      // clamped to 300..=retire_m - 40
}
```

`BotConfig::default()` is the row above.

With pace riders turned off, every slot is company and they all try to stay near the player. A count of eight then can put more than six riders inside 200 m. The band in the table is the default, pace riders on.
