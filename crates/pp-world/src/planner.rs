//! Which chunks should be loaded around the rider.

use std::cmp::Ordering;
use std::collections::HashSet;

use crate::{ChunkCoord, ChunkGrid, Route, WorldError};

/// How far a chunk behind the rider can be and still load before a chunk
/// ahead of it, in meters. Within this gap the chunk in front wins.
const AHEAD_BIAS_M: f64 = 32.0;

/// Distances and per-frame budgets for [`ChunkPlanner`].
///
/// Distances are meters. The defaults suit a rider at the 80 km/h descent
/// cap, a view of a few hundred meters, and a 128 m chunk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannerConfig {
    /// Radius around the rider that is loaded in every direction, in meters.
    ///
    /// The rider can see sideways across a valley and back down a switchback,
    /// so this is a circle, not a wedge. Default: 384 m, three 128 m chunks.
    pub near_radius_m: f64,
    /// How far along the route, ahead of the rider, the road corridor is
    /// loaded, in meters.
    ///
    /// Default: 640 m. At 80 km/h (22.22 m/s) that is 28.8 s of road, and
    /// 256 m of it sits past the 384 m view, about 11.5 s of time to build a
    /// chunk before it is on screen. The 150 m of road that must already be
    /// resident is 22 s inside this window.
    pub lookahead_m: f64,
    /// How far along the route, behind the rider, the road corridor is
    /// loaded, in meters.
    ///
    /// Default: 128 m, one chunk. On the default near radius this is already
    /// inside the view circle; it still covers the road immediately behind
    /// when the near radius is smaller, and on a hairpin where the path
    /// distance leaves the circle.
    pub lookbehind_m: f64,
    /// Half-width of the road corridor, in meters, measured from the centerline.
    ///
    /// Default: 96 m. That reaches the neighbouring chunk on each side of the
    /// centerline. A 64 m sample step on a 44 m hairpin bows about 11 m off
    /// the chord, which this half-width covers.
    pub corridor_half_width_m: f64,
    /// Extra meters a loaded chunk may sit outside the desired set before it
    /// is unloaded.
    ///
    /// Default: 64 m, half a chunk. A rider oscillating a few meters across a
    /// chunk boundary stays inside this band, so the set does not thrash.
    /// The keep region is the desired region with every radius grown by this
    /// margin.
    pub unload_margin_m: f64,
    /// Maximum chunks [`ChunkPlanner::update`] may load in one call.
    ///
    /// Default: 4. At 60 fps that is 240 chunks per second. Crossing one
    /// chunk at 80 km/h takes about 5.8 s and adds a handful of chunks, so
    /// four per frame stays ahead of the rider.
    pub max_loads_per_update: usize,
    /// Maximum chunks [`ChunkPlanner::update`] may unload in one call.
    ///
    /// Default: 4, matching the load budget so a teleport sheds old chunks
    /// as fast as it admits new ones.
    pub max_unloads_per_update: usize,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            near_radius_m: 384.0,
            lookahead_m: 640.0,
            lookbehind_m: 128.0,
            corridor_half_width_m: 96.0,
            unload_margin_m: 64.0,
            max_loads_per_update: 4,
            max_unloads_per_update: 4,
        }
    }
}

impl PlannerConfig {
    fn validate(self) -> Result<Self, WorldError> {
        for (field, value) in [
            ("near_radius_m", self.near_radius_m),
            ("lookahead_m", self.lookahead_m),
            ("lookbehind_m", self.lookbehind_m),
            ("corridor_half_width_m", self.corridor_half_width_m),
            ("unload_margin_m", self.unload_margin_m),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(WorldError::Config { field });
            }
        }
        Ok(self)
    }
}

/// Chunks to spawn and chunks to free after one planner step.
///
/// [`ChunkPlanner::update`] has already recorded both lists. The caller must
/// spawn `load` and free `unload` before the next call; the planner will not
/// ask again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDelta {
    /// Chunks to create, nearest first. The rider's own chunk is first when
    /// it is in this list.
    pub load: Vec<ChunkCoord>,
    /// Chunks to free, farthest from the rider first.
    pub unload: Vec<ChunkCoord>,
}

/// Streaming set of loaded chunks.
///
/// Call [`Self::prime`] once on the load screen, then [`Self::update`] once
/// per frame with the rider's distance along the route. Both calls sample
/// the route only inside the lookahead, lookbehind, and unload margin. The
/// number of samples does not grow with the length of the route.
#[derive(Debug, Clone)]
pub struct ChunkPlanner {
    grid: ChunkGrid,
    config: PlannerConfig,
    loaded: Vec<ChunkCoord>,
    desired: HashSet<ChunkCoord>,
    keep: HashSet<ChunkCoord>,
    pending: usize,
    focus: Focus,
    offsets: Vec<f64>,
    samples: Vec<[f64; 2]>,
    ordered: Vec<ChunkCoord>,
}

