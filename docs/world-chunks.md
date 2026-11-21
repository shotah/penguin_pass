# World chunks

`pp-world` decides which square chunks of the ground plane should exist
around the rider. It does not build meshes, sample noise, or link Godot.
The Godot side keeps one node per loaded chunk and frees it when the
planner says to unload.

Coordinates match the course: meters, right-handed, Y up, X east, Z
north. The ground plane is X/Z. Distance along a route is meters on the
3D centerline.

## Grid

A chunk is a square of `ChunkGrid::DEFAULT_EDGE_M` (128 m) on the ground
plane. `ChunkCoord { x, z }` is the column and row. Both are `i32`.

`coord_at` uses floor division, including for negative coordinates. The
chunk owns the half-open square `[index * edge, (index + 1) * edge)` on
each axis. A point exactly on the maximum edge belongs to the next chunk.

```text
        z
        ^
        |  (-1, 1) | (0, 1) | (1, 1)
        |----------+--------+--------
        |  (-1, 0) | (0, 0) | (1, 0)
        |----------+--------+--------
        |  (-1,-1) | (0,-1) | (1,-1)
        +----------------------------> x
                 origin
```

Intersection queries (circle, capsule) treat that square as closed, so a
shape that only touches a shared edge hits both chunks. `origin_m` is the
minimum corner. `center_m` is the middle. `bounds` reports the same square.
Query results are sorted by `x`, then `z`.

`i32` chunk indices with a 128 m edge cover world X/Z of about
±2.749×10^11 m. Past that, `coord_at` saturates instead of panicking.
Grid math uses `f64`, which represents every meter in that range exactly.
`Route::position_at` returns `f32`, because that is what `Course` returns.
`f32` spacing is about 0.03 m at 200 km, reaches 1 m near ±8.39×10^6 m
(about 8 400 km), and reaches the 128 m chunk edge near ±1.07×10^9 m.
A 200 km endless ride is safe. An endless road that runs for thousands of
kilometres should feed the planner positions that are still accurate in
`f32`, or the chunk under the rider can land in the wrong square.

## Seeds

```text
chunk_seed(world_seed, coord, layer) -> u64
```

`world_seed` selects the world, `coord` selects the chunk, and `layer`
selects an independent stream inside it. Use layer `0` for the terrain
heightfield and layer `1` for scenery. Neighbouring chunks and different
layers produce uncorrelated seeds.

This function is a compatibility promise. Saved games and shared seeds
depend on the exact bits. The mix will not change in a future version of
this crate; a different mix would be a different function. It is the
SplitMix64 finalizer (Vigna 2015, Stafford constants) applied to the world
seed, the sign-extended chunk indices, and the layer, with fixed odd
multipliers. The arithmetic is wrapping `u64`, so the result is the same
on every platform. The full steps are on `chunk_seed`.

## Planner

`ChunkPlanner` keeps the set of chunks that are loaded. Each update builds
the set the rider needs now and returns a budgeted list of loads and
unloads. The caller is assumed to do that work: the planner records the
chunks as loaded or unloaded immediately and will not ask again.

Three regions, all in meters:

- `near_radius_m` — every chunk that intersects the circle around the
  rider. The rider can see sideways and back down a switchback, so this
  is not a view wedge.
- `lookahead_m` and `corridor_half_width_m` — chunks along the centerline
  ahead of the rider, out to this distance along the route, within this
  half-width of the centerline. This is what keeps the road from popping
  in on a fast descent, past the view circle.
- `lookbehind_m` — the same corridor, shorter, behind the rider.

```text
                         road ahead
                            |
            +---+---+---+---+---+---+
            |   |   | c | c | c |   |
            +---+---+---+---+---+---+
            |   | n | n | c |   |   |
            +---+---+---+---+---+---+
            | n | n | R | n |   |   |     R  rider
            +---+---+---+---+---+---+     n  near radius
            |   | n | n | n |   |   |     c  corridor only
            +---+---+---+---+---+---+
            |   |   | b |   |   |   |
            +---+---+---+---+---+---+
                            |
                         road behind

    The unload margin is a band outside this shape.
    A loaded chunk stays until it is outside that band.
```

`unload_margin_m` is hysteresis. A loaded chunk is unloaded only once it
is outside the desired region by at least this margin (every radius above
is grown by the margin to form the keep set). A rider sitting on a chunk
boundary does not load and unload the same chunk every frame.

`max_loads_per_update` and `max_unloads_per_update` cap one frame. Loads
come out nearest first: the rider's own chunk, then by distance from the
rider to the chunk center. A chunk ahead of the rider is preferred to a
chunk behind it when the two distances are within 32 m (a chunk behind
sorts as if it were 32 m farther). Ties break by `ChunkCoord` order.
Unloads come out farthest first.

