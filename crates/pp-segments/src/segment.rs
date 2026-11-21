//! Climb, sprint, and lap segments along one course.

use std::collections::BTreeSet;

use pp_course::Course;

/// What a segment is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// A sustained uphill.
    Climb,
    /// The flattest short stretch that is not on a climb.
    Sprint,
    /// The whole lap, from the start line back to itself.
    Lap,
}

/// One timed stretch of a lap, in metres along the 3D centerline.
///
/// `start_m` and `end_m` are metres from the start line. A climb or sprint
/// with `end_m` greater than `start_m` stays inside one lap. One with
/// `end_m` less than `start_m` crosses the start line. The lap itself runs
/// from `0` to the lap length.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Stable id for this course. Records are keyed by it.
    ///
    /// The lap is `lap`. The sprint is `sprint`. A climb is `climb-` plus
    /// the start metre, rounded. The same course always produces the same ids.
    pub id: String,
    /// Rider-facing name.
    pub name: String,
    /// Climb, sprint, or lap.
    pub kind: SegmentKind,
    /// Start line, in metres along the lap.
    pub start_m: f64,
    /// Finish line, in metres along the lap.
    pub end_m: f64,
}

/// Grade at or above which a metre counts as the body of a climb, in percent.
const CLIMB_GRADE_PCT: f64 = 2.5;
/// Downhill metres that still belong to the climb.
const MAX_DIP_M: isize = 80;
/// Non-downhill metres below [`CLIMB_GRADE_PCT`] that still belong to the climb.
const MAX_BREATHER_M: isize = 150;
/// How far the start may walk back down the ramp, in metres.
const RAMP_M: isize = 150;
/// Minimum grade of that ramp, in percent.
const RAMP_GRADE_PCT: f64 = 0.5;
/// Shortest climb, in metres.
const MIN_CLIMB_M: isize = 400;
/// Lowest mean grade of a climb, in percent.
const MIN_AVG_GRADE_PCT: f64 = 3.0;
/// Smallest net rise that still counts, in metres of elevation.
const MIN_GAIN_M: f64 = 5.0;
const SPRINT_MIN_M: usize = 200;
const SPRINT_MAX_M: usize = 300;
const MAX_SAMPLES: usize = 200_000;

impl Segment {
    /// Length of this segment, in metres.
    ///
    /// `lap_m` is the course length. It is used when the segment crosses the
    /// start line.
    #[must_use]
    pub fn length_m(&self, lap_m: f64) -> f64 {
        if self.kind == SegmentKind::Lap {
            return if self.end_m >= self.start_m {
                self.end_m - self.start_m
            } else {
                0.0
            };
        }
        span_len(self.start_m, self.end_m, lap_m)
    }

    /// Mean of [`Course::grade_pct_at`](pp_course::Course::grade_pct_at) along
    /// this segment, in percent.
    ///
    /// Samples are one metre apart, starting at [`Self::start_m`]. A closed
    /// lap averages near zero.
    #[must_use]
    pub fn average_grade_pct(&self, course: &Course) -> f64 {
        let lap = course.length_m();
        let len = self.length_m(lap);
        if !len.is_finite() || len < 1.0 || !lap.is_finite() || lap <= 0.0 || len > 1.0e7 {
            return 0.0;
        }
        let steps = len.floor() as usize;
        if steps == 0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for i in 0..steps {
            let distance = (self.start_m + i as f64).rem_euclid(lap);
            sum += f64::from(course.grade_pct_at(distance));
        }
        sum / steps as f64
    }
}

