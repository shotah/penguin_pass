//! Times a ride against the segments on the loaded course.

use std::collections::BTreeMap;

use crate::feed::{self, LINE_EPS_M, Sample, Step};
use crate::records::today;
use crate::segment::{Segment, SegmentKind};

/// A best effort the timer compares against, and writes back when beaten.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentEffort {
    /// [`Segment::id`](crate::Segment::id).
    pub segment_id: String,
    /// Finish time, in seconds.
    pub time_s: f64,
    /// Time taken to reach each tenth of the segment, from 10% through the finish.
    pub tenths_s: [f64; 10],
    /// UTC day the effort was set, `YYYY-MM-DD`.
    pub date: String,
}

impl SegmentEffort {
    /// Time the best effort had taken at `fraction` of the segment, in seconds.
    ///
    /// `fraction` is 0 at the start line and 1 at the finish. Between stored
    /// tenths the time is linear. At 0 the time is 0.
    #[must_use]
    pub fn time_at(&self, fraction: f64) -> f64 {
        if !fraction.is_finite() || fraction <= 0.0 {
            return 0.0;
        }
        let fraction = fraction.min(1.0);
        let upper = (fraction * 10.0).ceil() as usize;
        let upper = upper.clamp(1, 10);
        let lower = upper - 1;
        let lower_t = if lower == 0 {
            0.0
        } else {
            self.tenths_s[lower - 1]
        };
        let upper_t = self.tenths_s[upper - 1];
        let lower_f = lower as f64 / 10.0;
        let upper_f = upper as f64 / 10.0;
        let span = upper_f - lower_f;
        if span <= 0.0 {
            return upper_t;
        }
        let weight = (fraction - lower_f) / span;
        lower_t + (upper_t - lower_t) * weight
    }
}

/// Something that happened on this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum SegmentEvent {
    /// The rider crossed this segment's start line going forward.
    Entered {
        /// [`Segment::id`](crate::Segment::id).
        segment_id: String,
    },
    /// The rider reached the finish line of a segment they had entered.
    Finished {
        /// [`Segment::id`](crate::Segment::id).
        segment_id: String,
        /// Finish time, in seconds. Includes any stop inside the segment.
        time_s: f64,
        /// The best time before this finish. `None` on the first effort.
        previous_best_s: Option<f64>,
        /// `true` when this finish is strictly faster than the stored best.
        record: bool,
    },
}

/// Where the rider is on a segment that is still open.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentProgress {
    /// [`Segment::id`](crate::Segment::id).
    pub segment_id: String,
    /// Seconds since the start line.
    pub elapsed_s: f64,
    /// 0 at the start line, 1 at the finish line.
    pub fraction: f64,
    /// Seconds ahead of the best effort at this fraction.
    ///
    /// Positive when this attempt has taken less time than the best did to
    /// reach the same point. `None` when the segment has no best yet.
    pub split_s: Option<f64>,
}

#[derive(Clone, Copy)]
struct Mark {
    distance_m: f64,
    index: usize,
    exit: bool,
}

#[derive(Debug)]
struct Attempt {
    index: usize,
    entry_s: f64,
    entry_m: f64,
    exit_m: f64,
    tenths: [f64; 10],
    filled: u8,
}

/// Times every segment from the ride clock.
///
/// Feed [`Telemetry::elapsed_s`](pp_core::Telemetry::elapsed_s) and
/// [`Telemetry::distance_m`](pp_core::Telemetry::distance_m): total distance,
/// which keeps counting across laps. Lap distance that resets to zero looks
/// like riding backwards and is ignored.
///
/// A segment counts only when the rider crosses its start line going forward.
/// Beginning the ride already inside one does not produce a time. Pass a
/// sample on the start line (usually `(0.0, 0.0)`) when the ride begins
/// there. Entry and exit times are linear between the two samples of a tick,
/// so the same motion gives the same time at 30, 60, and 144 Hz.
///
/// The clock keeps running while the rider is stopped inside a segment.
/// A tick whose distance jumps more than [`MAX_TICK_DISTANCE_M`](crate::MAX_TICK_DISTANCE_M)
/// drops any open segment and does not award a finish. Time or distance
/// going backwards, and non-finite samples, are ignored.
#[derive(Debug)]
pub struct SegmentTimer {
    segments: Vec<Segment>,
    lap_m: f64,
    bests: BTreeMap<String, SegmentEffort>,
    active: Vec<Attempt>,
    anchor: Option<Sample>,
    progress: Vec<SegmentProgress>,
}