#[derive(Debug, Clone, Copy)]
struct Focus {
    x: f64,
    z: f64,
    forward_x: f64,
    forward_z: f64,
    chunk: ChunkCoord,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            x: 0.0,
            z: 0.0,
            forward_x: 1.0,
            forward_z: 0.0,
            chunk: ChunkCoord::new(0, 0),
        }
    }
}

impl Default for ChunkPlanner {
    /// Default 128 m grid and [`PlannerConfig::default`].
    fn default() -> Self {
        Self::from_parts(ChunkGrid::default(), PlannerConfig::default())
    }
}

impl ChunkPlanner {
    /// Planner for `grid` using `config`.
    ///
    /// Returns [`WorldError::Config`] when a distance is negative or not finite.
    pub fn try_new(grid: ChunkGrid, config: PlannerConfig) -> Result<Self, WorldError> {
        Ok(Self::from_parts(grid, config.validate()?))
    }

    fn from_parts(grid: ChunkGrid, config: PlannerConfig) -> Self {
        Self {
            grid,
            config,
            loaded: Vec::with_capacity(1024),
            desired: HashSet::with_capacity(1024),
            keep: HashSet::with_capacity(1024),
            pending: 0,
            focus: Focus::default(),
            offsets: Vec::with_capacity(64),
            samples: Vec::with_capacity(64),
            ordered: Vec::with_capacity(1024),
        }
    }

    /// Grid this planner queries.
    #[must_use]
    pub const fn grid(&self) -> &ChunkGrid {
        &self.grid
    }

    /// Config copied in at construction.
    #[must_use]
    pub const fn config(&self) -> &PlannerConfig {
        &self.config
    }

