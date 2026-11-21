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