impl SegmentTimer {
    /// `efforts` are the stored bests for this course. Unknown ids are ignored.
    ///
    /// When two efforts share an id, the faster one is kept.
    #[must_use]
    pub fn new(segments: Vec<Segment>, efforts: &[SegmentEffort]) -> Self {
        let lap_m = segments
            .iter()
            .find(|segment| segment.kind == SegmentKind::Lap)
            .map(|segment| {
                if segment.end_m >= segment.start_m {
                    segment.end_m - segment.start_m
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);
        let mut bests: BTreeMap<String, SegmentEffort> = BTreeMap::new();
        if lap_m > 0.0 {
            for effort in efforts {
                if !effort_ok(effort)
                    || segments
                        .iter()
                        .all(|segment| segment.id != effort.segment_id)
                {
                    continue;
                }
                let replace = match bests.get(&effort.segment_id) {
                    Some(existing) => effort.time_s < existing.time_s,
                    None => true,
                };
                if replace {
                    bests.insert(effort.segment_id.clone(), effort.clone());
                }
            }
        }
        Self {
            segments,
            lap_m,
            bests,
            active: Vec::new(),
            anchor: None,
            progress: Vec::new(),
        }
    }

    /// Advance to this sample. Events are in distance order. At a line that
    /// is both a finish and the next start, the finish is first.
    #[must_use]
    pub fn tick(&mut self, elapsed_s: f64, total_distance_m: f64) -> Vec<SegmentEvent> {
        if !(self.lap_m.is_finite() && self.lap_m > 0.0) {
            return Vec::new();
        }
        match feed::classify(self.anchor, elapsed_s, total_distance_m) {
            Step::Ignore => Vec::new(),
            Step::Break(sample) => {
                self.active.clear();
                self.anchor = Some(sample);
                self.rebuild_progress();
                Vec::new()
            }
            Step::Start(sample) => {
                self.anchor = Some(sample);
                let events = self.enter_on_line(sample);
                self.rebuild_progress();
                events
            }
            Step::Advance { prev, curr } => {
                let events = self.advance(prev, curr);
                self.anchor = Some(curr);
                self.rebuild_progress();
                events
            }
        }
    }

    /// Segments that have been entered and not yet finished, lap first.
    #[must_use]
    pub fn in_progress(&self) -> &[SegmentProgress] {
        &self.progress
    }

    /// Current bests, including records set during this ride. Sorted by id.
    #[must_use]
    pub fn efforts(&self) -> Vec<SegmentEffort> {
        self.bests.values().cloned().collect()
    }

    fn enter_on_line(&mut self, sample: Sample) -> Vec<SegmentEvent> {
        let mut events = Vec::new();
        for index in 0..self.segments.len() {
            let start_m = self.segments[index].start_m;
            let Some(line) = on_start_line(sample.distance_m, start_m, self.lap_m) else {
                continue;
            };
            self.begin(index, sample.elapsed_s, line, &mut events);
        }
        events
    }

    fn advance(&mut self, prev: Sample, curr: Sample) -> Vec<SegmentEvent> {
        let mut marks = Vec::new();
        for index in 0..self.segments.len() {
            let start_m = self.segments[index].start_m;
            let Some(entry) = crossed(prev.distance_m, curr.distance_m, start_m, self.lap_m) else {
                continue;
            };
            marks.push(Mark {
                distance_m: entry,
                index,
                exit: false,
            });
            let len = self.length_of(index);
            let exit_at = entry + len;
            if exit_at > prev.distance_m && exit_at <= curr.distance_m + 1.0e-6 {
                marks.push(Mark {
                    distance_m: exit_at.min(curr.distance_m),
                    index,
                    exit: true,
                });
            }
        }
        for attempt in &self.active {
            if attempt.exit_m > prev.distance_m && attempt.exit_m <= curr.distance_m + 1.0e-6 {
                let already = marks.iter().any(|mark| {
                    mark.exit
                        && mark.index == attempt.index
                        && (mark.distance_m - attempt.exit_m).abs() < 1.0e-3
                });
                if !already {
                    marks.push(Mark {
                        distance_m: attempt.exit_m.min(curr.distance_m),
                        index: attempt.index,
                        exit: true,
                    });
                }
            }
        }
        marks.sort_by(|a, b| {
            a.distance_m.total_cmp(&b.distance_m).then_with(|| {
                u8::from(!a.exit)
                    .cmp(&u8::from(!b.exit))
                    .then(a.index.cmp(&b.index))
            })
        });
        let mut events = Vec::new();
        for mark in marks {
            if mark.exit {
                self.finish(mark.index, mark.distance_m, prev, curr, &mut events);
            } else {
                self.begin(
                    mark.index,
                    feed::lerp_time(prev, curr, mark.distance_m),
                    mark.distance_m,
                    &mut events,
                );
            }
        }
        for attempt in &mut self.active {
            absorb(attempt, prev, curr, curr.distance_m);
        }
        events
    }

    fn begin(&mut self, index: usize, entry_s: f64, entry_m: f64, events: &mut Vec<SegmentEvent>) {
        if self.active.iter().any(|attempt| attempt.index == index) {
            return;
        }
        let len = self.length_of(index);
        if len <= 0.0 {
            return;
        }
        let Some(segment) = self.segments.get(index) else {
            return;
        };
        events.push(SegmentEvent::Entered {
            segment_id: segment.id.clone(),
        });
        self.active.push(Attempt {
            index,
            entry_s,
            entry_m,
            exit_m: entry_m + len,
            tenths: [0.0; 10],
            filled: 0,
        });
    }

    fn finish(
        &mut self,
        index: usize,
        exit_m: f64,
        prev: Sample,
        curr: Sample,
        events: &mut Vec<SegmentEvent>,
    ) {
        let Some(pos) = self
            .active
            .iter()
            .position(|attempt| attempt.index == index)
        else {
            return;
        };
        let mut attempt = self.active.remove(pos);
        absorb(&mut attempt, prev, curr, exit_m);
        let time_s = (feed::lerp_time(prev, curr, exit_m) - attempt.entry_s).max(0.0);
        if !time_s.is_finite() {
            return;
        }
        complete_tenths(&mut attempt, time_s);
        let Some(segment) = self.segments.get(index) else {
            return;
        };
        let id = segment.id.clone();
        let previous_best_s = self.bests.get(&id).map(|effort| effort.time_s);
        let record = previous_best_s.is_none_or(|best| time_s < best);
        if record {
            self.bests.insert(
                id.clone(),
                SegmentEffort {
                    segment_id: id.clone(),
                    time_s,
                    tenths_s: attempt.tenths,
                    date: today(),
                },
            );
        }
        events.push(SegmentEvent::Finished {
            segment_id: id,
            time_s,
            previous_best_s,
            record,
        });
    }

    fn length_of(&self, index: usize) -> f64 {
        let Some(segment) = self.segments.get(index) else {
            return 0.0;
        };
        if segment.kind == SegmentKind::Lap {
            return self.lap_m;
        }
        if segment.end_m > segment.start_m {
            segment.end_m - segment.start_m
        } else {
            segment.end_m + self.lap_m - segment.start_m
        }
    }

    fn rebuild_progress(&mut self) {
        let Some(sample) = self.anchor else {
            self.progress.clear();
            return;
        };
        let mut rows = Vec::with_capacity(self.active.len());
        for attempt in &self.active {
            let Some(segment) = self.segments.get(attempt.index) else {
                continue;
            };
            let len = (attempt.exit_m - attempt.entry_m).max(1.0e-9);
            let fraction = ((sample.distance_m - attempt.entry_m) / len).clamp(0.0, 1.0);
            let elapsed_s = (sample.elapsed_s - attempt.entry_s).max(0.0);
            let split_s = self
                .bests
                .get(&segment.id)
                .map(|effort| effort.time_at(fraction) - elapsed_s);
            rows.push((
                attempt.index,
                SegmentProgress {
                    segment_id: segment.id.clone(),
                    elapsed_s,
                    fraction,
                    split_s,
                },
            ));
        }
        rows.sort_by_key(|(index, _)| *index);
        self.progress = rows.into_iter().map(|(_, row)| row).collect();
    }
}

fn on_start_line(distance_m: f64, start_m: f64, lap_m: f64) -> Option<f64> {
    if lap_m <= 0.0 {
        return None;
    }
    let phase = (distance_m - start_m).rem_euclid(lap_m);
    if phase <= LINE_EPS_M {
        Some(distance_m - phase)
    } else {
        None
    }
}

fn crossed(prev: f64, curr: f64, origin: f64, period: f64) -> Option<f64> {
    if period <= 0.0 {
        return None;
    }
    let k = ((prev - origin) / period).floor() + 1.0;
    let x = origin + k * period;
    if x > prev && x <= curr + 1.0e-6 {
        Some(x.min(curr))
    } else {
        None
    }
}

fn absorb(attempt: &mut Attempt, prev: Sample, curr: Sample, limit_m: f64) {
    let len = attempt.exit_m - attempt.entry_m;
    if len <= 0.0 {
        return;
    }
    while attempt.filled < 10 {
        let k = u32::from(attempt.filled) + 1;
        let mark = attempt.entry_m + len * f64::from(k) / 10.0;
        if mark > limit_m + 1.0e-6 || mark > curr.distance_m + 1.0e-6 {
            break;
        }
        let t = (feed::lerp_time(prev, curr, mark) - attempt.entry_s).max(0.0);
        if let Some(slot) = attempt.tenths.get_mut(usize::from(attempt.filled)) {
            *slot = t;
        }
        attempt.filled += 1;
    }
}

fn complete_tenths(attempt: &mut Attempt, time_s: f64) {
    let filled = usize::from(attempt.filled);
    for k in filled..10 {
        attempt.tenths[k] = time_s * (k + 1) as f64 / 10.0;
    }
    attempt.tenths[9] = time_s;
    attempt.filled = 10;
}

fn effort_ok(effort: &SegmentEffort) -> bool {
    !effort.segment_id.is_empty()
        && effort.time_s.is_finite()
        && effort.time_s > 0.0
        && !effort.date.is_empty()
        && effort.tenths_s.iter().all(|t| t.is_finite() && *t >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course() -> Vec<Segment> {
        vec![
            Segment {
                id: "lap".to_owned(),
                name: "Lap".to_owned(),
                kind: SegmentKind::Lap,
                start_m: 0.0,
                end_m: 2_000.0,
            },
            Segment {
                id: "climb".to_owned(),
                name: "Climb".to_owned(),
                kind: SegmentKind::Climb,
                start_m: 400.0,
                end_m: 1_200.0,
            },
            Segment {
                id: "sprint".to_owned(),
                name: "Sprint".to_owned(),
                kind: SegmentKind::Sprint,
                start_m: 1_400.0,
                end_m: 1_650.0,
            },
            Segment {
                id: "wrap".to_owned(),
                name: "Wrap".to_owned(),
                kind: SegmentKind::Sprint,
                start_m: 1_900.0,
                end_m: 50.0,
            },
        ]
    }

    fn finishes(events: &[SegmentEvent]) -> Vec<(String, f64)> {
        events
            .iter()
            .filter_map(|event| match event {
                SegmentEvent::Finished {
                    segment_id, time_s, ..
                } => Some((segment_id.clone(), *time_s)),
                SegmentEvent::Entered { .. } => None,
            })
            .collect()
    }

    fn ride(hz: f64, speed: f64, goal_m: f64) -> Vec<(String, f64)> {
        let mut timer = SegmentTimer::new(course(), &[]);
        let mut t = 0.0;
        let dt = 1.0 / hz;
        let mut out = Vec::new();
        out.extend(finishes(&timer.tick(0.0, 0.0)));
        while t * speed < goal_m {
            t += dt;
            let d = speed * t;
            out.extend(finishes(&timer.tick(t, d)));
            if d > goal_m {
                break;
            }
        }
        out
    }

    fn time_of(finishes: &[(String, f64)], id: &str, nth: usize) -> f64 {
        finishes
            .iter()
            .filter(|(segment, _)| segment == id)
            .nth(nth)
            .map(|(_, time)| *time)
            .unwrap_or_else(|| panic!("missing {id} #{nth} in {finishes:?}"))
    }

    #[test]
    fn times_match_across_frame_rates() {
        let speed = 9.37;
        let goal = 4_100.0;
        let slow = ride(30.0, speed, goal);
        let mid = ride(60.0, speed, goal);
        let fast = ride(144.0, speed, goal);
        for id in ["lap", "climb", "sprint", "wrap"] {
            for nth in 0..2 {
                let a = time_of(&slow, id, nth);
                let b = time_of(&mid, id, nth);
                let c = time_of(&fast, id, nth);
                assert!((a - b).abs() < 1.0e-3, "{id} #{nth} {a} vs {b}");
                assert!((b - c).abs() < 1.0e-3, "{id} #{nth} {b} vs {c}");
            }
        }
        let climb = 800.0 / speed;
        let sprint = 250.0 / speed;
        let wrap = 150.0 / speed;
        let lap = 2_000.0 / speed;
        assert!((time_of(&fast, "climb", 0) - climb).abs() < 1.0e-6);
        assert!((time_of(&fast, "sprint", 0) - sprint).abs() < 1.0e-6);
        assert!((time_of(&fast, "wrap", 0) - wrap).abs() < 1.0e-6);
        assert!((time_of(&fast, "lap", 0) - lap).abs() < 1.0e-6);
        assert!((time_of(&fast, "lap", 1) - lap).abs() < 1.0e-6);
    }

    fn go(
        timer: &mut SegmentTimer,
        from_t: f64,
        from_d: f64,
        to_t: f64,
        to_d: f64,
    ) -> Vec<SegmentEvent> {
        let mut events = Vec::new();
        let steps = ((to_d - from_d) / 100.0).ceil().max(1.0) as i32;
        for i in 1..=steps {
            let weight = f64::from(i) / f64::from(steps);
            events.extend(timer.tick(
                from_t + (to_t - from_t) * weight,
                from_d + (to_d - from_d) * weight,
            ));
        }
        events
    }

    #[test]
    fn starting_inside_a_segment_does_not_time_it() {
        let mut timer = SegmentTimer::new(course(), &[]);
        // 500 m is inside the climb (400..1200). The lap start was missed too.
        let events = timer.tick(0.0, 500.0);
        assert!(events.is_empty());
        let events = go(&mut timer, 0.0, 500.0, 80.0, 1_300.0);
        assert!(
            finishes(&events).iter().all(|(id, _)| id != "climb"),
            "{events:?}"
        );
        // The next climb starts at 2400 m and finishes at 3200 m.
        let events = go(&mut timer, 80.0, 1_300.0, 280.0, 3_300.0);
        let climb_finishes = finishes(&events)
            .iter()
            .filter(|(id, _)| id == "climb")
            .count();
        assert_eq!(climb_finishes, 1);
    }

    #[test]
    fn a_stop_inside_a_segment_stays_on_the_clock() {
        let mut timer = SegmentTimer::new(course(), &[]);
        let _ = timer.tick(0.0, 0.0);
        let _ = timer.tick(40.0, 400.0);
        let before = timer
            .in_progress()
            .iter()
            .find(|row| row.segment_id == "climb")
            .expect("on the climb")
            .elapsed_s;
        let _ = timer.tick(50.0, 400.0);
        let held = timer
            .in_progress()
            .iter()
            .find(|row| row.segment_id == "climb")
            .expect("still on the climb");
        assert!((held.elapsed_s - before - 10.0).abs() < 1e-9);
        assert!(held.fraction.abs() < 1e-9);
        let events = go(&mut timer, 50.0, 400.0, 120.0, 1_200.0);
        let time = time_of(&finishes(&events), "climb", 0);
        // Entered at t=40 on the line. Stopped until t=50. Finished at t=120.
        assert!((time - 80.0).abs() < 1e-9, "{time}");
    }

    #[test]
    fn backwards_nan_and_a_huge_gap_do_not_invent_a_time() {
        let mut clean = SegmentTimer::new(course(), &[]);
        let _ = clean.tick(0.0, 0.0);
        let expected = time_of(
            &finishes(&go(&mut clean, 0.0, 0.0, 120.0, 1_200.0)),
            "climb",
            0,
        );

        let mut timer = SegmentTimer::new(course(), &[]);
        let _ = timer.tick(0.0, 0.0);
        let _ = timer.tick(20.0, 200.0);
        assert!(timer.tick(10.0, 250.0).is_empty(), "time backwards");
        assert!(timer.tick(25.0, 150.0).is_empty(), "distance backwards");
        assert!(timer.tick(f64::NAN, 300.0).is_empty());
        assert!(timer.tick(30.0, f64::NAN).is_empty());
        let events = go(&mut timer, 20.0, 200.0, 120.0, 1_200.0);
        assert!((time_of(&finishes(&events), "climb", 0) - expected).abs() < 1e-6);

        let mut jumped = SegmentTimer::new(course(), &[]);
        let _ = jumped.tick(0.0, 0.0);
        let _ = jumped.tick(45.0, 450.0);
        assert!(
            jumped
                .in_progress()
                .iter()
                .any(|row| row.segment_id == "climb")
        );
        let events = jumped.tick(50.0, 10_450.0);
        assert!(finishes(&events).is_empty());
        assert!(jumped.in_progress().is_empty());
    }

    #[test]
    fn a_record_is_replaced_only_when_the_effort_is_faster() {
        let mut timer = SegmentTimer::new(course(), &[]);
        let _ = timer.tick(0.0, 0.0);
        let first = go(&mut timer, 0.0, 0.0, 200.0, 2_000.0);
        let lap = first
            .iter()
            .find_map(|event| match event {
                SegmentEvent::Finished {
                    segment_id,
                    time_s,
                    previous_best_s,
                    record,
                } if segment_id == "lap" => Some((*time_s, *previous_best_s, *record)),
                _ => None,
            })
            .expect("lap");
        assert_eq!(lap, (200.0, None, true));
        let slow = go(&mut timer, 200.0, 2_000.0, 500.0, 4_000.0);
        let lap = slow
            .iter()
            .find_map(|event| match event {
                SegmentEvent::Finished {
                    segment_id,
                    time_s,
                    previous_best_s,
                    record,
                } if segment_id == "lap" => Some((*time_s, *previous_best_s, *record)),
                _ => None,
            })
            .expect("second lap");
        assert_eq!(lap.1, Some(200.0));
        assert!(!lap.2);
        assert!((lap.0 - 300.0).abs() < 1e-9);
        let stored = timer
            .efforts()
            .into_iter()
            .find(|effort| effort.segment_id == "lap")
            .expect("stored");
        assert!((stored.time_s - 200.0).abs() < 1e-9);
        assert!(stored.date.len() == 10 && stored.date.as_bytes()[4] == b'-');
        let fast = go(&mut timer, 500.0, 4_000.0, 600.0, 6_000.0);
        let record = fast.iter().any(|event| {
            matches!(
                event,
                SegmentEvent::Finished {
                    segment_id,
                    record: true,
                    previous_best_s: Some(previous),
                    ..
                } if segment_id == "lap" && (*previous - 200.0).abs() < 1e-9
            )
        });
        assert!(record);
        let stored = timer
            .efforts()
            .into_iter()
            .find(|effort| effort.segment_id == "lap")
            .expect("stored");
        assert!((stored.time_s - 100.0).abs() < 1e-9);
        let step = stored.time_s / 10.0;
        for (k, tenth) in stored.tenths_s.iter().enumerate() {
            assert!((tenth - step * (k + 1) as f64).abs() < 1e-6, "{tenth}");
        }
    }

    #[test]
    fn the_live_split_interpolates_between_tenths() {
        let prior = SegmentEffort {
            segment_id: "climb".to_owned(),
            time_s: 100.0,
            tenths_s: [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0],
            date: "2020-01-01".to_owned(),
        };
        assert!((prior.time_at(0.0)).abs() < 1e-12);
        assert!((prior.time_at(0.05) - 5.0).abs() < 1e-12);
        assert!((prior.time_at(0.1) - 10.0).abs() < 1e-12);
        assert!((prior.time_at(0.15) - 15.0).abs() < 1e-12);
        assert!((prior.time_at(1.0) - 100.0).abs() < 1e-12);
        let mut timer = SegmentTimer::new(course(), &[prior]);
        let _ = timer.tick(0.0, 0.0);
        let _ = timer.tick(12.0, 400.0);
        let _ = timer.tick(24.0, 550.0);
        let row = timer
            .in_progress()
            .iter()
            .find(|row| row.segment_id == "climb")
            .expect("climb");
        // Climb is 400..1200. At 550 m the fraction is 150/800.
        assert!((row.fraction - 150.0 / 800.0).abs() < 1e-9);
        assert!((row.elapsed_s - 12.0).abs() < 1e-9);
        let expected = prior_time(150.0 / 800.0) - 12.0;
        let split = row.split_s.expect("split");
        assert!((split - expected).abs() < 1e-9, "{split} vs {expected}");
        assert!(split > 0.0, "ahead of a 100 s effort");
    }

    fn prior_time(fraction: f64) -> f64 {
        SegmentEffort {
            segment_id: "climb".to_owned(),
            time_s: 100.0,
            tenths_s: [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0],
            date: "2020-01-01".to_owned(),
        }
        .time_at(fraction)
    }

    #[test]
    fn entry_and_exit_in_one_tick_use_the_interpolated_clock() {
        let segments = vec![
            Segment {
                id: "lap".to_owned(),
                name: "Lap".to_owned(),
                kind: SegmentKind::Lap,
                start_m: 0.0,
                end_m: 1_000.0,
            },
            Segment {
                id: "climb".to_owned(),
                name: "Climb".to_owned(),
                kind: SegmentKind::Climb,
                start_m: 100.0,
                end_m: 300.0,
            },
        ];
        let mut timer = SegmentTimer::new(segments, &[]);
        let _ = timer.tick(0.0, 0.0);
        let events = timer.tick(40.0, 400.0);
        let time = time_of(&finishes(&events), "climb", 0);
        assert!((time - 20.0).abs() < 1e-9, "{time}");
        assert!(events.iter().any(|event| matches!(
            event,
            SegmentEvent::Entered { segment_id } if segment_id == "climb"
        )));
    }
}