    /// Chunks currently treated as loaded, in [`ChunkCoord`] order.
    pub fn loaded(&self) -> impl Iterator<Item = ChunkCoord> + '_ {
        self.loaded.iter().copied()
    }

    /// Whether `coord` is in the loaded set.
    #[must_use]
    pub fn is_loaded(&self, coord: ChunkCoord) -> bool {
        self.loaded.binary_search(&coord).is_ok()
    }

    /// Desired chunks that are not loaded yet.
    #[must_use]
    pub const fn pending_count(&self) -> usize {
        self.pending
    }

    /// Drop every loaded chunk and the last desired set.
    ///
    /// The caller frees any chunk it had spawned. The next [`Self::update`]
    /// starts from an empty world.
    pub fn reset(&mut self) {
        self.loaded.clear();
        self.desired.clear();
        self.keep.clear();
        self.pending = 0;
        self.focus = Focus::default();
    }

    /// Exact number of [`Route::position_at`] calls made by one [`Self::update`]
    /// or [`Self::prime`].
    ///
    /// The step is half the chunk edge. The window is the lookahead and the
    /// lookbehind, each extended by the unload margin so hysteresis does not
    /// need a second pass over the route. The count is
    ///
    /// ```text
    /// 1 + ceil(lookahead / step) + ceil(lookbehind / step) + 2 * ceil(margin / step)
    /// ```
    ///
    /// and does not depend on the route length. The default config samples
    /// the route 15 times.
    #[must_use]
    pub fn route_samples_per_update(&self) -> usize {
        let step = sample_step_m(self.grid.edge_m());
        let margin = extent_steps(self.config.unload_margin_m, step);
        1 + extent_steps(self.config.lookahead_m, step)
            + extent_steps(self.config.lookbehind_m, step)
            + margin
            + margin
    }

    /// Load the full desired set for the load screen, ignoring budgets.
    ///
    /// Replaces the loaded set with every chunk the rider needs at
    /// `rider_distance_m`. The returned list is that set in load priority
    /// order: the rider's chunk, then nearer chunks, with a chunk ahead of
    /// the rider preferred to a chunk behind it when their distances are
    /// within 32 m. Any chunk previously loaded and absent from the list
    /// must be freed by the caller.
    #[must_use]
    pub fn prime(&mut self, route: &impl Route, rider_distance_m: f64) -> Vec<ChunkCoord> {
        self.rebuild(route, rider_distance_m);
        self.fill_ordered();
        self.loaded.clear();
        self.loaded.extend(self.ordered.iter().copied());
        self.loaded.sort_unstable();
        self.loaded.dedup();
        self.pending = 0;
        self.ordered.clone()
    }

    /// Advance the loaded set for the rider at `rider_distance_m`.
    ///
    /// Computes the desired set, then loads at most `max_loads_per_update`
    /// missing chunks (priority order) and unloads at most
    /// `max_unloads_per_update` chunks that have fallen outside the desired
    /// set by at least `unload_margin_m` (farthest first). Those chunks are
    /// recorded immediately.
    ///
    /// A large jump in `rider_distance_m` does not panic and does not drop
    /// the budgets. Later calls finish the loads and unloads.
    ///
    /// On a loop, lookahead and lookbehind wrap at the lap length. On an
    /// open route, distances below 0 clamp to the start.
    #[must_use]
    pub fn update(&mut self, route: &impl Route, rider_distance_m: f64) -> ChunkDelta {
        self.rebuild(route, rider_distance_m);
        let load = self.collect_loads();
        let unload = self.collect_unloads();
        for coord in &unload {
            self.loaded_remove(*coord);
        }
        for coord in &load {
            self.loaded_insert(*coord);
        }
        self.recompute_pending();
        ChunkDelta { load, unload }
    }

    fn rebuild(&mut self, route: &impl Route, rider_distance_m: f64) {
        let length = loop_length(route.length_m());
        let anchor = anchor_distance(rider_distance_m, length);
        let step = sample_step_m(self.grid.edge_m());
        let lookahead = self.config.lookahead_m;
        let lookbehind = self.config.lookbehind_m;
        let margin = self.config.unload_margin_m;

        self.offsets.clear();
        self.offsets.push(0.0);
        push_extent(lookahead, step, 1.0, 0.0, &mut self.offsets);
        push_extent(margin, step, 1.0, lookahead, &mut self.offsets);
        push_extent(lookbehind, step, -1.0, 0.0, &mut self.offsets);
        push_extent(margin, step, -1.0, lookbehind, &mut self.offsets);
        self.offsets.sort_unstable_by(f64::total_cmp);

        self.samples.clear();
        for offset in &self.offsets {
            let distance = distance_at(anchor, *offset, length);
            self.samples.push(finite_xz(route.position_at(distance)));
        }
        debug_assert_eq!(self.samples.len(), self.route_samples_per_update());

        self.focus = focus_from(&self.offsets, &self.samples, self.grid);
        self.desired.clear();
        self.keep.clear();

        let grid = self.grid;
        let half = self.config.corridor_half_width_m;
        let rider = self.focus;
        grid.visit_circle(rider.x, rider.z, self.config.near_radius_m, |coord| {
            let _ = self.desired.insert(coord);
        });
        grid.visit_circle(
            rider.x,
            rider.z,
            self.config.near_radius_m + margin,
            |coord| {
                let _ = self.keep.insert(coord);
            },
        );

        let segments = self.offsets.len().saturating_sub(1);
        for index in 0..segments {
            let start_offset = self.offsets[index];
            let end_offset = self.offsets[index + 1];
            let start = self.samples[index];
            let end = self.samples[index + 1];
            grid.visit_capsule(start[0], start[1], end[0], end[1], half + margin, |coord| {
                let _ = self.keep.insert(coord);
            });
            if in_window(start_offset, lookbehind, lookahead)
                && in_window(end_offset, lookbehind, lookahead)
            {
                grid.visit_capsule(start[0], start[1], end[0], end[1], half, |coord| {
                    let _ = self.desired.insert(coord);
                });
            }
        }

        let _ = self.desired.insert(self.focus.chunk);
        self.keep.extend(self.desired.iter().copied());
        debug_assert!(self.desired.iter().all(|coord| self.keep.contains(coord)));
    }

    fn fill_ordered(&mut self) {
        self.ordered.clear();
        self.ordered.extend(self.desired.iter().copied());
        let grid = self.grid;
        let focus = self.focus;
        self.ordered
            .sort_unstable_by(|left, right| load_ordering(*left, *right, grid, focus));
    }

    fn collect_loads(&mut self) -> Vec<ChunkCoord> {
        self.fill_ordered();
        let budget = self.config.max_loads_per_update;
        let mut load = Vec::new();
        for coord in &self.ordered {
            if load.len() == budget {
                break;
            }
            if self.loaded.binary_search(coord).is_err() {
                load.push(*coord);
            }
        }
        load
    }

    fn collect_unloads(&self) -> Vec<ChunkCoord> {
        let mut unload = Vec::new();
        for coord in &self.loaded {
            if !self.keep.contains(coord) {
                unload.push(*coord);
            }
        }
        let grid = self.grid;
        let focus = self.focus;
        unload.sort_unstable_by(|left, right| unload_ordering(*left, *right, grid, focus));
        unload.truncate(self.config.max_unloads_per_update);
        unload
    }

    fn loaded_insert(&mut self, coord: ChunkCoord) {
        if let Err(index) = self.loaded.binary_search(&coord) {
            self.loaded.insert(index, coord);
        }
    }

    fn loaded_remove(&mut self, coord: ChunkCoord) {
        if let Ok(index) = self.loaded.binary_search(&coord) {
            self.loaded.remove(index);
        }
    }

    fn recompute_pending(&mut self) {
        self.pending = self
            .desired
            .iter()
            .filter(|coord| self.loaded.binary_search(coord).is_err())
            .count();
    }
}

fn sample_step_m(edge_m: f64) -> f64 {
    edge_m * 0.5
}

fn extent_steps(extent_m: f64, step_m: f64) -> usize {
    if !extent_m.is_finite() || extent_m <= 0.0 || !step_m.is_finite() || step_m <= 0.0 {
        return 0;
    }
    let steps = (extent_m / step_m).ceil();
    if !steps.is_finite() || steps <= 0.0 {
        return 0;
    }
    if steps >= usize::MAX as f64 {
        return usize::MAX;
    }
    steps as usize
}