/// Climbs, one sprint, and the lap on `course`.
///
/// The result is deterministic. Order is the lap, then climbs from the start
/// line, then the sprint. Climbs do not overlap each other or the sprint.
/// The lap overlaps everything.
///
/// A climb is at least 400 m with a mean grade of at least 3%. The body is
/// where the grade stays at or above 2.5%. A downhill nick of up to 80 m, or
/// a gentler breather of up to 150 m, stays inside the climb. The start walks
/// back up to 150 m while the grade is still at least 0.5%, so the ramp is
/// included and the flat before it is not. The finish is the crest. If the
/// whole span averages under 3%, metres are dropped from the weaker end until
/// it qualifies or it gets shorter than 400 m.
///
/// The longest climb is named after the course (`Penguin Pass climb`). Any
/// other climb is `Climb at N m`.
///
/// The sprint is the flattest stretch between 200 and 300 m that does not
/// overlap a climb. Flatness is the mean of the absolute grade. Ties go to
/// the longer stretch, then the one that starts earlier. A course with no
/// 3% climb, such as a flat preset, is a sprint and the lap.
#[must_use]
pub fn detect(course: &Course) -> Vec<Segment> {
    let lap = course.length_m();
    if !lap.is_finite() || lap < 1.0 {
        return Vec::new();
    }
    let mut segments = vec![Segment {
        id: "lap".to_owned(),
        name: "Lap".to_owned(),
        kind: SegmentKind::Lap,
        start_m: 0.0,
        end_m: lap,
    }];
    let profile = sample_grade(course);
    let climbs = find_climbs(course, &profile, lap);
    let named = name_climbs(course.name(), &climbs, lap);
    let sprint_blocked: Vec<(f64, f64)> =
        named.iter().map(|seg| (seg.start_m, seg.end_m)).collect();
    segments.extend(named);
    if let Some(sprint) = find_sprint(&profile, &sprint_blocked, lap) {
        segments.push(sprint);
    }
    segments
}

fn sample_grade(course: &Course) -> Vec<f64> {
    let n_f = course.length_m().floor();
    if !n_f.is_finite() || n_f < 1.0 {
        return Vec::new();
    }
    let n = if n_f > MAX_SAMPLES as f64 {
        MAX_SAMPLES
    } else {
        n_f as usize
    };
    let mut grade = Vec::with_capacity(n);
    for i in 0..n {
        grade.push(f64::from(course.grade_pct_at(i as f64)));
    }
    grade
}

fn grade_at(grade: &[f64], index: isize) -> f64 {
    let n = grade.len() as isize;
    if n <= 0 {
        return 0.0;
    }
    grade[index.rem_euclid(n) as usize]
}

#[derive(Clone, Copy)]
struct RawClimb {
    start_m: f64,
    end_m: f64,
}

