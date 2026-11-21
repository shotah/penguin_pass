//! Fish laid along one lap, and the per-ride collection state.
//!
//! Groups are readable shapes: a line in one slot, a diagonal across the
//! lane, a one-slot zigzag, or a single golden fish two slots off the
//! previous group. The same course length, grade profile, seed, and slot
//! count always lay the same fish.
//!
//! A fish is collected when the rider's total distance passes it and the
//! rider is within [`COLLECT_HALF_SLOT`] of its slot. Distance is an interval,
//! so a long frame cannot skip a fish. Fish return on the next lap.

use crate::rng::Rng;

/// Meters kept clear of fish after the start line.
pub const START_CLEAR_M: f64 = 100.0;

/// Meters kept clear of fish before the finish line.
pub const END_CLEAR_M: f64 = 50.0;

/// Shortest gap from the last fish of one group to the first of the next, in meters.
pub const GROUP_GAP_MIN_M: f64 = 150.0;

/// Longest gap from the last fish of one group to the first of the next, in meters.
pub const GROUP_GAP_MAX_M: f64 = 400.0;

/// Shortest spacing between fish inside a group, in meters.
pub const FISH_SPACING_MIN_M: f64 = 6.0;

/// Longest spacing between fish inside a group, in meters.
pub const FISH_SPACING_MAX_M: f64 = 10.0;

/// Seconds a one-slot lane change takes.
///
/// The view eases a tap over 0.3 s. This crate does not depend on the view;
/// the number is repeated so a pattern can be checked against that ease.
pub const LANE_EASE_S: f64 = 0.3;

/// Speed used when checking that a pattern is rideable, in meters per second.
///
/// 60 km/h. One slot at this speed takes [`METERS_PER_SLOT`] of road.
pub const LANE_CHANGE_SPEED_MPS: f64 = 60.0 / 3.6;

/// Meters of road one slot change needs at [`LANE_CHANGE_SPEED_MPS`].
///
/// 60 km/h for [`LANE_EASE_S`] is 5 m. Fish that differ by *n* slots are at
/// least *n* times this far apart, including across the start line.
pub const METERS_PER_SLOT: f64 = 5.0;

/// Grade, in percent, at and above which groups are short lines with long gaps.
///
/// The rider is working. Patterns stay in one slot and the next group waits
/// 320 m to 400 m.
pub const STEEP_GRADE_PCT: f32 = 5.0;

/// Grade, in percent, at and above which groups are only lines and diagonals.
///
/// Below this, including descents, groups may zigzag or offer a golden fish,
/// and the gap shrinks to 150 m to 250 m. From here up to [`STEEP_GRADE_PCT`]
/// the gap is 240 m to 320 m.
pub const MODERATE_GRADE_PCT: f32 = 2.0;

/// How close the rider must be, in slots, to collect a fish.
///
/// The rider's slot is a fraction from 0.0 at the centre side to `slots - 1`
/// at the edge. A rider at 2.5 is within half a slot of both slot 2 and slot
/// 3, and collects a fish in either. The test is `<=`.
pub const COLLECT_HALF_SLOT: f32 = 0.5;

const STEEP_GAP_MIN_M: f64 = 320.0;
const STEEP_GAP_MAX_M: f64 = 400.0;
const MODERATE_GAP_MIN_M: f64 = 240.0;
const MODERATE_GAP_MAX_M: f64 = 320.0;
const EASY_GAP_MIN_M: f64 = 150.0;
const EASY_GAP_MAX_M: f64 = 250.0;
const LINE_MIN: usize = 5;
const LINE_MAX: usize = 8;

const _: () = {
    assert!(STEEP_GAP_MIN_M > EASY_GAP_MAX_M);
    assert!(EASY_GAP_MIN_M >= GROUP_GAP_MIN_M);
    assert!(STEEP_GAP_MAX_M <= GROUP_GAP_MAX_M);
};

/// A readable run of fish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// Five to eight fish in one slot.
    Line,
    /// One fish per slot, stepping from one edge of the lane to the other.
    Diagonal,
    /// A one-slot weave about the middle of the lane.
    Zigzag,
    /// One golden fish, two slots off the previous group's slot.
    Golden,
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Line => "line",
            Self::Diagonal => "diagonal",
            Self::Zigzag => "zigzag",
            Self::Golden => "golden",
        })
    }
}

/// One fish on the lap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fish {
    /// Distance from the start line along the lap, in meters.
    pub distance_m: f64,
    /// Slot index. 0 is nearest the centre line.
    pub slot: u8,
    /// A golden fish. It counts in the golden total as well as the fish total.
    pub golden: bool,
}