fn push_extent(extent: f64, step: f64, sign: f64, base: f64, out: &mut Vec<f64>) {
    let steps = extent_steps(extent, step);
    for index in 1..=steps {
        let mut distance = step * (index as f64);
        if index == steps || distance > extent {
            distance = extent;
        }
        out.push(sign * (base + distance));
    }
}

fn loop_length(length: Option<f64>) -> Option<f64> {
    length.filter(|len| len.is_finite() && *len > 0.0)
}

fn anchor_distance(distance_m: f64, length: Option<f64>) -> f64 {
    if !distance_m.is_finite() {
        return 0.0;
    }
    match length {
        Some(len) => distance_m.rem_euclid(len),
        None => distance_m.max(0.0),
    }
}

fn distance_at(anchor: f64, offset: f64, length: Option<f64>) -> f64 {
    let distance = anchor + offset;
    if !distance.is_finite() {
        return 0.0;
    }
    match length {
        Some(len) => distance.rem_euclid(len),
        None => distance.max(0.0),
    }
}

fn finite_xz(position: [f32; 3]) -> [f64; 2] {
    let x = f64::from(position[0]);
    let z = f64::from(position[2]);
    if x.is_finite() && z.is_finite() {
        [x, z]
    } else {
        [0.0, 0.0]
    }
}

fn in_window(offset: f64, lookbehind_m: f64, lookahead_m: f64) -> bool {
    offset >= -lookbehind_m && offset <= lookahead_m
}

fn focus_from(offsets: &[f64], samples: &[[f64; 2]], grid: ChunkGrid) -> Focus {
    let rider_index = offsets
        .iter()
        .position(|offset| offset.abs() <= 1e-9)
        .unwrap_or(0);
    let rider = samples.get(rider_index).copied().unwrap_or([0.0, 0.0]);
    let mut forward = (1.0, 0.0);
    let mut found_ahead = false;
    let mut best_ahead = f64::MAX;
    let mut best_behind = f64::MIN;
    let mut behind = (1.0, 0.0);
    let mut found_behind = false;
    for (offset, sample) in offsets.iter().zip(samples.iter()) {
        if *offset > 1e-9 && *offset < best_ahead {
            best_ahead = *offset;
            forward = unit_xz(sample[0] - rider[0], sample[1] - rider[1]);
            found_ahead = true;
        } else if *offset < -1e-9 && (!found_behind || *offset > best_behind) {
            best_behind = *offset;
            behind = unit_xz(rider[0] - sample[0], rider[1] - sample[1]);
            found_behind = true;
        }
    }
    if !found_ahead && found_behind {
        forward = behind;
    }
    Focus {
        x: rider[0],
        z: rider[1],
        forward_x: forward.0,
        forward_z: forward.1,
        chunk: grid.coord_at(rider[0], rider[1]),
    }
}

fn unit_xz(x: f64, z: f64) -> (f64, f64) {
    let len_sq = x * x + z * z;
    if len_sq > 1e-12 {
        let len = len_sq.sqrt();
        (x / len, z / len)
    } else {
        (1.0, 0.0)
    }
}

fn center_distance(coord: ChunkCoord, grid: ChunkGrid, focus: Focus) -> f64 {
    let [x, z] = grid.center_m(coord);
    let dx = x - focus.x;
    let dz = z - focus.z;
    (dx * dx + dz * dz).sqrt()
}

fn priority_score(coord: ChunkCoord, grid: ChunkGrid, focus: Focus) -> f64 {
    let [x, z] = grid.center_m(coord);
    let dx = x - focus.x;
    let dz = z - focus.z;
    let distance = (dx * dx + dz * dz).sqrt();
    if dx * focus.forward_x + dz * focus.forward_z >= 0.0 {
        distance
    } else {
        distance + AHEAD_BIAS_M
    }
}

fn load_ordering(left: ChunkCoord, right: ChunkCoord, grid: ChunkGrid, focus: Focus) -> Ordering {
    let left_rank = u8::from(left != focus.chunk);
    let right_rank = u8::from(right != focus.chunk);
    left_rank
        .cmp(&right_rank)
        .then_with(|| {
            priority_score(left, grid, focus).total_cmp(&priority_score(right, grid, focus))
        })
        .then_with(|| left.x.cmp(&right.x))
        .then_with(|| left.z.cmp(&right.z))
}