`prime` ignores the budgets and returns the whole desired set, in that
same order, for the load screen. It replaces the loaded set. `update` is
the per-frame call. `loaded`, `is_loaded`, and `pending_count` report the
set (`pending_count` is desired chunks not loaded yet). `reset` drops the
set; the caller frees whatever it had spawned.

On a loop (`Route::length_m` is `Some(len)`), lookahead and lookbehind
wrap with `rem_euclid`. Riding at 4950 m on a 5000 m lap looks ahead into
the start of the lap. On an open or endless route (`None`), distances
below 0 clamp to the start and nothing wraps. A teleport or a lap restart
is just a new `rider_distance_m`: the following updates load the new
region and unload the old one, still inside the budgets, without dropping
chunks the caller was not told to free.

`update` and `prime` sample the route only inside the lookahead,
lookbehind, and unload margin. The step is half the chunk edge. The
default config samples the route 15 times per call:

```text
1 + ceil(lookahead / step) + ceil(lookbehind / step) + 2 * ceil(margin / step)
```

That count does not grow with the length of the route. `route_samples_per_update`
returns it.

## Default config

| Field | Default | Why |
| --- | --- | --- |
| Chunk edge | 128 m | A few dozen chunks cover the view. |
| `near_radius_m` | 384 m | Three chunks. A few hundred meters in every direction. |
| `lookahead_m` | 640 m | 28.8 s of road at 80 km/h (22.22 m/s). 256 m of that is past the view circle, about 11.5 s to build a chunk before it is on screen. The 150 m of road that must already be loaded is 22 s inside this window. |
| `lookbehind_m` | 128 m | One chunk of road behind. Inside the default view circle on a straight; still covers the road just behind when the near radius is smaller. |
| `corridor_half_width_m` | 96 m | Reaches the neighbouring chunk. A 64 m sample on a 44 m hairpin bows about 11 m off the chord, which this covers. |
| `unload_margin_m` | 64 m | Half a chunk. A few meters of wobble on a boundary stays inside the keep set. |
| `max_loads_per_update` | 4 | 240 chunks per second at 60 fps. Crossing one chunk at 80 km/h takes about 5.8 s and adds a handful of chunks. |
| `max_unloads_per_update` | 4 | Same budget, so a teleport sheds old chunks as fast as it admits new ones. |

Measured with that config:

| | |
| --- | --- |
| `prime` at the Penguin Pass start line | 41 chunks |
| Updates to stream that prime under the load budget | 11 |
| Route samples per `update` or `prime` | 15 |
| Loaded chunks over a full Penguin Pass lap at 80 km/h and 60 updates per second | at most 55 |
| Loaded chunks over a 200 km straight endless road | at most 54 |

The lap starts from `prime`. After it, the loaded set is the desired set
for that position plus chunks still inside the unload margin. The endless
ride unloads the road behind; the loaded set stays near the rider, including
at chunk index x = 1562 (200 km / 128 m).

## Godot side

Keep one `ChunkPlanner` and one `world_seed: u64` for the route. The grid
on the planner is the grid used to place meshes.

At startup, on the load screen:

```rust
let mut planner = ChunkPlanner::default();
for coord in planner.prime(&course, rider_distance_m) {
    spawn_chunk(&planner, world_seed, coord);
}
```

Each frame, after the ride steps:

```rust
let delta = planner.update(&course, rider_distance_m);
for coord in delta.load {
    spawn_chunk(&planner, world_seed, coord);
}
for coord in delta.unload {
    free_chunk(coord);
}
```

`spawn_chunk` turns the coordinate into geometry:

1. `origin = planner.grid().origin_m(coord)` and `edge = planner.grid().edge_m()`.
   The mesh covers the square from `origin` to `origin + edge` on X and Z.
2. `terrain = chunk_seed(world_seed, coord, 0)` seeds the heightfield for
   that square only.
3. `scatter = chunk_seed(world_seed, coord, 1)` seeds props in the same
   square. The two streams do not match, so trees do not follow the
   height noise.
4. Parent the nodes under one node keyed by `ChunkCoord`, so unload can
   find them.

`free_chunk` frees that node and its meshes. Do not keep generating it.
The planner will ask for the chunk again only if the rider comes back.

Apply every coordinate in the delta. The planner already counts those
chunks as loaded or gone. Skipping one leaves a hole the planner will
not fill.

`pending_count` is the number of desired chunks still waiting on the load
budget. The load screen can use `prime` instead of waiting those frames.
A seek or a restart is the same `update` call with the new distance; the
budgets spread the catch-up over the following frames.

## Endless routes

Implement `Route` with `length_m` returning `None`. `position_at` may
return any finite point; the planner will not pass a negative distance.
`StraightRoute` is that shape: an open line along +X from the origin.
A generated road of unbounded length uses the same pair of calls, `prime`
once and `update` every frame. Chunks behind the margin unload, so the
loaded set stays a few dozen chunks no matter how far the rider goes.
The limit on that ride is `f32` position precision, described above, not
the planner's memory.
