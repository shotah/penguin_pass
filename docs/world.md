# World ground

The ride stands on one height function, `Field::height`. Within 96 m of the
centreline that is the exact relief formula, so the road bed and its banks are
not a coarse grid. Past 192 m it is a baked sample of the same formula. The
fade between them depends only on position.

Corridor chunks cover the road. Outside them there is one far mesh: a 64 m
grid out to about 1.2 km past the course, then a 256 m grid out to about 5.6
km. Cells that sit wholly inside a corridor chunk are left out. Where a finer
edge meets a coarser one, the in-between vertices lie on the coarse edge.
Every open edge has a skirt that drops 40 m, so a gap cannot show sky.

The terrain shader hazes the outer fifth of that reach fully. Inside that,
colour still comes from the theme: meadow, forest, rock, and snow in the
alpine theme, sand and rock in the desert, chosen by height and slope.

Landmarks that pass a look at the rider's-eye frames are placed from CC0
models. A vista sits ahead of a bend, on the outside, past the apex. A
roadside site is 13 to 26 m from the centreline. A crest site is what the
road climbs toward. Something that spans the road stands on the centreline,
past the first 700 m, so the rider goes through it. Trees, rocks, and tall
grass stay out of each footprint plus an 8 m margin, and trees are thinned
along the sight line from the reveal point. On the 10 km mountain test
course the search placed 16 sites, one about every 625 m. Penguin Pass is
5 km and lands about eight.

A generated snow horn stands on the alpine horizon, in view from the start
straight. The desert horizon keeps the sandstone mesa and adds a dark cone
with a crater, a leaning smoke column, and a warm rim. Neither has to be in
view from the summit.

Time of day and weather live in `pp_world`. `hour_at(time, ride_seconds)`
maps `Auto` onto a full day every 30 minutes of ride time, starting at 08:00,
and maps Dawn, Day, Sunset, and Night onto fixed hours. `resolve_weather`
keeps a chosen weather, or drifts `Auto` across clear, cloudy, rain, cloudy,
snow, and fog. `atmosphere_at(theme, hour, weather)` returns sun elevation
and azimuth, colours, fog, stars, precipitation, wetness, and headlight.
`ride_atmosphere` does all three. Noon on a clear day is the theme's current
look. Snow on the desert theme is dust. The `Atmosphere` node under the
world applies that state to the sun, sky, fog, road, a headlight, and the
precipitation quads. `Atmosphere::default_atmosphere()` is clear alpine noon,
and the world build applies clear noon for whichever theme is loaded.