/// One group in [`FishField::groups`].
///
/// `first` indexes [`FishField::fish`]. `first + count` stays inside that slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FishGroup {
    /// Shape of this group.
    pub pattern: Pattern,
    /// Index of the first fish.
    pub first: usize,
    /// How many fish are in the group.
    pub count: usize,
}

/// Every fish on one lap.
#[derive(Clone, Debug, PartialEq)]
pub struct FishField {
    length_m: f64,
    slots: u8,
    fish: Vec<Fish>,
    groups: Vec<FishGroup>,
}

impl FishField {
    /// Lay fish along one lap.
    ///
    /// `course_length_m` is the lap length in meters. `grade_at` is the road
    /// grade, in percent, at a distance along the lap; positive is uphill.
    /// `seed` fixes the layout. `slots` is the lane width, 5 in the game.
    /// Slot 0 is nearest the centre line.
    ///
    /// Fish sit in `[100, length - 50]` meters. Groups are separated by 150 m
    /// to 400 m of clear road, measured from the last fish of one group to
    /// the first of the next. Inside a group the spacing is 6 m to 10 m and
    /// constant, so the shape reads as a line. A group that would enter the
    /// last 50 m is left out.
    ///
    /// Steep road (grade at least 5%) gets a line of five and a long gap.
    /// Moderate climbs get lines and diagonals. Flats and descents get every
    /// pattern, including an occasional golden fish. A non-finite grade is
    /// treated as flat. A non-finite length, a length shorter than the two
    /// margins, or zero slots yields an empty field.
    #[must_use]
    pub fn generate(
        course_length_m: f64,
        grade_at: impl Fn(f64) -> f32,
        seed: u64,
        slots: u8,
    ) -> Self {
        let length_m = if course_length_m.is_finite() && course_length_m > 0.0 {
            course_length_m
        } else {
            0.0
        };
        let mut field = Self {
            length_m,
            slots,
            fish: Vec::new(),
            groups: Vec::new(),
        };
        if slots == 0 || length_m < START_CLEAR_M + END_CLEAR_M {
            return field;
        }
        let end = length_m - END_CLEAR_M;
        let mut rng = Rng::new(seed);
        let mut cursor = START_CLEAR_M;
        let mut anchor = slots / 2;
        let mut guard = 0u32;
        while cursor <= end && guard < 10_000 {
            guard += 1;
            let grade = finite_grade(grade_at(cursor));
            let plan = Plan::roll(&mut rng, grade, slots, anchor);
            if !plan.fits(cursor, end) {
                break;
            }
            anchor = plan.last_slot();
            field.push_group(&plan, cursor);
            let Some(last) = field.fish.last().map(|fish| fish.distance_m) else {
                break;
            };
            cursor = last + gap_m(&mut rng, grade);
        }
        field
    }

    /// Number of fish on the lap.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fish.len()
    }

    /// No fish were placed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fish.is_empty()
    }

    /// Fish in lap order.
    #[must_use]
    pub fn fish(&self) -> &[Fish] {
        &self.fish
    }

    /// Groups in lap order. They partition [`Self::fish`].
    #[must_use]
    pub fn groups(&self) -> &[FishGroup] {
        &self.groups
    }

    /// Lap length used to place the fish, in meters.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.length_m
    }

    /// Slot count passed to [`Self::generate`].
    #[must_use]
    pub fn slots(&self) -> u8 {
        self.slots
    }

    fn push_group(&mut self, plan: &Plan, cursor: f64) {
        let first = self.fish.len();
        let golden = plan.pattern == Pattern::Golden;
        for (i, slot) in plan.slots.iter().copied().enumerate() {
            self.fish.push(Fish {
                distance_m: cursor + (i as f64) * plan.spacing,
                slot,
                golden,
            });
        }
        self.groups.push(FishGroup {
            pattern: plan.pattern,
            first,
            count: plan.slots.len(),
        });
    }
}

/// What happened when the rider passed a fish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FishEvent {
    /// The rider was within half a slot.
    ///
    /// `index` is into [`FishField::fish`] and is the same on every lap.
    /// `combo` is the run of collects since the last miss, including this one.
    Collected {
        /// Index into the field.
        index: usize,
        /// The fish is golden.
        golden: bool,
        /// Consecutive collects, including this one.
        combo: u32,
    },
    /// The rider passed the fish outside half a slot. The combo resets.
    Missed {
        /// Index into the field.
        index: usize,
    },
}

/// A fish still ahead of the rider, for drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FishAhead {
    /// Index into the field.
    pub index: usize,
    /// Lap distance of the fish, in meters.
    pub distance_m: f64,
    /// Meters of road from the rider to the next time this fish appears.
    pub ahead_m: f64,
    /// Slot index.
    pub slot: u8,
    /// The fish is golden.
    pub golden: bool,
}