fn find_climbs(course: &Course, grade: &[f64], lap: f64) -> Vec<RawClimb> {
    let n = grade.len() as isize;
    if n < MIN_CLIMB_M {
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut index = 0isize;
    while index < n {
        if grade_at(grade, index) < CLIMB_GRADE_PCT {
            index += 1;
            continue;
        }
        let body = index;
        let mut start = body;
        let mut back = 0isize;
        while back < RAMP_M {
            let prev = start - 1;
            if grade_at(grade, prev) < RAMP_GRADE_PCT {
                break;
            }
            if found
                .last()
                .is_some_and(|climb: &RawClimb| unwrapped_contains(climb, prev, lap))
            {
                break;
            }
            start = prev;
            back += 1;
        }
        let (last, next_index) = scan_forward(grade, body, start, n);
        if let Some(climb) = trim_climb(course, grade, start, last, lap) {
            found.push(climb);
        }
        index = next_index.max(body + 1);
        if index >= n {
            break;
        }
    }
    dedup_climbs(found, lap)
}

fn unwrapped_contains(climb: &RawClimb, index: isize, lap: f64) -> bool {
    let point = (index as f64).rem_euclid(lap);
    contains_point(climb.start_m, climb.end_m, point)
}

fn contains_point(start: f64, end: f64, point: f64) -> bool {
    if end > start {
        point >= start && point < end
    } else if end < start {
        point >= start || point < end
    } else {
        false
    }
}

/// Returns the last included index and where the scan should resume.
fn scan_forward(grade: &[f64], body: isize, start: isize, n: isize) -> (isize, isize) {
    let mut last = body;
    let mut cursor = body;
    let mut dip = 0isize;
    let mut soft = 0isize;
    let limit = start + n;
    while cursor + 1 < limit {
        cursor += 1;
        let sample = grade_at(grade, cursor);
        if sample >= CLIMB_GRADE_PCT {
            dip = 0;
            soft = 0;
            last = cursor;
        } else if sample >= 0.0 {
            soft += 1;
            dip = 0;
            if soft > MAX_BREATHER_M {
                break;
            }
            last = cursor;
        } else {
            dip += 1;
            soft += 1;
            if dip > MAX_DIP_M {
                break;
            }
            last = cursor;
        }
    }
    (last, last + 1)
}

fn trim_climb(
    course: &Course,
    grade: &[f64],
    start: isize,
    last: isize,
    lap: f64,
) -> Option<RawClimb> {
    if last < start {
        return None;
    }
    let crest = crest_index(course, start, last);
    let valley = valley_index(course, start, crest);
    let mut span_start = valley;
    let mut span_end = crest;
    if span_end <= span_start {
        return None;
    }
    let (sum, count) = shrink_to_grade(grade, &mut span_start, &mut span_end);
    if count < MIN_CLIMB_M || sum / (count as f64) < MIN_AVG_GRADE_PCT {
        return None;
    }
    let gain = f64::from(course.elevation_at(span_end as f64))
        - f64::from(course.elevation_at(span_start as f64));
    if gain < MIN_GAIN_M {
        return None;
    }
    let (start_m, end_m) = normalize(span_start, span_end, lap)?;
    Some(RawClimb { start_m, end_m })
}

fn crest_index(course: &Course, start: isize, last: isize) -> isize {
    let mut best = start;
    let mut best_elev = f64::from(course.elevation_at(start as f64));
    let mut index = start;
    while index <= last {
        let elev = f64::from(course.elevation_at(index as f64));
        if elev >= best_elev {
            best_elev = elev;
            best = index;
        }
        index += 1;
    }
    best
}

fn valley_index(course: &Course, start: isize, crest: isize) -> isize {
    let search_end = crest.min(start + RAMP_M);
    let mut best = start;
    let mut best_elev = f64::from(course.elevation_at(start as f64));
    let mut index = start;
    while index <= search_end {
        let elev = f64::from(course.elevation_at(index as f64));
        if elev < best_elev {
            best_elev = elev;
            best = index;
        }
        index += 1;
    }
    best
}

fn shrink_to_grade(grade: &[f64], start: &mut isize, end: &mut isize) -> (f64, isize) {
    let mut sum = 0.0;
    let mut index = *start;
    while index < *end {
        sum += grade_at(grade, index);
        index += 1;
    }
    let mut count = *end - *start;
    while count >= MIN_CLIMB_M && sum / (count as f64) < MIN_AVG_GRADE_PCT {
        let left = grade_at(grade, *start);
        let right = grade_at(grade, *end - 1);
        if left <= right {
            sum -= left;
            *start += 1;
        } else {
            sum -= right;
            *end -= 1;
        }
        count -= 1;
    }
    (sum, count)
}

fn normalize(start: isize, end: isize, lap: f64) -> Option<(f64, f64)> {
    let len = (end - start) as f64;
    if len < MIN_CLIMB_M as f64 || len >= lap - 1.0 || lap <= 0.0 {
        return None;
    }
    let start_m = (start as f64).rem_euclid(lap);
    let end_unwrapped = start_m + len;
    if end_unwrapped <= lap {
        Some((start_m, end_unwrapped))
    } else if end_unwrapped - lap < start_m {
        Some((start_m, end_unwrapped - lap))
    } else {
        None
    }
}

fn dedup_climbs(climbs: Vec<RawClimb>, lap: f64) -> Vec<RawClimb> {
    let mut ordered = climbs;
    ordered.sort_by(|a, b| a.start_m.total_cmp(&b.start_m));
    let mut kept: Vec<RawClimb> = Vec::new();
    for climb in ordered {
        if let Some(prev) = kept.last()
            && ranges_overlap(prev.start_m, prev.end_m, climb.start_m, climb.end_m, lap)
        {
            let prev_len = span_len(prev.start_m, prev.end_m, lap);
            let len = span_len(climb.start_m, climb.end_m, lap);
            if len > prev_len {
                kept.pop();
                kept.push(climb);
            }
            continue;
        }
        kept.push(climb);
    }
    kept
}

fn name_climbs(course_name: &str, climbs: &[RawClimb], lap: f64) -> Vec<Segment> {
    let Some(best) = climbs.iter().max_by(|a, b| {
        let len_a = span_len(a.start_m, a.end_m, lap);
        let len_b = span_len(b.start_m, b.end_m, lap);
        len_a
            .total_cmp(&len_b)
            .then_with(|| b.start_m.total_cmp(&a.start_m))
    }) else {
        return Vec::new();
    };
    let best_start = best.start_m;
    let mut used = BTreeSet::new();
    let mut named = Vec::with_capacity(climbs.len());
    for climb in climbs {
        let metres = round_metre(climb.start_m);
        let base = format!("climb-{metres}");
        let mut id = base.clone();
        let mut suffix = 2u32;
        while !used.insert(id.clone()) {
            id = format!("{base}-{suffix}");
            suffix += 1;
        }
        let name = if (climb.start_m - best_start).abs() <= 1.0e-6 {
            format!("{course_name} climb")
        } else {
            format!("Climb at {metres} m")
        };
        named.push(Segment {
            id,
            name,
            kind: SegmentKind::Climb,
            start_m: climb.start_m,
            end_m: climb.end_m,
        });
    }
    named
}

fn round_metre(distance_m: f64) -> i64 {
    if !distance_m.is_finite() || distance_m <= 0.0 {
        return 0;
    }
    let rounded = distance_m.round();
    if rounded >= i64::MAX as f64 {
        return i64::MAX;
    }
    rounded as i64
}

struct SprintCand {
    score: f64,
    len: f64,
    start: f64,
    end: f64,
}

impl SprintCand {
    fn is_better(&self, other: &Self) -> bool {
        if self.score < other.score - 1.0e-6 {
            return true;
        }
        if (self.score - other.score).abs() > 1.0e-6 {
            return false;
        }
        if self.len > other.len + 1.0e-9 {
            return true;
        }
        (self.len - other.len).abs() <= 1.0e-9 && self.start < other.start
    }
}

fn find_sprint(grade: &[f64], climbs: &[(f64, f64)], lap: f64) -> Option<Segment> {
    let n = grade.len();
    if n < SPRINT_MIN_M || lap <= 0.0 {
        return None;
    }
    let mut prefix = vec![0.0; 2 * n + 1];
    for i in 0..2 * n {
        prefix[i + 1] = prefix[i] + grade[i % n].abs();
    }
    let max_len = SPRINT_MAX_M.min(n);
    let mut best: Option<SprintCand> = None;
    for len in SPRINT_MIN_M..=max_len {
        for start in 0..n {
            let start_m = start as f64;
            let end_unwrapped = start_m + len as f64;
            let end_m = if end_unwrapped <= lap {
                end_unwrapped
            } else {
                end_unwrapped - lap
            };
            let blocked = climbs.iter().any(|(climb_start, climb_end)| {
                ranges_overlap(start_m, end_m, *climb_start, *climb_end, lap)
            });
            if blocked {
                continue;
            }
            let sum = prefix[start + len] - prefix[start];
            let cand = SprintCand {
                score: sum / len as f64,
                len: len as f64,
                start: start_m,
                end: end_m,
            };
            let replace = match &best {
                None => true,
                Some(current) => cand.is_better(current),
            };
            if replace {
                best = Some(cand);
            }
        }
    }
    best.map(|cand| Segment {
        id: "sprint".to_owned(),
        name: "Sprint".to_owned(),
        kind: SegmentKind::Sprint,
        start_m: cand.start,
        end_m: cand.end,
    })
}

fn span_len(start: f64, end: f64, lap: f64) -> f64 {
    if !start.is_finite() || !end.is_finite() || !lap.is_finite() || lap <= 0.0 {
        return 0.0;
    }
    if end > start {
        end - start
    } else {
        (end + lap - start).max(0.0)
    }
}

fn ranges_overlap(a0: f64, a1: f64, b0: f64, b1: f64, lap: f64) -> bool {
    let mut a_parts = [(0.0, 0.0); 2];
    let mut b_parts = [(0.0, 0.0); 2];
    let a_n = pieces(a0, a1, lap, &mut a_parts);
    let b_n = pieces(b0, b1, lap, &mut b_parts);
    for a in a_parts.iter().take(a_n) {
        for b in b_parts.iter().take(b_n) {
            if a.0 < b.1 && b.0 < a.1 {
                return true;
            }
        }
    }
    false
}

fn pieces(start: f64, end: f64, lap: f64, out: &mut [(f64, f64); 2]) -> usize {
    if end > start {
        out[0] = (start, end.min(lap));
        1
    } else if end < start && lap > start {
        out[0] = (start, lap);
        out[1] = (0.0, end.max(0.0));
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use pp_course::Course;
    use pp_routegen::{RouteSpec, Terrain, TerrainPreset, generate};

    use super::*;

    fn assert_invariants(course: &Course, segments: &[Segment]) {
        let lap = course.length_m();
        let laps: Vec<_> = segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Lap)
            .collect();
        assert_eq!(laps.len(), 1, "{}", course.name());
        assert_eq!(laps[0].id, "lap");
        assert!((laps[0].start_m).abs() < 1e-9);
        assert!((laps[0].end_m - lap).abs() < 1e-6);
        let sprints: Vec<_> = segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Sprint)
            .collect();
        assert!(sprints.len() <= 1, "{}", course.name());
        for sprint in &sprints {
            let len = sprint.length_m(lap);
            assert!(
                (200.0..=300.0).contains(&len),
                "{} sprint {len}",
                course.name()
            );
            assert_eq!(sprint.id, "sprint");
        }
        for climb in segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Climb)
        {
            let len = climb.length_m(lap);
            let grade = climb.average_grade_pct(course);
            assert!(len >= 400.0 - 1e-6, "{} {climb:?}", course.name());
            assert!(
                grade >= MIN_AVG_GRADE_PCT - 1e-6,
                "{} {} grade {grade}",
                course.name(),
                climb.name
            );
            assert!(climb.id.starts_with("climb-"), "{}", climb.id);
        }
        let named = segments
            .iter()
            .filter(|segment| {
                segment.kind == SegmentKind::Climb && segment.name.ends_with(" climb")
            })
            .count();
        let climb_count = segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Climb)
            .count();
        if climb_count == 0 {
            assert_eq!(named, 0);
        } else {
            assert_eq!(named, 1, "{}", course.name());
        }
        for (i, a) in segments.iter().enumerate() {
            if a.kind == SegmentKind::Lap {
                continue;
            }
            for b in segments.iter().skip(i + 1) {
                if b.kind == SegmentKind::Lap {
                    continue;
                }
                assert!(
                    !ranges_overlap(a.start_m, a.end_m, b.start_m, b.end_m, lap),
                    "{} overlap {a:?} {b:?}",
                    course.name()
                );
            }
        }
    }

    fn report(course: &Course, segments: &[Segment]) -> String {
        let mut lines = Vec::new();
        for segment in segments {
            let len = segment.length_m(course.length_m());
            let grade = segment.average_grade_pct(course);
            lines.push(format!(
                "{:16} {:7} {:8.1}–{:8.1}  len {:7.1}  grade {:+6.2}%  {}",
                course.name(),
                match segment.kind {
                    SegmentKind::Lap => "lap",
                    SegmentKind::Climb => "climb",
                    SegmentKind::Sprint => "sprint",
                },
                segment.start_m,
                segment.end_m,
                len,
                grade,
                segment.name
            ));
        }
        lines.join("\n")
    }

    #[test]
    fn penguin_pass_has_one_named_climb_and_a_sprint() {
        let course = Course::penguin_pass();
        let segments = detect(&course);
        assert_eq!(detect(&course), segments);
        assert_invariants(&course, &segments);
        let climbs: Vec<_> = segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Climb)
            .collect();
        assert_eq!(climbs.len(), 1, "{}", report(&course, &segments));
        assert_eq!(climbs[0].name, "Penguin Pass climb");
        assert!((climbs[0].start_m - 273.0).abs() < 1.0);
        assert!((climbs[0].end_m - 2619.0).abs() < 1.0);
        assert!((climbs[0].length_m(course.length_m()) - 2346.0).abs() < 1.0);
        assert!(climbs[0].average_grade_pct(&course) > 6.5);
        let sprint = segments
            .iter()
            .find(|segment| segment.kind == SegmentKind::Sprint)
            .expect("sprint");
        assert!((sprint.start_m - 4754.0).abs() < 1.0);
        assert!((sprint.end_m - 54.0).abs() < 1.0);
        assert!((sprint.length_m(course.length_m()) - 300.0).abs() < 1.0);
        assert!(sprint.average_grade_pct(&course).abs() < 0.05);
    }

    #[test]
    fn generated_presets_follow_the_rules() {
        let presets = [
            ("Flat 2 km", 1u64, 2_000.0, TerrainPreset::Flat),
            ("Flat 10 km", 1, 10_000.0, TerrainPreset::Flat),
            ("Rolling 5 km", 3, 5_000.0, TerrainPreset::Rolling),
            ("Rolling 10 km", 3, 10_000.0, TerrainPreset::Rolling),
            ("Mountain 10 km", 11, 10_000.0, TerrainPreset::Mountain),
            ("Desert 10 km", 7, 10_000.0, TerrainPreset::Desert),
        ];
        let mut text = String::new();
        for (name, seed, distance_m, terrain) in presets {
            let spec = RouteSpec::new(name, seed, distance_m, Terrain::Preset(terrain));
            let route = generate(&spec).expect(name);
            let course = route.course();
            let segments = detect(course);
            assert_eq!(detect(course), segments);
            assert_invariants(course, &segments);
            if terrain == TerrainPreset::Flat {
                assert!(
                    segments
                        .iter()
                        .all(|segment| segment.kind != SegmentKind::Climb),
                    "{name} should not climb\n{}",
                    report(course, &segments)
                );
            }
            let climbs = segments
                .iter()
                .filter(|segment| segment.kind == SegmentKind::Climb)
                .count();
            let expected = match name {
                "Flat 2 km" | "Flat 10 km" => 0,
                "Rolling 5 km" => 4,
                "Rolling 10 km" => 8,
                "Mountain 10 km" | "Desert 10 km" => 1,
                _ => climbs,
            };
            assert_eq!(climbs, expected, "{name}\n{}", report(course, &segments));
            text.push_str(&report(course, &segments));
            text.push('\n');
        }
        assert!(text.contains("Sprint"), "{text}");
    }

    #[test]
    fn a_short_dip_stays_one_climb_and_a_long_gap_splits() {
        let dipped = circle_course("Dip", &with_return(dip_spans(), -6.0));
        let dipped_segments = detect(&dipped);
        assert_invariants(&dipped, &dipped_segments);
        let dips = dipped_segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Climb)
            .count();
        assert_eq!(dips, 1, "{}", report(&dipped, &dipped_segments));
        assert!(
            dipped_segments
                .iter()
                .any(|segment| segment.kind == SegmentKind::Climb
                    && segment.length_m(dipped.length_m()) > 900.0)
        );

        let split = circle_course("Split", &with_return(split_spans(), -5.0));
        let split_segments = detect(&split);
        assert_invariants(&split, &split_segments);
        let climbs: Vec<_> = split_segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Climb)
            .collect();
        assert_eq!(climbs.len(), 2, "{}", report(&split, &split_segments));
        assert!(
            climbs
                .iter()
                .all(|climb| climb.average_grade_pct(&split) >= 3.0)
        );
    }

    #[test]
    fn a_flat_circle_is_a_sprint_and_the_lap() {
        let course = circle_course(
            "Flat circle",
            &[Span {
                smooth: false,
                len: 2_000.0,
                grade: 0.0,
            }],
        );
        let segments = detect(&course);
        assert_invariants(&course, &segments);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].kind, SegmentKind::Sprint);
        assert!((segments[1].length_m(course.length_m()) - 300.0).abs() < 1e-6);
        assert!(segments[1].average_grade_pct(&course).abs() < 1e-6);
    }

    struct Span {
        smooth: bool,
        len: f64,
        grade: f64,
    }

    fn hold(len: f64, grade: f64) -> Span {
        Span {
            smooth: false,
            len,
            grade,
        }
    }

    fn smooth(len: f64, grade: f64) -> Span {
        Span {
            smooth: true,
            len,
            grade,
        }
    }

    fn dip_spans() -> Vec<Span> {
        vec![
            hold(300.0, 0.0),
            smooth(40.0, 6.0),
            hold(450.0, 6.0),
            smooth(40.0, -1.0),
            hold(50.0, -1.0),
            smooth(40.0, 6.0),
            hold(450.0, 6.0),
            smooth(40.0, 0.0),
        ]
    }

    fn split_spans() -> Vec<Span> {
        vec![
            hold(250.0, 0.0),
            smooth(40.0, 6.0),
            hold(500.0, 6.0),
            smooth(40.0, 0.0),
            smooth(40.0, -4.0),
            hold(200.0, -4.0),
            smooth(40.0, 0.0),
            smooth(40.0, 6.0),
            hold(500.0, 6.0),
            smooth(40.0, 0.0),
        ]
    }

    fn net_gain(spans: &[Span]) -> f64 {
        let mut grade = 0.0;
        let mut gain = 0.0;
        for span in spans {
            if span.smooth {
                gain += span.len / 100.0 * (grade + span.grade) / 2.0;
                grade = span.grade;
            } else {
                gain += span.grade / 100.0 * span.len;
                grade = span.grade;
            }
        }
        gain
    }

    fn with_return(mut spans: Vec<Span>, descent_pct: f64) -> Vec<Span> {
        let smooth_len = 40.0;
        let into = smooth_len / 100.0 * descent_pct / 2.0;
        let out = smooth_len / 100.0 * descent_pct / 2.0;
        let so_far = net_gain(&spans) + into + out;
        let hold_len = -so_far / (descent_pct / 100.0);
        assert!(hold_len > 1.0, "descent {hold_len}");
        spans.push(smooth(smooth_len, descent_pct));
        spans.push(hold(hold_len, descent_pct));
        spans.push(smooth(smooth_len, 0.0));
        spans
    }

    fn circle_course(name: &str, spans: &[Span]) -> Course {
        let horizontal: f64 = spans.iter().map(|span| span.len).sum();
        let radius = horizontal / std::f64::consts::TAU;
        let mut grade_json = String::new();
        for (index, span) in spans.iter().enumerate() {
            if index > 0 {
                grade_json.push(',');
            }
            if span.smooth {
                grade_json.push_str(&format!(
                    r#"{{"type":"smooth","length_m":{:.10},"to_grade_pct":{:.10}}}"#,
                    span.len, span.grade
                ));
            } else {
                grade_json.push_str(&format!(
                    r#"{{"type":"hold","length_m":{:.10},"grade_pct":{:.10}}}"#,
                    span.len, span.grade
                ));
            }
        }
        let json = format!(
            r#"{{"name":{name:?},"start":{{"x":0.0,"z":0.0,"heading_rad":0.0}},"segments":[{{"type":"arc","radius_m":{radius:.10},"angle_rad":{angle:.10}}}],"grade":[{grade_json}]}}"#,
            name = name,
            radius = radius,
            angle = std::f64::consts::TAU,
        );
        Course::from_str(&json).unwrap_or_else(|err| panic!("build {name}: {err}\n{json}"))
    }
}