fn unload_ordering(left: ChunkCoord, right: ChunkCoord, grid: ChunkGrid, focus: Focus) -> Ordering {
    center_distance(right, grid, focus)
        .total_cmp(&center_distance(left, grid, focus))
        .then_with(|| right.x.cmp(&left.x))
        .then_with(|| right.z.cmp(&left.z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Route, StraightRoute};
    use pp_course::Course;
    use std::cell::Cell;
    use std::collections::BTreeSet;

    /// Open line at a constant z, so tests can sit in the middle of a chunk.
    struct OffsetLine {
        z: f32,
    }

    impl Route for OffsetLine {
        fn position_at(&self, distance_m: f64) -> [f32; 3] {
            let x = if distance_m.is_finite() {
                distance_m.max(0.0) as f32
            } else {
                0.0
            };
            [x, 0.0, self.z]
        }

        fn length_m(&self) -> Option<f64> {
            None
        }
    }

    struct CountingRoute {
        calls: Cell<usize>,
        min_distance: Cell<f64>,
        length_m: Option<f64>,
    }

    impl CountingRoute {
        fn new(length_m: Option<f64>) -> Self {
            Self {
                calls: Cell::new(0),
                min_distance: Cell::new(f64::MAX),
                length_m,
            }
        }

        fn calls(&self) -> usize {
            self.calls.get()
        }

        fn min_distance(&self) -> f64 {
            self.min_distance.get()
        }

        fn reset_counts(&self) {
            self.calls.set(0);
            self.min_distance.set(f64::MAX);
        }
    }

    impl Route for CountingRoute {
        fn position_at(&self, distance_m: f64) -> [f32; 3] {
            self.calls.set(self.calls.get() + 1);
            if distance_m < self.min_distance.get() {
                self.min_distance.set(distance_m);
            }
            let x = if distance_m.is_finite() {
                distance_m as f32
            } else {
                0.0
            };
            [x, 0.0, 0.0]
        }

        fn length_m(&self) -> Option<f64> {
            self.length_m
        }
    }

    /// Circle that does not wrap on its own. The planner must wrap.
    struct RawCircle {
        length_m: f64,
        radius_m: f64,
        saw_outside: Cell<bool>,
    }

    impl Route for RawCircle {
        fn position_at(&self, distance_m: f64) -> [f32; 3] {
            if !distance_m.is_finite() || distance_m < 0.0 || distance_m >= self.length_m {
                self.saw_outside.set(true);
            }
            let angle = distance_m / self.radius_m;
            [
                (self.radius_m * angle.cos()) as f32,
                0.0,
                (self.radius_m * angle.sin()) as f32,
            ]
        }

        fn length_m(&self) -> Option<f64> {
            Some(self.length_m)
        }
    }

    fn config(
        near: f64,
        lookahead: f64,
        lookbehind: f64,
        half: f64,
        margin: f64,
        loads: usize,
        unloads: usize,
    ) -> PlannerConfig {
        PlannerConfig {
            near_radius_m: near,
            lookahead_m: lookahead,
            lookbehind_m: lookbehind,
            corridor_half_width_m: half,
            unload_margin_m: margin,
            max_loads_per_update: loads,
            max_unloads_per_update: unloads,
        }
    }

    #[test]
    fn try_new_rejects_bad_distances_and_default_samples_fifteen_times() {
        let grid = ChunkGrid::default();
        assert!(
            ChunkPlanner::try_new(
                grid,
                PlannerConfig {
                    near_radius_m: -1.0,
                    ..PlannerConfig::default()
                }
            )
            .is_err()
        );
        assert!(
            ChunkPlanner::try_new(
                grid,
                PlannerConfig {
                    lookahead_m: f64::NAN,
                    ..PlannerConfig::default()
                }
            )
            .is_err()
        );
        let planner = ChunkPlanner::default();
        assert_eq!(planner.route_samples_per_update(), 15);
        assert_eq!(planner.pending_count(), 0);
        assert_eq!(planner.loaded().count(), 0);
    }

    #[test]
    fn prime_covers_near_radius_and_lookahead_and_nothing_far() {
        let grid = ChunkGrid::try_new(128.0).unwrap();
        let cfg = config(100.0, 400.0, 80.0, 30.0, 0.0, 8, 8);
        let route = OffsetLine { z: 64.0 };
        let mut planner = ChunkPlanner::try_new(grid, cfg).unwrap();
        let rider = 1_000.0;
        let primed = planner.prime(&route, rider);
        assert_eq!(primed.first().copied(), Some(grid.coord_at(rider, 64.0)));

        let primed_set: BTreeSet<_> = primed.into_iter().collect();
        let near = set_of(grid.chunks_in_circle(rider, 64.0, cfg.near_radius_m));
        assert!(near.is_subset(&primed_set));

        let ahead = grid.coord_at(rider + 350.0, 64.0);
        assert!(primed_set.contains(&ahead));
        assert!(!near.contains(&ahead));
        assert!(primed_set.contains(&grid.coord_at(rider - 50.0, 64.0)));

        let side = ChunkCoord::new(grid.coord_at(rider, 64.0).x, 5);
        assert!(!primed_set.contains(&side));
        assert!(!primed_set.contains(&ChunkCoord::new(ahead.x + 30, 40)));

        let limit = cfg.lookahead_m
            + cfg.corridor_half_width_m
            + grid.edge_m() * std::f64::consts::SQRT_2
            + 1.0;
        for coord in &primed_set {
            let [cx, cz] = grid.center_m(*coord);
            let dist = ((cx - rider).powi(2) + (cz - 64.0).powi(2)).sqrt();
            assert!(dist <= limit, "{coord:?} is {dist} m from the rider");
        }
    }

    #[test]
    fn budgets_and_priority_are_nearest_first_with_the_rider_chunk_first() {
        let grid = ChunkGrid::try_new(128.0).unwrap();
        let cfg = config(200.0, 64.0, 64.0, 16.0, 0.0, 2, 2);
        let route = OffsetLine { z: 64.0 };
        let rider = 64.0;
        let mut reference = ChunkPlanner::try_new(grid, cfg).unwrap();
        let full = reference.prime(&route, rider);
        assert_eq!(full[0], ChunkCoord::new(0, 0));
        let ahead = full
            .iter()
            .position(|coord| *coord == ChunkCoord::new(1, 0));
        let behind = full
            .iter()
            .position(|coord| *coord == ChunkCoord::new(-1, 0));
        let ahead = ahead.expect("chunk ahead of the rider");
        let behind = behind.expect("chunk behind the rider");
        assert!(ahead < behind);

        let mut staged = ChunkPlanner::try_new(grid, cfg).unwrap();
        let mut got = Vec::new();
        let mut steps = 0usize;
        loop {
            let delta = staged.update(&route, rider);
            assert!(delta.unload.is_empty());
            assert!(delta.load.len() <= cfg.max_loads_per_update);
            if delta.load.is_empty() {
                break;
            }
            got.extend(delta.load);
            steps += 1;
            assert!(steps < 100);
        }
        assert_eq!(got, full);
        assert_eq!(steps, full.len().div_ceil(cfg.max_loads_per_update));
        assert_eq!(staged.pending_count(), 0);
        assert!(staged.is_loaded(ChunkCoord::new(0, 0)));
    }

    #[test]
    fn hysteresis_ignores_a_few_meters_of_boundary_wobble() {
        let grid = ChunkGrid::try_new(128.0).unwrap();
        // Chunk (2, 0) starts at x = 256. At x = 150 it is 106 m away, just
        // outside a 100 m radius. At x = 156 it is 100 m away and included.
        // The 6 m wobble is inside the 30 m unload margin.
        let cfg = config(100.0, 0.0, 0.0, 0.0, 30.0, 64, 64);
        let route = OffsetLine { z: 64.0 };
        let behind = 150.0;
        let ahead = 156.0;

        let mut at_behind = ChunkPlanner::try_new(grid, cfg).unwrap();
        let mut at_ahead = ChunkPlanner::try_new(grid, cfg).unwrap();
        let set_behind: BTreeSet<_> = at_behind.prime(&route, behind).into_iter().collect();
        let set_ahead: BTreeSet<_> = at_ahead.prime(&route, ahead).into_iter().collect();
        assert_ne!(set_behind, set_ahead);

        let mut planner = ChunkPlanner::try_new(grid, cfg).unwrap();
        let _ = planner.prime(&route, behind);
        let mut delta = planner.update(&route, ahead);
        let mut guard = 0usize;
        while !delta.load.is_empty() || !delta.unload.is_empty() {
            delta = planner.update(&route, ahead);
            guard += 1;
            assert!(guard < 10);
        }
        for step in 0..20 {
            let distance = if step % 2 == 0 { behind } else { ahead };
            let delta = planner.update(&route, distance);
            assert!(
                delta.load.is_empty() && delta.unload.is_empty(),
                "wobble {step} moved {:?}",
                delta
            );
        }
    }

    #[test]
    fn lookahead_wraps_across_the_lap_seam() {
        let length = 2_000.0;
        let radius = length / std::f64::consts::TAU;
        let route = RawCircle {
            length_m: length,
            radius_m: radius,
            saw_outside: Cell::new(false),
        };
        let grid = ChunkGrid::try_new(32.0).unwrap();
        // Near radius stays local. Lookahead of 300 m from 1800 m wraps
        // onto distance 100, on the far side of the circle.
        let cfg = config(40.0, 300.0, 0.0, 16.0, 0.0, 64, 64);
        let mut planner = ChunkPlanner::try_new(grid, cfg).unwrap();
        let rider = 1_800.0;
        let primed: BTreeSet<_> = planner.prime(&route, rider).into_iter().collect();
        assert!(
            !route.saw_outside.get(),
            "planner passed an unwrapped distance"
        );

        let start = route_position_wrapped(&route, 100.0);
        let start_chunk = grid.coord_at(f64::from(start[0]), f64::from(start[2]));
        assert!(primed.contains(&start_chunk));

        let rider_pos = route_position_wrapped(&route, rider);
        let geographic = distance_xz(rider_pos, start);
        assert!(geographic > cfg.near_radius_m + grid.edge_m());
        let near = set_of(grid.chunks_in_circle(
            f64::from(rider_pos[0]),
            f64::from(rider_pos[2]),
            cfg.near_radius_m,
        ));
        assert!(!near.contains(&start_chunk));

        let course = Course::penguin_pass();
        let lap = course.length_m();
        let course_rider = lap - 100.0;
        let narrow = config(40.0, 220.0, 0.0, 24.0, 0.0, 64, 64);
        let mut course_planner =
            ChunkPlanner::try_new(ChunkGrid::try_new(64.0).unwrap(), narrow).unwrap();
        let loaded: BTreeSet<_> = course_planner
            .prime(&course, course_rider)
            .into_iter()
            .collect();
        let early = course.position_at(120.0);
        let early_chunk = course_planner
            .grid()
            .coord_at(f64::from(early[0]), f64::from(early[2]));
        let here = course.position_at(course_rider);
        let apart = distance_xz(here, early);
        assert!(apart > narrow.near_radius_m);
        assert!(loaded.contains(&early_chunk));
        let here_near = set_of(course_planner.grid().chunks_in_circle(
            f64::from(here[0]),
            f64::from(here[2]),
            narrow.near_radius_m,
        ));
        assert!(!here_near.contains(&early_chunk));
    }

    #[test]
    fn open_route_does_not_sample_behind_the_origin() {
        let grid = ChunkGrid::try_new(128.0).unwrap();
        let cfg = config(0.0, 300.0, 500.0, 10.0, 0.0, 64, 64);
        let mut planner = ChunkPlanner::try_new(grid, cfg).unwrap();
        let route = CountingRoute::new(None);
        let primed = planner.prime(&route, -25.0);
        assert!(route.min_distance() >= 0.0);
        assert!(primed.iter().all(|coord| coord.x >= -1));
        assert!(primed.contains(&ChunkCoord::new(1, 0)));
        assert!(!primed.iter().any(|coord| coord.x < -1));
    }

    #[test]
    fn reset_clears_loaded_chunks() {
        let mut planner = ChunkPlanner::default();
        let _ = planner.prime(&StraightRoute, 0.0);
        assert!(planner.loaded().count() > 0);
        planner.reset();
        assert_eq!(planner.loaded().count(), 0);
        assert_eq!(planner.pending_count(), 0);
        assert!(!planner.is_loaded(ChunkCoord::new(0, 0)));
    }

    #[test]
    fn update_sample_count_depends_on_config_not_route_length() {
        let cfg = PlannerConfig::default();
        let lengths = [Some(50.0), Some(1_000.0), Some(5_000_000.0), None];
        for length in lengths {
            let route = CountingRoute::new(length);
            let mut planner = ChunkPlanner::try_new(ChunkGrid::default(), cfg).unwrap();
            let bound = planner.route_samples_per_update();
            let _ = planner.update(&route, 10_000.0);
            assert_eq!(route.calls(), bound, "length {length:?}");
            route.reset_counts();
            let _ = planner.prime(&route, 10_000.0);
            assert_eq!(route.calls(), bound, "prime length {length:?}");
            assert!(route.min_distance() >= 0.0);
        }
    }

    #[test]
    fn teleport_converges_within_a_budget_bound() {
        let course = Course::penguin_pass();
        let cfg = PlannerConfig::default();
        let budget = cfg
            .max_loads_per_update
            .max(cfg.max_unloads_per_update)
            .max(1);
        let mut planner = ChunkPlanner::try_new(ChunkGrid::default(), cfg).unwrap();
        let start: BTreeSet<_> = planner.prime(&course, 0.0).into_iter().collect();
        let far = course.length_m() * 0.5;
        let mut preview = ChunkPlanner::try_new(ChunkGrid::default(), cfg).unwrap();
        let expected: BTreeSet<_> = preview.prime(&course, far).into_iter().collect();
        assert_ne!(start, expected);
        let bound = start.len().max(expected.len()).div_ceil(budget) + 2;

        let mut steps = 0usize;
        loop {
            let delta = planner.update(&course, far);
            steps += 1;
            assert!(delta.load.len() <= cfg.max_loads_per_update);
            assert!(delta.unload.len() <= cfg.max_unloads_per_update);
            if delta.load.is_empty() && delta.unload.is_empty() {
                break;
            }
            assert!(steps <= bound, "not converged after {steps}, bound {bound}");
        }
        assert_eq!(planner.pending_count(), 0);
        let loaded: BTreeSet<_> = planner.loaded().collect();
        assert!(expected.is_subset(&loaded));
        assert!(loaded.iter().all(|coord| planner.keep.contains(coord)));
        let pos = course.position_at(far);
        let under = planner
            .grid()
            .coord_at(f64::from(pos[0]), f64::from(pos[2]));
        assert!(planner.is_loaded(under));
    }

    #[test]
    fn penguin_pass_lap_at_80_kph_stays_ahead_and_bounded() {
        // Measured maximum with PlannerConfig::default on this lap.
        const LOADED_BOUND: usize = 55;
        let course = Course::penguin_pass();
        let cfg = PlannerConfig::default();
        let mut planner = ChunkPlanner::try_new(ChunkGrid::default(), cfg).unwrap();
        let prime_len = planner.prime(&course, 0.0).len();
        assert_eq!(prime_len, 41);
        assert_eq!(prime_len.div_ceil(cfg.max_loads_per_update), 11);
        assert_eq!(planner.route_samples_per_update(), 15);
        let mut max_loaded = planner.loaded().count();
        let step = (80.0 / 3.6) / 60.0;
        let lap = course.length_m();
        let mut distance = 0.0;
        let mut frames = 0usize;
        while distance < lap {
            distance = (distance + step).min(lap);
            let delta = planner.update(&course, distance);
            assert!(delta.load.len() <= cfg.max_loads_per_update);
            assert!(delta.unload.len() <= cfg.max_unloads_per_update);
            assert_road_ahead(&planner, &course, distance);
            max_loaded = max_loaded.max(planner.loaded().count());
            frames += 1;
            assert!(frames < 20_000);
        }
        let mut settled = 0usize;
        loop {
            let delta = planner.update(&course, distance);
            settled += 1;
            assert!(delta.load.len() <= cfg.max_loads_per_update);
            assert!(delta.unload.len() <= cfg.max_unloads_per_update);
            max_loaded = max_loaded.max(planner.loaded().count());
            if delta.load.is_empty() && delta.unload.is_empty() {
                break;
            }
            assert!(
                settled <= 32,
                "did not settle, pending {}",
                planner.pending_count()
            );
        }
        assert_eq!(
            max_loaded, LOADED_BOUND,
            "measured loaded count changed; update docs/world-chunks.md"
        );
        assert_eq!(planner.pending_count(), 0);

        let mut fresh = ChunkPlanner::try_new(ChunkGrid::default(), cfg).unwrap();
        let prime_end: BTreeSet<_> = fresh.prime(&course, distance).into_iter().collect();
        let loaded: BTreeSet<_> = planner.loaded().collect();
        assert!(
            prime_end.is_subset(&loaded),
            "missing {}",
            prime_end.difference(&loaded).count()
        );
        assert!(loaded.iter().all(|coord| planner.keep.contains(coord)));
    }

    #[test]
    fn endless_straight_for_200_km_stays_bounded() {
        // i32 chunk indices with a 128 m edge cover about ±2.749e11 m.
        // f64 represents every meter in that range. f32 spacing at 2e5 m
        // is 0.03125 m, so this ride does not mis-bin a chunk. f32 spacing
        // reaches 1 m near ±8.39e6 m.
        // Measured maximum with PlannerConfig::default on this ride.
        const LOADED_BOUND: usize = 54;
        let cfg = PlannerConfig::default();
        let mut planner = ChunkPlanner::try_new(ChunkGrid::default(), cfg).unwrap();
        let _ = planner.prime(&StraightRoute, 0.0);
        let mut distance = 0.0;
        let mut max_loaded = planner.loaded().count();
        while distance < 200_000.0 {
            distance += 250.0;
            for _ in 0..12 {
                let delta = planner.update(&StraightRoute, distance);
                assert!(delta.load.len() <= cfg.max_loads_per_update);
                assert!(delta.unload.len() <= cfg.max_unloads_per_update);
            }
            assert_eq!(planner.pending_count(), 0);
            let count = planner.loaded().count();
            max_loaded = max_loaded.max(count);
            assert!(count <= LOADED_BOUND, "loaded {count} at {distance} m");
        }
        assert_eq!(
            max_loaded, LOADED_BOUND,
            "measured loaded count changed; update docs/world-chunks.md"
        );
        let pos = StraightRoute.position_at(distance);
        let coord = planner
            .grid()
            .coord_at(f64::from(pos[0]), f64::from(pos[2]));
        assert_eq!(coord.x, 1_562);
        assert!(planner.is_loaded(coord) || planner.is_loaded(ChunkCoord::new(coord.x, -1)));
        assert!(
            planner
                .loaded()
                .all(|chunk| { (chunk.x - coord.x).abs() < 20 && chunk.z.abs() <= 8 })
        );
    }

    fn assert_road_ahead(planner: &ChunkPlanner, course: &Course, distance: f64) {
        let grid = planner.grid();
        let here = course.position_at(distance);
        let under = grid.coord_at(f64::from(here[0]), f64::from(here[2]));
        assert!(
            planner.is_loaded(under),
            "rider chunk {under:?} missing at {distance} m"
        );
        let mut previous = here;
        let mut along: f64 = 0.0;
        while along < 150.0 {
            let next_along = (along + 16.0).min(150.0);
            let next = course.position_at(distance + next_along);
            let chunks = grid.chunks_in_capsule(
                f64::from(previous[0]),
                f64::from(previous[2]),
                f64::from(next[0]),
                f64::from(next[2]),
                4.0,
            );
            for coord in chunks {
                assert!(
                    planner.is_loaded(coord),
                    "chunk {coord:?} under the next {next_along} m missing at {distance} m"
                );
            }
            previous = next;
            along = next_along;
        }
    }

    fn set_of(coords: Vec<ChunkCoord>) -> BTreeSet<ChunkCoord> {
        coords.into_iter().collect()
    }

    fn route_position_wrapped(route: &RawCircle, distance: f64) -> [f32; 3] {
        route.position_at(distance.rem_euclid(route.length_m))
    }

    fn distance_xz(left: [f32; 3], right: [f32; 3]) -> f64 {
        let dx = f64::from(left[0] - right[0]);
        let dz = f64::from(left[2] - right[2]);
        (dx * dx + dz * dz).sqrt()
    }
}