/// Per-ride collection state.
///
/// Fish return each lap. Totals count every lap. The combo is consecutive
/// fish along the road, and a miss in any slot breaks it.
#[derive(Clone, Debug)]
pub struct FishRun {
    field: FishField,
    next_abs: Vec<f64>,
    count: u32,
    golden_count: u32,
    combo: u32,
    best_combo: u32,
}

impl FishRun {
    /// Start a ride over `field`. Nothing has been passed yet.
    #[must_use]
    pub fn new(field: &FishField) -> Self {
        let next_abs = field.fish.iter().map(|fish| fish.distance_m).collect();
        Self {
            field: field.clone(),
            next_abs,
            count: 0,
            golden_count: 0,
            combo: 0,
            best_combo: 0,
        }
    }

    /// The field this run was built from.
    #[must_use]
    pub fn field(&self) -> &FishField {
        &self.field
    }

    /// Fish collected this ride, over every lap.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Golden fish collected this ride.
    #[must_use]
    pub fn golden_count(&self) -> u32 {
        self.golden_count
    }

    /// Consecutive collects since the last miss.
    #[must_use]
    pub fn combo(&self) -> u32 {
        self.combo
    }

    /// Longest combo this ride.
    #[must_use]
    pub fn best_combo(&self) -> u32 {
        self.best_combo
    }

    /// Pass every fish in `(prev_distance_m, distance_m]`.
    ///
    /// Distances are total meters ridden, counting across laps, and should
    /// move forward. `slot` is the rider's lateral position at `distance_m`,
    /// as a fractional slot. Every fish in the interval is judged at that
    /// slot. A frame that also changes lanes should be short enough that the
    /// lane is already at the fish: at 30 Hz and 80 km/h a frame is under a
    /// metre, and fish in a group are at least 6 m apart.
    ///
    /// A non-finite distance, or a distance that does not move forward,
    /// emits nothing and changes nothing. The same interval reported twice
    /// emits nothing the second time.
    #[must_use]
    pub fn tick(&mut self, prev_distance_m: f64, distance_m: f64, slot: f32) -> Vec<FishEvent> {
        if !prev_distance_m.is_finite() || !distance_m.is_finite() || distance_m <= prev_distance_m
        {
            return Vec::new();
        }
        let length = self.field.length_m;
        if !length.is_finite() || length <= 0.0 {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for (index, next) in self.next_abs.iter_mut().enumerate() {
            advance_past(next, prev_distance_m, length);
            let mut steps = 0u32;
            while *next <= distance_m && steps < 100_000 {
                let at = *next;
                hits.push((at, index));
                *next += length;
                steps += 1;
                if *next <= at {
                    break;
                }
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.emit(hits, slot)
    }

    /// Fish whose next appearance is within `window_m` ahead of `distance_m`.
    ///
    /// A fish the rider has already passed this lap shows up again only when
    /// the next lap's copy is inside the window. The result is ordered by
    /// distance ahead. A non-positive or non-finite window yields nothing.
    #[must_use]
    pub fn ahead(&self, distance_m: f64, window_m: f64) -> Vec<FishAhead> {
        if !distance_m.is_finite() || !window_m.is_finite() || window_m <= 0.0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (index, fish) in self.field.fish.iter().enumerate() {
            let abs = self.upcoming(index, distance_m);
            let ahead_m = abs - distance_m;
            if ahead_m > 0.0 && ahead_m <= window_m {
                out.push(FishAhead {
                    index,
                    distance_m: fish.distance_m,
                    ahead_m,
                    slot: fish.slot,
                    golden: fish.golden,
                });
            }
        }
        out.sort_by(|a, b| a.ahead_m.total_cmp(&b.ahead_m).then(a.index.cmp(&b.index)));
        out
    }

    fn emit(&mut self, hits: Vec<(f64, usize)>, slot: f32) -> Vec<FishEvent> {
        let mut events = Vec::with_capacity(hits.len());
        for (_distance, index) in hits {
            let fish = self.field.fish[index];
            let close =
                slot.is_finite() && (slot - f32::from(fish.slot)).abs() <= COLLECT_HALF_SLOT;
            if close {
                self.count = self.count.saturating_add(1);
                if fish.golden {
                    self.golden_count = self.golden_count.saturating_add(1);
                }
                self.combo = self.combo.saturating_add(1);
                self.best_combo = self.best_combo.max(self.combo);
                events.push(FishEvent::Collected {
                    index,
                    golden: fish.golden,
                    combo: self.combo,
                });
            } else {
                self.combo = 0;
                events.push(FishEvent::Missed { index });
            }
        }
        events
    }

    fn upcoming(&self, index: usize, distance_m: f64) -> f64 {
        let abs = self.next_abs[index];
        let length = self.field.length_m;
        if abs > distance_m || !length.is_finite() || length <= 0.0 {
            return abs;
        }
        let laps = ((distance_m - abs) / length).floor() + 1.0;
        if laps.is_finite() && laps > 0.0 {
            abs + laps * length
        } else {
            abs
        }
    }
}

fn advance_past(next: &mut f64, prev: f64, length: f64) {
    if *next > prev {
        return;
    }
    let laps = ((prev - *next) / length).floor() + 1.0;
    if laps.is_finite() && laps > 0.0 {
        *next += laps * length;
    }
}

fn finite_grade(grade: f32) -> f32 {
    if grade.is_finite() { grade } else { 0.0 }
}

fn gap_m(rng: &mut Rng, grade: f32) -> f64 {
    let (lo, hi) = gap_bounds(grade);
    draw(rng, lo, hi)
}

fn gap_bounds(grade: f32) -> (f64, f64) {
    if grade >= STEEP_GRADE_PCT {
        (STEEP_GAP_MIN_M, STEEP_GAP_MAX_M)
    } else if grade >= MODERATE_GRADE_PCT {
        (MODERATE_GAP_MIN_M, MODERATE_GAP_MAX_M)
    } else {
        (EASY_GAP_MIN_M, EASY_GAP_MAX_M)
    }
}

fn spacing_m(rng: &mut Rng) -> f64 {
    draw(rng, FISH_SPACING_MIN_M, FISH_SPACING_MAX_M)
}

fn draw(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    if hi > lo {
        lo + (hi - lo) * rng.unit()
    } else {
        lo
    }
}

struct Plan {
    pattern: Pattern,
    spacing: f64,
    slots: Vec<u8>,
}

impl Plan {
    fn roll(rng: &mut Rng, grade: f32, slot_count: u8, anchor: u8) -> Self {
        let pattern = pick_pattern(rng, grade, slot_count);
        let plan = match pattern {
            Pattern::Line => None,
            Pattern::Diagonal => diagonal(rng, slot_count),
            Pattern::Zigzag => zigzag(rng, grade, slot_count),
            Pattern::Golden => golden(rng, slot_count, anchor),
        };
        plan.filter(Plan::feasible)
            .unwrap_or_else(|| line(rng, grade, slot_count))
    }

    fn feasible(&self) -> bool {
        if self.slots.len() < 2 {
            return !self.slots.is_empty();
        }
        let max_step = self
            .slots
            .windows(2)
            .map(|pair| u32::from(pair[0].abs_diff(pair[1])))
            .max()
            .unwrap_or(0);
        let need = f64::from(max_step) * METERS_PER_SLOT;
        self.spacing + 1.0e-9 >= need
            && (FISH_SPACING_MIN_M..=FISH_SPACING_MAX_M).contains(&self.spacing)
    }

    fn fits(&self, cursor: f64, end: f64) -> bool {
        if self.slots.is_empty() {
            return false;
        }
        let span = (self.slots.len() - 1) as f64 * self.spacing;
        cursor.is_finite() && cursor + span <= end
    }

    fn last_slot(&self) -> u8 {
        self.slots.last().copied().unwrap_or(0)
    }
}

fn pick_pattern(rng: &mut Rng, grade: f32, slots: u8) -> Pattern {
    if slots < 2 || grade >= STEEP_GRADE_PCT {
        return Pattern::Line;
    }
    let roll = rng.index(100);
    if grade >= MODERATE_GRADE_PCT {
        return if roll < 75 {
            Pattern::Line
        } else {
            Pattern::Diagonal
        };
    }
    if slots < 3 {
        return match roll {
            0..=49 => Pattern::Line,
            50..=74 => Pattern::Diagonal,
            _ => Pattern::Zigzag,
        };
    }
    match roll {
        0..=39 => Pattern::Line,
        40..=64 => Pattern::Diagonal,
        65..=89 => Pattern::Zigzag,
        _ => Pattern::Golden,
    }
}

fn line(rng: &mut Rng, grade: f32, slots: u8) -> Plan {
    let slot = u8::try_from(rng.index(u32::from(slots))).unwrap_or(0);
    Plan {
        pattern: Pattern::Line,
        spacing: spacing_m(rng),
        slots: vec![slot; fish_count(rng, grade)],
    }
}

fn fish_count(rng: &mut Rng, grade: f32) -> usize {
    if grade >= STEEP_GRADE_PCT {
        LINE_MIN
    } else if grade >= MODERATE_GRADE_PCT {
        LINE_MIN + usize::try_from(rng.index(2)).unwrap_or(0)
    } else {
        let choices = u32::try_from(LINE_MAX - LINE_MIN + 1).unwrap_or(1);
        LINE_MIN + usize::try_from(rng.index(choices)).unwrap_or(0)
    }
}

fn diagonal(rng: &mut Rng, slots: u8) -> Option<Plan> {
    if slots < 2 {
        return None;
    }
    let count = usize::from(slots).min(LINE_MAX);
    let rising = rng.coin();
    let mut seq = Vec::with_capacity(count);
    for step in 0..u8::try_from(count).unwrap_or(0) {
        let slot = if rising { step } else { slots - 1 - step };
        seq.push(slot);
    }
    Some(Plan {
        pattern: Pattern::Diagonal,
        spacing: spacing_m(rng),
        slots: seq,
    })
}

fn zigzag(rng: &mut Rng, grade: f32, slots: u8) -> Option<Plan> {
    if slots < 2 {
        return None;
    }
    let count = fish_count(rng, grade);
    let mut slot = slots / 2;
    let mut dir: i16 = if rng.coin() { 1 } else { -1 };
    let mut seq = Vec::with_capacity(count);
    for _ in 0..count {
        seq.push(slot);
        let next = i16::from(slot) + dir;
        if !(0..i16::from(slots)).contains(&next) {
            dir = -dir;
        }
        let moved = i16::from(slot) + dir;
        slot = u8::try_from(moved).unwrap_or(slot);
    }
    Some(Plan {
        pattern: Pattern::Zigzag,
        spacing: spacing_m(rng),
        slots: seq,
    })
}

fn golden(rng: &mut Rng, slots: u8, anchor: u8) -> Option<Plan> {
    let mut choices = Vec::new();
    for slot in 0..slots {
        if slot.abs_diff(anchor) == 2 {
            choices.push(slot);
        }
    }
    let n = u32::try_from(choices.len()).unwrap_or(0);
    if n == 0 {
        return None;
    }
    let slot = choices[usize::try_from(rng.index(n)).unwrap_or(0)];
    Some(Plan {
        pattern: Pattern::Golden,
        spacing: 0.0,
        slots: vec![slot],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(field: &FishField, grade_at: impl Fn(f64) -> f32) {
        assert_margins(field);
        assert_partition(field);
        assert_shapes(field, &grade_at);
        assert_gaps(field, &grade_at);
        assert_lane_changes(field);
    }

    fn assert_margins(field: &FishField) {
        let end = field.length_m() - END_CLEAR_M;
        for fish in field.fish() {
            assert!(
                fish.distance_m >= START_CLEAR_M && fish.distance_m <= end,
                "fish at {} m",
                fish.distance_m
            );
            assert!(fish.slot < field.slots());
        }
        let mut previous = f64::NEG_INFINITY;
        for fish in field.fish() {
            assert!(fish.distance_m > previous, "fish must be in order");
            previous = fish.distance_m;
        }
    }

    fn assert_partition(field: &FishField) {
        let mut cursor = 0;
        for group in field.groups() {
            assert_eq!(group.first, cursor);
            assert!(group.count > 0);
            cursor += group.count;
        }
        assert_eq!(cursor, field.len());
    }

    fn assert_shapes(field: &FishField, grade_at: &impl Fn(f64) -> f32) {
        for (index, group) in field.groups().iter().enumerate() {
            let grade = finite_grade(grade_at(field.fish()[group.first].distance_m));
            assert!(
                pattern_allowed(group.pattern, grade, field.slots()),
                "{grade}% produced {}",
                group.pattern
            );
            assert!(count_allowed(group, grade, field.slots()));
            let slice = &field.fish()[group.first..group.first + group.count];
            match group.pattern {
                Pattern::Line => assert_line(slice),
                Pattern::Diagonal => assert_diagonal(slice, field.slots()),
                Pattern::Zigzag => assert_zigzag(slice),
                Pattern::Golden => assert_golden(field, index),
            }
            if slice.len() >= 2 {
                let gap = slice[1].distance_m - slice[0].distance_m;
                assert!((FISH_SPACING_MIN_M..=FISH_SPACING_MAX_M).contains(&gap));
                for window in slice.windows(2) {
                    let step = window[1].distance_m - window[0].distance_m;
                    assert!(
                        (step - gap).abs() < 1e-9,
                        "spacing inside a group is constant"
                    );
                }
            }
        }
    }

    fn pattern_allowed(pattern: Pattern, grade: f32, slots: u8) -> bool {
        if grade >= STEEP_GRADE_PCT || slots < 2 {
            return pattern == Pattern::Line;
        }
        if grade >= MODERATE_GRADE_PCT {
            return matches!(pattern, Pattern::Line | Pattern::Diagonal);
        }
        pattern != Pattern::Golden || slots >= 3
    }

    fn count_allowed(group: &FishGroup, grade: f32, slots: u8) -> bool {
        match group.pattern {
            Pattern::Golden => group.count == 1,
            Pattern::Diagonal => group.count == usize::from(slots).min(LINE_MAX),
            Pattern::Line | Pattern::Zigzag => {
                let (lo, hi) = if grade >= STEEP_GRADE_PCT {
                    (LINE_MIN, LINE_MIN)
                } else if grade >= MODERATE_GRADE_PCT {
                    (LINE_MIN, LINE_MIN + 1)
                } else {
                    (LINE_MIN, LINE_MAX)
                };
                (lo..=hi).contains(&group.count)
            }
        }
    }

    fn assert_line(slice: &[Fish]) {
        assert!(
            slice
                .iter()
                .all(|fish| !fish.golden && fish.slot == slice[0].slot)
        );
    }

    fn assert_diagonal(slice: &[Fish], slots: u8) {
        assert!(slice.iter().all(|fish| !fish.golden));
        let rising: Vec<u8> = (0..slice.len())
            .map(|i| u8::try_from(i).unwrap_or(0))
            .collect();
        let falling: Vec<u8> = (0..slice.len())
            .map(|i| slots - 1 - u8::try_from(i).unwrap_or(0))
            .collect();
        let got: Vec<u8> = slice.iter().map(|fish| fish.slot).collect();
        assert!(got == rising || got == falling, "{got:?}");
    }

    fn assert_zigzag(slice: &[Fish]) {
        assert!(slice.iter().all(|fish| !fish.golden));
        assert!(
            slice
                .windows(2)
                .all(|pair| pair[0].slot.abs_diff(pair[1].slot) == 1)
        );
        let slots: Vec<u8> = slice.iter().map(|fish| fish.slot).collect();
        assert!(slots.iter().any(|slot| *slot != slots[0]), "zigzag moves");
    }

    fn assert_golden(field: &FishField, index: usize) {
        let group = field.groups()[index];
        let fish = field.fish()[group.first];
        assert!(fish.golden);
        let anchor = if index == 0 {
            field.slots() / 2
        } else {
            let prev = field.groups()[index - 1];
            field.fish()[prev.first + prev.count - 1].slot
        };
        assert_eq!(fish.slot.abs_diff(anchor), 2);
    }

    fn assert_gaps(field: &FishField, grade_at: &impl Fn(f64) -> f32) {
        let groups = field.groups();
        for pair in groups.windows(2) {
            let origin = field.fish()[pair[0].first];
            let left = field.fish()[pair[0].first + pair[0].count - 1];
            let right = field.fish()[pair[1].first];
            let gap = right.distance_m - left.distance_m;
            let grade = finite_grade(grade_at(origin.distance_m));
            let (lo, hi) = gap_bounds(grade);
            assert!(
                gap >= lo && gap <= hi,
                "gap {gap} m after grade {grade}%, expected {lo}..={hi}"
            );
            assert!((GROUP_GAP_MIN_M..=GROUP_GAP_MAX_M).contains(&gap));
        }
    }

    fn assert_lane_changes(field: &FishField) {
        let fish = field.fish();
        if fish.len() < 2 {
            return;
        }
        for window in fish.windows(2) {
            let distance = window[1].distance_m - window[0].distance_m;
            assert_change(window[0], window[1], distance);
        }
        let wrap = (field.length_m() - fish.last().expect("len").distance_m) + fish[0].distance_m;
        assert_change(*fish.last().expect("len"), fish[0], wrap);
    }

    fn assert_change(from: Fish, to: Fish, distance: f64) {
        let need = f64::from(from.slot.abs_diff(to.slot)) * METERS_PER_SLOT;
        assert!(
            distance + 1e-6 >= need,
            "slot {} to {} in {distance} m needs {need} m",
            from.slot,
            to.slot
        );
    }

    #[test]
    fn flat_and_steep_roads_follow_the_layout_rules() {
        for seed in 0..12 {
            let flat = FishField::generate(10_000.0, |_| 0.0, seed, 5);
            let steep = FishField::generate(10_000.0, |_| 8.0, seed, 5);
            let moderate = FishField::generate(10_000.0, |_| 3.0, seed, 5);
            check(&flat, |_| 0.0);
            check(&steep, |_| 8.0);
            check(&moderate, |_| 3.0);
            assert!(flat.groups().len() > steep.groups().len(), "seed {seed}");
            assert!(
                steep
                    .groups()
                    .iter()
                    .all(|group| group.pattern == Pattern::Line)
            );
            assert!(steep.fish().iter().all(|fish| !fish.golden));
            assert!(
                moderate
                    .groups()
                    .iter()
                    .all(|group| { matches!(group.pattern, Pattern::Line | Pattern::Diagonal) })
            );
        }
    }

    #[test]
    fn easy_roads_use_every_pattern_and_steep_roads_do_not() {
        let mut saw_golden = false;
        let mut saw_zigzag = false;
        let mut saw_diagonal = false;
        for seed in 0..30 {
            let field = FishField::generate(10_000.0, |_| -4.0, seed, 5);
            for group in field.groups() {
                match group.pattern {
                    Pattern::Golden => saw_golden = true,
                    Pattern::Zigzag => saw_zigzag = true,
                    Pattern::Diagonal => saw_diagonal = true,
                    Pattern::Line => {}
                }
            }
        }
        assert!(saw_golden && saw_zigzag && saw_diagonal);
    }

    #[test]
    fn the_built_in_loop_and_a_flat_course_are_deterministic() {
        let course = pp_course::Course::penguin_pass();
        let length = course.length_m();
        let once = FishField::generate(length, |d| course.grade_pct_at(d), 1, 5);
        let twice = FishField::generate(length, |d| course.grade_pct_at(d), 1, 5);
        assert_eq!(once, twice);
        check(&once, |d| course.grade_pct_at(d));
        assert!(!once.is_empty());

        let flat = FishField::generate(10_000.0, |_| 0.0, 1, 5);
        let again = FishField::generate(10_000.0, |_| 0.0, 1, 5);
        let other = FishField::generate(10_000.0, |_| 0.0, 2, 5);
        assert_eq!(flat, again);
        assert_ne!(flat.fish(), other.fish());
        check(&flat, |_| 0.0);
    }

    #[test]
    fn a_non_finite_grade_still_places_fish_and_a_short_lap_does_not() {
        let field = FishField::generate(2_000.0, |_| f32::NAN, 1, 5);
        check(&field, |_| 0.0);
        assert!(!field.is_empty());
        assert!(FishField::generate(120.0, |_| 0.0, 1, 5).is_empty());
        assert!(FishField::generate(f64::NAN, |_| 0.0, 1, 5).is_empty());
        assert!(FishField::generate(10_000.0, |_| 0.0, 1, 0).is_empty());
    }

    fn handmade(length: f64, fish: Vec<Fish>) -> FishField {
        FishField {
            length_m: length,
            slots: 5,
            fish,
            groups: Vec::new(),
        }
    }

    fn sample_field() -> FishField {
        handmade(
            1_000.0,
            vec![
                Fish {
                    distance_m: 200.0,
                    slot: 2,
                    golden: false,
                },
                Fish {
                    distance_m: 208.0,
                    slot: 2,
                    golden: false,
                },
                Fish {
                    distance_m: 216.0,
                    slot: 3,
                    golden: false,
                },
                Fish {
                    distance_m: 400.0,
                    slot: 2,
                    golden: true,
                },
            ],
        )
    }

    fn ride(
        field: &FishField,
        end: f64,
        speed: f64,
        hz: f64,
        slot_at: impl Fn(f64) -> f32,
    ) -> (FishRun, Vec<FishEvent>) {
        let mut run = FishRun::new(field);
        let mut events = Vec::new();
        let mut dist = 0.0;
        let step = speed / hz;
        while dist < end {
            let next = (dist + step).min(end);
            if next <= dist {
                break;
            }
            events.extend(run.tick(dist, next, slot_at(next)));
            dist = next;
        }
        (run, events)
    }

    #[test]
    fn collection_matches_across_sample_rates_and_speeds() {
        let field = sample_field();
        let end = field.length_m() * 2.0;
        let (_run, reference) = ride(&field, end, 60.0 / 3.6, 60.0, |_| 2.0);
        let combos: Vec<u32> = reference
            .iter()
            .filter_map(|event| match event {
                FishEvent::Collected { combo, .. } => Some(*combo),
                FishEvent::Missed { .. } => None,
            })
            .collect();
        assert_eq!(combos, vec![1, 2, 1, 2, 3, 1]);
        assert_eq!(
            reference
                .iter()
                .filter(|event| matches!(event, FishEvent::Collected { golden: true, .. }))
                .count(),
            2
        );
        for hz in [30.0, 60.0, 144.0] {
            for kph in [5.0, 80.0] {
                let (_run, events) = ride(&field, end, kph / 3.6, hz, |_| 2.0);
                assert_eq!(events, reference, "{kph} km/h at {hz} Hz");
            }
        }
        let mut jumped = FishRun::new(&field);
        let events = jumped.tick(0.0, end, 2.0);
        assert_eq!(events, reference);
        assert_eq!(jumped.count(), 6);
        assert_eq!(jumped.golden_count(), 2);
        assert_eq!(jumped.best_combo(), 3);
        assert_eq!(jumped.combo(), 1);
    }

    #[test]
    fn a_lane_change_between_fish_collects_the_same_way_at_every_rate() {
        let field = handmade(
            1_000.0,
            vec![
                Fish {
                    distance_m: 180.0,
                    slot: 0,
                    golden: false,
                },
                Fish {
                    distance_m: 500.0,
                    slot: 4,
                    golden: false,
                },
                Fish {
                    distance_m: 520.0,
                    slot: 0,
                    golden: false,
                },
            ],
        );
        let slot_at = |distance: f64| if distance < 350.0 { 0.0 } else { 4.0 };
        let (_run, reference) = ride(&field, 1_000.0, 60.0 / 3.6, 60.0, slot_at);
        assert!(matches!(
            reference.as_slice(),
            [
                FishEvent::Collected {
                    index: 0,
                    combo: 1,
                    ..
                },
                FishEvent::Collected {
                    index: 1,
                    combo: 2,
                    ..
                },
                FishEvent::Missed { index: 2 },
            ]
        ));
        for hz in [30.0, 60.0, 144.0] {
            for kph in [5.0, 80.0] {
                let (_run, events) = ride(&field, 1_000.0, kph / 3.6, hz, slot_at);
                assert_eq!(events, reference, "{kph} km/h at {hz} Hz");
            }
        }
    }

    #[test]
    fn mid_move_slots_use_a_half_slot_window() {
        let field = handmade(
            1_000.0,
            vec![
                Fish {
                    distance_m: 100.0,
                    slot: 2,
                    golden: false,
                },
                Fish {
                    distance_m: 110.0,
                    slot: 3,
                    golden: false,
                },
            ],
        );
        let collect = |slot: f32| FishRun::new(&field).tick(0.0, 200.0, slot);
        assert!(matches!(
            collect(2.0).as_slice(),
            [
                FishEvent::Collected { index: 0, .. },
                FishEvent::Missed { index: 1 }
            ]
        ));
        assert!(matches!(
            collect(2.5).as_slice(),
            [
                FishEvent::Collected {
                    index: 0,
                    combo: 1,
                    ..
                },
                FishEvent::Collected {
                    index: 1,
                    combo: 2,
                    ..
                },
            ]
        ));
        assert!(matches!(
            collect(2.51).as_slice(),
            [
                FishEvent::Missed { index: 0 },
                FishEvent::Collected { index: 1, .. }
            ]
        ));
        assert!(matches!(
            collect(1.5).as_slice(),
            [
                FishEvent::Collected { index: 0, .. },
                FishEvent::Missed { index: 1 }
            ]
        ));
        assert!(
            collect(f32::NAN)
                .iter()
                .all(|event| matches!(event, FishEvent::Missed { .. }))
        );
    }

    #[test]
    fn fish_return_on_the_next_lap_and_a_repeated_interval_does_not() {
        let field = handmade(
            1_000.0,
            vec![
                Fish {
                    distance_m: 120.0,
                    slot: 1,
                    golden: false,
                },
                Fish {
                    distance_m: 980.0,
                    slot: 1,
                    golden: true,
                },
            ],
        );
        let mut run = FishRun::new(&field);
        let events = run.tick(900.0, 1_200.0, 1.0);
        assert_eq!(
            events,
            vec![
                FishEvent::Collected {
                    index: 1,
                    golden: true,
                    combo: 1
                },
                FishEvent::Collected {
                    index: 0,
                    golden: false,
                    combo: 2
                },
            ]
        );
        assert!(run.tick(900.0, 1_200.0, 1.0).is_empty());
        assert!(run.tick(1_200.0, 900.0, 1.0).is_empty());
        let again = run.tick(1_900.0, 2_200.0, 1.0);
        assert_eq!(
            again,
            vec![
                FishEvent::Collected {
                    index: 1,
                    golden: true,
                    combo: 3
                },
                FishEvent::Collected {
                    index: 0,
                    golden: false,
                    combo: 4
                },
            ]
        );
        assert_eq!(run.count(), 4);
        assert_eq!(run.golden_count(), 2);
        assert_eq!(run.best_combo(), 4);
    }

    #[test]
    fn ahead_lists_only_the_window_and_wraps_the_lap() {
        let field = sample_field();
        let run = FishRun::new(&field);
        let near = run.ahead(0.0, 205.0);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].index, 0);
        assert!((near[0].ahead_m - 200.0).abs() < 1e-9);
        let window = run.ahead(190.0, 30.0);
        assert_eq!(window.len(), 3);
        assert!(window[0].ahead_m < window[1].ahead_m);
        assert!(run.ahead(0.0, 0.0).is_empty());

        let mut run = FishRun::new(&field);
        let _ = run.tick(0.0, 210.0, 2.0);
        let after = run.ahead(210.0, 20.0);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].index, 2);

        let wrapped = run.ahead(950.0, 300.0);
        assert!(
            wrapped
                .iter()
                .any(|fish| fish.index == 0 && fish.ahead_m < 300.0)
        );
    }
}
