//! Closed-loop road centerline.
//!
//! A course is a horizontal path of straight segments and circular arcs, plus
//! a grade profile along horizontal distance. Sampling takes 3D arc length.
//! The built-in loop is embedded from `courses/penguin_pass.json`.

use std::fmt;
use std::sync::OnceLock;

use glam::DVec3;
use serde::Deserialize;

/// One sample of the elevation chart used by the HUD and the TV dashboard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfilePoint {
    /// Distance along the 3D centerline from the start line, in meters.
    pub distance_m: f64,
    /// Elevation above the course datum, in meters.
    pub elevation_m: f32,
}

/// Why [`FromStr`](std::str::FromStr) rejected a description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseError {
    message: String,
}

impl CourseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CourseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CourseError {}

/// A closed road centerline.
///
/// Coordinates are meters, right-handed, Y up: index 0 is x (east), index 1
/// is elevation, index 2 is z (north). Heading zero points toward +x, and a
/// positive turn is toward +z. Distance arguments are meters along the 3D
/// centerline. They wrap with Euclidean remainder, so negative distances and
/// distances past the lap length are valid.
///
/// The datum is the start line: elevation is 0 m at distance 0. Grade is
/// rise over horizontal run, in percent, positive uphill in the direction
/// of travel.
///
/// [`Course::reversed`] walks the same loop backwards. Distance 0 is still
/// the start line, because the lap closes. Position and elevation come from
/// the stored centerline at the mirrored arc length. Tangent and grade
/// change sign, so the direction of travel points the other way and uphill
/// becomes downhill. Calling it again restores the forward course, including
/// the name and the lap length.
///
/// Parse a description with [`str::parse`] or [`std::str::FromStr::from_str`].
/// [`CourseError`] is the error type. See `docs/course.md` for the JSON shape.
#[derive(Clone)]
pub struct Course {
    name: String,
    /// Travel runs against the stored centerline when this is set.
    reversed: bool,
    length_m: f64,
    horizontal_m: f64,
    total_ascent_m: f32,
    placed: Vec<Placed>,
    grade: Vec<GradeSpan>,
    /// Horizontal distance at uniform 3D-distance samples.
    ///
    /// `s_h_at_s3d[i]` is the horizontal distance at 3D distance
    /// `i * length_m / (len - 1)`.
    s_h_at_s3d: Vec<f64>,
}

impl fmt::Debug for Course {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Course")
            .field("name", &self.name)
            .field("reversed", &self.reversed)
            .field("length_m", &self.length_m)
            .field("total_ascent_m", &self.total_ascent_m)
            .finish()
    }
}

impl Course {
    /// The built-in Penguin Pass loop.
    ///
    /// The description is embedded in the binary and parsed once. A corrupt
    /// embed is a programming error and panics. [`FromStr`](std::str::FromStr)
    /// is the fallible entry point for any other text.
    #[must_use]
    pub fn penguin_pass() -> Self {
        static COURSE: OnceLock<Course> = OnceLock::new();
        COURSE
            .get_or_init(|| {
                parse_course(include_str!("../courses/penguin_pass.json"))
                    .expect("built-in Penguin Pass course is valid")
            })
            .clone()
    }

    /// The built-in Lonely Road loop.
    #[must_use]
    pub fn lonely_road() -> Self {
        static COURSE: OnceLock<Course> = OnceLock::new();
        COURSE
            .get_or_init(|| {
                parse_course(include_str!("../courses/lonely_road.json"))
                    .expect("built-in Lonely Road course is valid")
            })
            .clone()
    }

    /// Course name, as given in the description.
    ///
    /// Reversing the course does not change the name. Direction is
    /// [`Course::is_reversed`].
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A copy that travels the loop in the opposite direction.
    ///
    /// The stored centerline is unchanged. Queries honor the flipped
    /// direction, and calling this again returns a course that samples like
    /// `self`.
    #[must_use]
    pub fn reversed(&self) -> Self {
        let mut course = self.clone();
        course.reversed = !course.reversed;
        course
    }

    /// Whether [`Course::reversed`] has been applied an odd number of times.
    #[must_use]
    pub fn is_reversed(&self) -> bool {
        self.reversed
    }

    /// Lap length along the 3D centerline, in meters.
    ///
    /// The same lap, forward or reversed.
    #[must_use]
    pub fn length_m(&self) -> f64 {
        self.length_m
    }

    /// Centerline position at `distance_m`, in meters, as `[x, y, z]`.
    ///
    /// `y` is elevation. Non-finite distances sample the start line. On a
    /// reversed course this is the stored position at the mirrored distance.
    #[must_use]
    pub fn position_at(&self, distance_m: f64) -> [f32; 3] {
        let s_h = self.s_h_at(self.forward_distance(distance_m));
        let (x, z, _) = self.pose_at(s_h);
        let y = self.elevation_at_h(s_h);
        [x as f32, y as f32, z as f32]
    }

    /// Unit tangent at `distance_m`, pointing in the direction of travel.
    ///
    /// Components are dimensionless. The vector is normalized in `f32`.
    /// A reversed course reports the negation of the stored tangent, which
    /// stays unit length and points backward along the loop.
    #[must_use]
    pub fn tangent_at(&self, distance_m: f64) -> [f32; 3] {
        let s_h = self.s_h_at(self.forward_distance(distance_m));
        let (_, _, heading) = self.pose_at(s_h);
        let grade = self.grade_at_h(s_h) / 100.0;
        let cos_theta = 1.0 / (1.0 + grade * grade).sqrt();
        let tangent = unit_f32(DVec3::new(
            heading.cos() * cos_theta,
            grade * cos_theta,
            heading.sin() * cos_theta,
        ));
        if self.reversed {
            [-tangent[0], -tangent[1], -tangent[2]]
        } else {
            tangent
        }
    }

    /// Elevation above the course datum at `distance_m`, in meters.
    ///
    /// On a reversed course this is the stored elevation at the mirrored
    /// distance. The start line is still the datum, so distance 0 and the
    /// lap length are both 0 m.
    #[must_use]
    pub fn elevation_at(&self, distance_m: f64) -> f32 {
        self.elevation_at_h(self.s_h_at(self.forward_distance(distance_m))) as f32
    }

    /// Grade at `distance_m`, in percent.
    ///
    /// Positive is uphill in the direction of travel. The value is rise over
    /// horizontal run, not rise over 3D distance: `100 * tan(theta)`.
    /// A reversed course reports the negation of the stored grade at the
    /// mirrored distance. Grade is continuous across span boundaries, so
    /// that negation is the slope of [`Course::elevation_at`] along
    /// increasing distance, including when a sample lands on a boundary.
    #[must_use]
    pub fn grade_pct_at(&self, distance_m: f64) -> f32 {
        let grade = self.grade_at_h(self.s_h_at(self.forward_distance(distance_m))) as f32;
        if self.reversed { -grade } else { grade }
    }

    /// Elevation chart from the start line to the lap length.
    ///
    /// `step_m` is the 3D spacing in meters, walked in the direction of
    /// travel. The result always includes distance 0 and the lap length when
    /// `step_m` is finite and at least one micrometer, and the chart would
    /// contain at most 100_000 points. Otherwise the result is empty.
    ///
    /// A reversed course uses the same distances and the mirrored elevations,
    /// so the chart is the forward profile flipped along distance.
    #[must_use]
    pub fn profile(&self, step_m: f64) -> Vec<ProfilePoint> {
        if !step_m.is_finite() || step_m < 1.0e-6 || self.length_m <= 0.0 {
            return Vec::new();
        }
        let count = (self.length_m / step_m).ceil() as usize + 1;
        if count > 100_000 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(count);
        let mut distance = 0.0;
        loop {
            out.push(ProfilePoint {
                distance_m: distance,
                elevation_m: self.elevation_at(distance),
            });
            if distance >= self.length_m - 1.0e-9 {
                break;
            }
            let mut next = distance + step_m;
            // Snap a short closing sample onto the lap length so the chart
            // does not end with two points a fraction of a millimeter apart.
            if next >= self.length_m - 1.0e-3 {
                next = self.length_m;
            }
            if next <= distance {
                break;
            }
            distance = next;
        }
        out
    }

    /// Total ascent over one lap, in meters.
    ///
    /// This is the sum of the upward vertical travel. On a closed loop it
    /// equals the total descent, so reversing the course does not change it:
    /// climbing backward is the forward descent.
    #[must_use]
    pub fn total_ascent_m(&self) -> f32 {
        self.total_ascent_m
    }

    // 3D distance on the stored centerline for a travel distance.
    //
    // Forward courses return `distance_m` unchanged, including non-finite
    // values, so `s_h_at` keeps its wrap and start-line rules. A reversed
    // course maps `d` through `wrap(d) = d.rem_euclid(length)` to
    // `(length - wrap(d)).rem_euclid(length)`. Distance 0 stays on the start
    // line, and increasing distance walks backward along the loop.
    fn forward_distance(&self, distance_m: f64) -> f64 {
        if !self.reversed || !distance_m.is_finite() || self.length_m <= 0.0 {
            return distance_m;
        }
        let wrapped = distance_m.rem_euclid(self.length_m);
        (self.length_m - wrapped).rem_euclid(self.length_m)
    }

    fn s_h_at(&self, distance_m: f64) -> f64 {
        let n = self.s_h_at_s3d.len().saturating_sub(1);
        if n == 0 || !distance_m.is_finite() || self.length_m <= 0.0 {
            return 0.0;
        }
        let wrapped = distance_m.rem_euclid(self.length_m);
        let step = self.length_m / n as f64;
        let x = wrapped / step;
        let mut index = x.floor() as usize;
        if index >= n {
            index = n - 1;
        }
        let t = (x - index as f64).clamp(0.0, 1.0);
        let s0 = self.s_h_at_s3d[index];
        let s1 = self.s_h_at_s3d[index + 1];
        s0 + (s1 - s0) * t
    }

    fn pose_at(&self, s_h: f64) -> (f64, f64, f64) {
        let s = if s_h.is_finite() {
            s_h.clamp(0.0, self.horizontal_m)
        } else {
            0.0
        };
        let Some(first) = self.placed.first() else {
            return (0.0, 0.0, 0.0);
        };
        let mut chosen = first;
        for seg in &self.placed {
            if s + 1.0e-9 >= seg.s0 {
                chosen = seg;
            } else {
                break;
            }
        }
        let ds = (s - chosen.s0).max(0.0);
        match chosen.kind {
            SegKind::Line { length } => {
                sample_line(chosen.x, chosen.z, chosen.heading, ds.min(length))
            }
            SegKind::Arc { radius, angle } => {
                let max_d = radius * angle.abs();
                sample_arc(
                    chosen.x,
                    chosen.z,
                    chosen.heading,
                    radius,
                    angle,
                    ds.min(max_d),
                )
            }
        }
    }

    fn grade_at_h(&self, s_h: f64) -> f64 {
        let s = s_h.clamp(0.0, self.horizontal_m);
        let mut cursor = 0.0;
        for (index, span) in self.grade.iter().enumerate() {
            let len = span.length();
            let end = cursor + len;
            let last = index + 1 == self.grade.len();
            if s <= end + 1.0e-9 || last {
                return span.grade_at((s - cursor).clamp(0.0, len));
            }
            cursor = end;
        }
        0.0
    }

    fn elevation_at_h(&self, s_h: f64) -> f64 {
        let s = s_h.clamp(0.0, self.horizontal_m);
        let mut cursor = 0.0;
        let mut elevation = 0.0;
        for span in &self.grade {
            let len = span.length();
            let end = cursor + len;
            if s <= end + 1.0e-12 {
                return elevation + span.elev_gain((s - cursor).clamp(0.0, len));
            }
            elevation += span.elev_gain(len);
            cursor = end;
        }
        elevation
    }
}

impl std::str::FromStr for Course {
    type Err = CourseError;

    /// Parses a course description.
    ///
    /// The text is JSON. Validation failures return [`CourseError`] instead
    /// of panicking: bad JSON, an empty name, non-positive lengths, a path
    /// that does not close, a grade discontinuity, grade steeper than 100%,
    /// or a net elevation change.
    fn from_str(data: &str) -> Result<Self, Self::Err> {
        parse_course(data)
    }
}

fn unit_f32(v: DVec3) -> [f32; 3] {
    let mut out = [v.x as f32, v.y as f32, v.z as f32];
    let mag = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
    if mag > 0.0 {
        out = [out[0] / mag, out[1] / mag, out[2] / mag];
    } else {
        out = [1.0, 0.0, 0.0];
    }
    out
}

fn sample_line(x: f64, z: f64, heading: f64, distance: f64) -> (f64, f64, f64) {
    (
        x + distance * heading.cos(),
        z + distance * heading.sin(),
        heading,
    )
}

/// Point and heading `distance` meters along a circular arc.
///
/// `angle` is the full signed sweep of the arc in radians, positive to the
/// left. `distance` runs from 0 to `radius * angle.abs()`.
fn sample_arc(
    x: f64,
    z: f64,
    heading: f64,
    radius: f64,
    angle: f64,
    distance: f64,
) -> (f64, f64, f64) {
    let dir = if angle >= 0.0 { 1.0 } else { -1.0 };
    let travelled = dir * distance / radius;
    let normal = heading + dir * std::f64::consts::FRAC_PI_2;
    let cx = x + radius * normal.cos();
    let cz = z + radius * normal.sin();
    let start_from_center = heading - dir * std::f64::consts::FRAC_PI_2;
    let a = start_from_center + travelled;
    (
        cx + radius * a.cos(),
        cz + radius * a.sin(),
        heading + travelled,
    )
}

fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(std::f64::consts::TAU);
    if d > std::f64::consts::PI {
        d - std::f64::consts::TAU
    } else {
        d
    }
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep_integral(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let t3 = t * t * t;
    t3 - 0.5 * t3 * t
}

#[derive(Clone)]
struct Placed {
    s0: f64,
    x: f64,
    z: f64,
    heading: f64,
    kind: SegKind,
}

#[derive(Clone, Copy)]
enum SegKind {
    Line { length: f64 },
    Arc { radius: f64, angle: f64 },
}

#[derive(Clone)]
enum GradeSpan {
    Hold {
        length: f64,
        grade_pct: f64,
    },
    Smooth {
        length: f64,
        from_pct: f64,
        to_pct: f64,
    },
    Wave {
        length: f64,
        amplitude_pct: f64,
        cycles: u32,
    },
}

impl GradeSpan {
    fn length(&self) -> f64 {
        match *self {
            Self::Hold { length, .. } | Self::Smooth { length, .. } | Self::Wave { length, .. } => {
                length
            }
        }
    }

    fn start_grade(&self) -> f64 {
        match *self {
            Self::Hold { grade_pct, .. } => grade_pct,
            Self::Smooth { from_pct, .. } => from_pct,
            Self::Wave { .. } => 0.0,
        }
    }

    fn end_grade(&self) -> f64 {
        match *self {
            Self::Hold { grade_pct, .. } => grade_pct,
            Self::Smooth { to_pct, .. } => to_pct,
            Self::Wave { .. } => 0.0,
        }
    }

    fn grade_at(&self, s: f64) -> f64 {
        match *self {
            Self::Hold { grade_pct, .. } => grade_pct,
            Self::Smooth {
                length,
                from_pct,
                to_pct,
            } => {
                let t = if length > 0.0 { s / length } else { 0.0 };
                from_pct + (to_pct - from_pct) * smoothstep(t)
            }
            Self::Wave {
                length,
                amplitude_pct,
                cycles,
            } => {
                let omega = std::f64::consts::TAU * f64::from(cycles) / length;
                amplitude_pct * (omega * s).sin()
            }
        }
    }

    fn elev_gain(&self, s: f64) -> f64 {
        match *self {
            Self::Hold { grade_pct, .. } => grade_pct / 100.0 * s,
            Self::Smooth {
                length,
                from_pct,
                to_pct,
            } => {
                let t = if length > 0.0 {
                    (s / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (from_pct / 100.0) * s
                    + ((to_pct - from_pct) / 100.0) * length * smoothstep_integral(t)
            }
            Self::Wave {
                length,
                amplitude_pct,
                cycles,
            } => {
                let omega = std::f64::consts::TAU * f64::from(cycles) / length;
                if omega == 0.0 {
                    0.0
                } else {
                    (amplitude_pct / 100.0) / omega * (1.0 - (omega * s).cos())
                }
            }
        }
    }

    fn ascent_descent(&self) -> (f64, f64) {
        match *self {
            Self::Hold { length, grade_pct } => {
                let dy = grade_pct / 100.0 * length;
                if dy >= 0.0 { (dy, 0.0) } else { (0.0, -dy) }
            }
            Self::Smooth {
                length,
                from_pct,
                to_pct,
            } => smooth_ascent_descent(length, from_pct, to_pct),
            Self::Wave {
                length,
                amplitude_pct,
                ..
            } => {
                let ascent = (amplitude_pct / 100.0).abs() * length / std::f64::consts::PI;
                (ascent, ascent)
            }
        }
    }
}

fn smooth_ascent_descent(length: f64, from_pct: f64, to_pct: f64) -> (f64, f64) {
    let span = GradeSpan::Smooth {
        length,
        from_pct,
        to_pct,
    };
    let dy_end = span.elev_gain(length);
    if from_pct * to_pct < 0.0 {
        let target = -from_pct / (to_pct - from_pct);
        let mut lo = 0.0;
        let mut hi = 1.0;
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if smoothstep(mid) < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let s_root = 0.5 * (lo + hi) * length;
        let dy_root = span.elev_gain(s_root);
        if from_pct > 0.0 {
            (dy_root.max(0.0), (dy_root - dy_end).max(0.0))
        } else {
            ((dy_end - dy_root).max(0.0), (-dy_root).max(0.0))
        }
    } else if dy_end >= 0.0 {
        (dy_end, 0.0)
    } else {
        (0.0, -dy_end)
    }
}

const MAX_ABS_GRADE_PCT: f64 = 100.0;
const POSITION_TOL_M: f64 = 1.0e-3;
const HEADING_TOL_RAD: f64 = 1.0e-5;
const ELEVATION_TOL_M: f64 = 1.0e-3;
const GRADE_MATCH_TOL: f64 = 1.0e-3;
const SPAN_SUM_TOL_M: f64 = 1.0e-4;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CourseFile {
    name: String,
    start: StartFile,
    segments: Vec<SegmentFile>,
    grade: Vec<GradeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartFile {
    x: f64,
    z: f64,
    heading_rad: f64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SegmentFile {
    Line { length_m: f64 },
    Arc { radius_m: f64, angle_rad: f64 },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum GradeFile {
    Hold {
        length_m: f64,
        grade_pct: f64,
    },
    Smooth {
        length_m: f64,
        to_grade_pct: f64,
    },
    Wave {
        length_m: f64,
        amplitude_pct: f64,
        cycles: u32,
    },
}

fn parse_course(data: &str) -> Result<Course, CourseError> {
    let file: CourseFile = serde_json::from_str(data)
        .map_err(|err| CourseError::new(format!("invalid JSON: {err}")))?;
    if file.name.trim().is_empty() {
        return Err(CourseError::new("name must not be empty"));
    }
    if !file.start.x.is_finite() || !file.start.z.is_finite() || !file.start.heading_rad.is_finite()
    {
        return Err(CourseError::new("start pose is not finite"));
    }
    if file.segments.is_empty() {
        return Err(CourseError::new("course needs at least one segment"));
    }
    if file.grade.is_empty() {
        return Err(CourseError::new("course needs a grade profile"));
    }

    let placed = place_segments(&file)?;
    let horizontal_m = placed
        .last()
        .map(|seg| seg.s0 + horizontal_len(&seg.kind))
        .unwrap_or(0.0);
    if horizontal_m < 1.0 {
        return Err(CourseError::new("horizontal length must be at least 1 m"));
    }
    let (end_x, end_z, end_h) = end_pose(&placed);
    let gap = (end_x - file.start.x).hypot(end_z - file.start.z);
    if gap > POSITION_TOL_M {
        return Err(CourseError::new(format!(
            "path does not close: gap is {gap} m"
        )));
    }
    let heading_gap = angle_diff(end_h, file.start.heading_rad).abs();
    if heading_gap > HEADING_TOL_RAD {
        return Err(CourseError::new(format!(
            "heading does not close: error is {heading_gap} rad"
        )));
    }

    let grade = parse_grade(&file.grade, horizontal_m)?;
    let mut ascent = 0.0;
    let mut descent = 0.0;
    for span in &grade {
        let (up, down) = span.ascent_descent();
        ascent += up;
        descent += down;
    }
    let net = ascent - descent;
    if net.abs() > ELEVATION_TOL_M {
        return Err(CourseError::new(format!(
            "net elevation is {net} m; a closed loop must return to the datum"
        )));
    }

    let draft = Course {
        name: file.name,
        reversed: false,
        length_m: 0.0,
        horizontal_m,
        total_ascent_m: ascent as f32,
        placed,
        grade,
        s_h_at_s3d: Vec::new(),
    };
    let (length_m, s_h_at_s3d) = build_arc_table(&draft);
    Ok(Course {
        length_m,
        s_h_at_s3d,
        ..draft
    })
}

fn horizontal_len(kind: &SegKind) -> f64 {
    match *kind {
        SegKind::Line { length } => length,
        SegKind::Arc { radius, angle } => radius * angle.abs(),
    }
}

fn place_segments(file: &CourseFile) -> Result<Vec<Placed>, CourseError> {
    let mut placed = Vec::with_capacity(file.segments.len());
    let mut x = file.start.x;
    let mut z = file.start.z;
    let mut heading = file.start.heading_rad;
    let mut s = 0.0;
    for (index, segment) in file.segments.iter().enumerate() {
        let kind = match *segment {
            SegmentFile::Line { length_m } => {
                if !length_m.is_finite() || length_m <= 0.0 {
                    return Err(CourseError::new(format!(
                        "segment {index} line length must be finite and positive"
                    )));
                }
                SegKind::Line { length: length_m }
            }
            SegmentFile::Arc {
                radius_m,
                angle_rad,
            } => {
                if !radius_m.is_finite() || radius_m <= 0.1 {
                    return Err(CourseError::new(format!(
                        "segment {index} arc radius must be finite and greater than 0.1 m"
                    )));
                }
                if !angle_rad.is_finite()
                    || angle_rad.abs() < 1.0e-6
                    || angle_rad.abs() > std::f64::consts::TAU + 1.0e-4
                {
                    return Err(CourseError::new(format!(
                        "segment {index} arc angle must be finite, non-zero, and at most one turn"
                    )));
                }
                SegKind::Arc {
                    radius: radius_m,
                    angle: angle_rad,
                }
            }
        };
        placed.push(Placed {
            s0: s,
            x,
            z,
            heading,
            kind,
        });
        let len = horizontal_len(&placed[index].kind);
        let (nx, nz, nh) = match placed[index].kind {
            SegKind::Line { length } => sample_line(x, z, heading, length),
            SegKind::Arc { radius, angle } => {
                sample_arc(x, z, heading, radius, angle, radius * angle.abs())
            }
        };
        x = nx;
        z = nz;
        heading = nh;
        s += len;
    }
    Ok(placed)
}

fn end_pose(placed: &[Placed]) -> (f64, f64, f64) {
    let Some(last) = placed.last() else {
        return (0.0, 0.0, 0.0);
    };
    match last.kind {
        SegKind::Line { length } => sample_line(last.x, last.z, last.heading, length),
        SegKind::Arc { radius, angle } => sample_arc(
            last.x,
            last.z,
            last.heading,
            radius,
            angle,
            radius * angle.abs(),
        ),
    }
}

fn parse_grade(files: &[GradeFile], horizontal_m: f64) -> Result<Vec<GradeSpan>, CourseError> {
    let mut spans = Vec::with_capacity(files.len());
    let mut previous: Option<f64> = None;
    for (index, file) in files.iter().enumerate() {
        let span = match *file {
            GradeFile::Hold {
                length_m,
                grade_pct,
            } => {
                check_span_length(index, length_m)?;
                check_grade_value(index, grade_pct)?;
                if let Some(prev) = previous
                    && (prev - grade_pct).abs() > GRADE_MATCH_TOL
                {
                    return Err(CourseError::new(format!(
                        "grade jump at span {index}: {prev}% to {grade_pct}%"
                    )));
                }
                previous = Some(grade_pct);
                GradeSpan::Hold {
                    length: length_m,
                    grade_pct,
                }
            }
            GradeFile::Smooth {
                length_m,
                to_grade_pct,
            } => {
                check_span_length(index, length_m)?;
                check_grade_value(index, to_grade_pct)?;
                let from_pct = previous.unwrap_or(0.0);
                check_grade_value(index, from_pct)?;
                previous = Some(to_grade_pct);
                GradeSpan::Smooth {
                    length: length_m,
                    from_pct,
                    to_pct: to_grade_pct,
                }
            }
            GradeFile::Wave {
                length_m,
                amplitude_pct,
                cycles,
            } => {
                check_span_length(index, length_m)?;
                check_grade_value(index, amplitude_pct)?;
                check_grade_value(index, -amplitude_pct)?;
                if cycles == 0 || cycles > 64 {
                    return Err(CourseError::new(format!(
                        "grade span {index} wave cycles must be from 1 to 64"
                    )));
                }
                let from = previous.unwrap_or(0.0);
                if from.abs() > GRADE_MATCH_TOL {
                    return Err(CourseError::new(format!(
                        "grade span {index} wave must start from 0% grade, not {from}%"
                    )));
                }
                previous = Some(0.0);
                GradeSpan::Wave {
                    length: length_m,
                    amplitude_pct,
                    cycles,
                }
            }
        };
        spans.push(span);
    }
    let sum: f64 = spans.iter().map(GradeSpan::length).sum();
    if (sum - horizontal_m).abs() > SPAN_SUM_TOL_M {
        return Err(CourseError::new(format!(
            "grade spans cover {sum} m but the horizontal length is {horizontal_m} m"
        )));
    }
    let start = spans[0].start_grade();
    let end = spans.last().map(GradeSpan::end_grade).unwrap_or(start);
    if (start - end).abs() > GRADE_MATCH_TOL {
        return Err(CourseError::new(format!(
            "grade does not match across the start line: {end}% into {start}%"
        )));
    }
    Ok(spans)
}

fn check_span_length(index: usize, length_m: f64) -> Result<(), CourseError> {
    if !length_m.is_finite() || length_m <= 0.0 {
        Err(CourseError::new(format!(
            "grade span {index} length must be finite and positive"
        )))
    } else {
        Ok(())
    }
}

fn check_grade_value(index: usize, grade_pct: f64) -> Result<(), CourseError> {
    if !grade_pct.is_finite() || grade_pct.abs() > MAX_ABS_GRADE_PCT {
        Err(CourseError::new(format!(
            "grade span {index} grade must be finite and within ±{MAX_ABS_GRADE_PCT}%"
        )))
    } else {
        Ok(())
    }
}

fn build_arc_table(course: &Course) -> (f64, Vec<f64>) {
    const DH: f64 = 0.05;
    let mut samples = Vec::new();
    let mut s_h = 0.0;
    let mut s_3d = 0.0;
    samples.push((0.0, 0.0));
    while s_h < course.horizontal_m - 1.0e-9 {
        let dh = DH.min(course.horizontal_m - s_h);
        let g0 = course.grade_at_h(s_h);
        let gm = course.grade_at_h(s_h + 0.5 * dh);
        let g1 = course.grade_at_h(s_h + dh);
        let factor = |grade: f64| (1.0 + (grade / 100.0).powi(2)).sqrt();
        let ds = dh / 6.0 * (factor(g0) + 4.0 * factor(gm) + factor(g1));
        s_h += dh;
        s_3d += ds;
        samples.push((s_h, s_3d));
    }
    if let Some(last) = samples.last_mut() {
        last.0 = course.horizontal_m;
    }
    let length = s_3d;
    let n = ((length / 0.25).round() as usize).max(8);
    let step = length / n as f64;
    let mut table = vec![0.0; n + 1];
    let mut j = 0;
    for (i, slot) in table.iter_mut().enumerate().skip(1).take(n - 1) {
        let target = i as f64 * step;
        while j + 1 < samples.len() && samples[j + 1].1 < target {
            j += 1;
        }
        let (h0, z0) = samples[j];
        let (h1, z1) = samples
            .get(j + 1)
            .copied()
            .unwrap_or((course.horizontal_m, length));
        let t = if z1 > z0 {
            ((target - z0) / (z1 - z0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        *slot = h0 + (h1 - h0) * t;
    }
    table[n] = course.horizontal_m;
    (length, table)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn pos_dist(a: [f32; 3], b: [f32; 3]) -> f32 {
        let dx = a[0] - b[0];
        let dy = a[1] - b[1];
        let dz = a[2] - b[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    fn heading_of(tangent: [f32; 3]) -> f64 {
        f64::from(tangent[2]).atan2(f64::from(tangent[0]))
    }

    #[test]
    fn left_quarter_circle_ends_north_of_east() {
        let (x, z, h) = sample_arc(
            0.0,
            0.0,
            0.0,
            10.0,
            std::f64::consts::FRAC_PI_2,
            10.0 * std::f64::consts::FRAC_PI_2,
        );
        assert!((x - 10.0).abs() < 1e-9, "{x}");
        assert!((z - 10.0).abs() < 1e-9, "{z}");
        assert!((h - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{h}");
    }

    #[test]
    fn right_hairpin_from_west_offsets_north() {
        let r = 36.0;
        let (x, z, h) = sample_arc(
            0.0,
            0.0,
            std::f64::consts::PI,
            r,
            -std::f64::consts::PI,
            r * std::f64::consts::PI,
        );
        assert!(x.abs() < 1e-8, "{x}");
        assert!((z - 2.0 * r).abs() < 1e-8, "{z}");
        assert!(angle_diff(h, 0.0).abs() < 1e-8, "{h}");
    }

    #[test]
    fn smoothstep_full_integral_is_half() {
        assert!((smoothstep_integral(1.0) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn wave_ascent_matches_the_closed_form() {
        let length = 800.0;
        let span = GradeSpan::Wave {
            length,
            amplitude_pct: 5.0,
            cycles: 2,
        };
        let (up, down) = span.ascent_descent();
        let expected = 0.05 * length / std::f64::consts::PI;
        assert!((up - expected).abs() < 1e-9, "{up} vs {expected}");
        assert!((down - expected).abs() < 1e-9);
        assert!(span.elev_gain(length).abs() < 1e-9);
    }

    #[test]
    fn lonely_road_loads_closes_and_reverses() {
        let course = Course::lonely_road();
        assert_eq!(course.name(), "Lonely Road");
        let length = course.length_m();
        assert!((length - 18_000.0).abs() <= 100.0, "lap length {length} m");

        let p0 = course.position_at(0.0);
        let p_end = course.position_at(length);
        assert!(pos_dist(p0, p_end) < 1e-2, "seam {p0:?} vs {p_end:?}");
        assert!(course.elevation_at(0.0).abs() < 1e-2);
        assert!(course.elevation_at(length).abs() < 1e-2);

        let mut min_g = f32::MAX;
        let mut max_g = f32::MIN;
        let mut d = 0.0;
        while d <= length {
            let g = course.grade_pct_at(d);
            min_g = min_g.min(g);
            max_g = max_g.max(g);
            d += 10.0;
        }
        assert!(max_g <= 2.5, "max grade {max_g}%");
        assert!(min_g >= -2.5, "min grade {min_g}%");
        assert!(f64::from(course.total_ascent_m()) >= 80.0);
        assert!(f64::from(course.total_ascent_m()) <= 125.0);

        let mut longest_straight = 0.0_f64;
        let mut run = 0.0_f64;
        let mut prev = heading_of(course.tangent_at(0.0));
        let step = 5.0;
        let mut dist = 0.0;
        while dist < length {
            dist += step;
            let h = heading_of(course.tangent_at(dist));
            let dh = angle_diff(h, prev).abs();
            if dh < 1.0e-4 {
                run += step;
            } else {
                longest_straight = longest_straight.max(run);
                run = 0.0;
            }
            prev = h;
        }
        longest_straight = longest_straight.max(run);
        assert!(
            longest_straight > 4_000.0,
            "longest near-straight run {longest_straight} m"
        );

        let backward = course.reversed();
        assert!(backward.is_reversed());
        let mirrored = (length - length * 0.42).rem_euclid(length);
        let gap = (backward.elevation_at(length * 0.42) - course.elevation_at(mirrored)).abs();
        assert!(gap < 1.0e-2_f32, "{gap}");
        let back = backward.reversed();
        assert!(!back.is_reversed());
        assert!((back.length_m() - length).abs() < 1e-6);
    }

    #[test]
    fn penguin_pass_length_seam_and_tangent() {
        let course = Course::penguin_pass();
        assert_eq!(course.name(), "Penguin Pass");
        let length = course.length_m();
        assert!(
            (length - 5_000.0).abs() <= 5.0,
            "lap length {length} m is outside 5000 ± 5"
        );

        let p0 = course.position_at(0.0);
        let p_end = course.position_at(length);
        let p_neg = course.position_at(-length);
        assert!(
            pos_dist(p0, p_end) < 1e-2,
            "seam position {p0:?} vs {p_end:?}"
        );
        assert!(pos_dist(p0, p_neg) < 1e-2, "negative wrap {p_neg:?}");

        let t0 = course.tangent_at(0.0);
        let t_end = course.tangent_at(length);
        let dot = t0[0] * t_end[0] + t0[1] * t_end[1] + t0[2] * t_end[2];
        assert!(
            dot > 0.9999,
            "seam heading dot {dot}, t0 {t0:?} t_end {t_end:?}"
        );

        // 1 m of parameter is 1 m of road, including across the start line.
        let steps = length.floor() as usize;
        for i in 0..steps {
            let d0 = i as f64;
            let d1 = d0 + 1.0;
            let dist = pos_dist(course.position_at(d0), course.position_at(d1));
            assert!((dist - 1.0).abs() < 0.01, "3D step at {d0} m was {dist} m");
        }
    }

    #[test]
    fn penguin_pass_grade_ascent_and_datum() {
        let course = Course::penguin_pass();
        let length = course.length_m();
        let mut min_g = f32::MAX;
        let mut max_g = f32::MIN;
        let mut max_jump = 0.0f32;
        let mut up = 0.0f64;
        let mut down = 0.0f64;
        let mut min_elev = f32::MAX;
        let mut d = 0.0;
        let mut prev_g = course.grade_pct_at(0.0);
        while d < length {
            let g = course.grade_pct_at(d);
            assert!(
                (-12.0..=12.0).contains(&g),
                "grade {g}% at {d} m is outside ±12%"
            );
            min_g = min_g.min(g);
            max_g = max_g.max(g);
            let elev = course.elevation_at(d);
            min_elev = min_elev.min(elev);
            let d2 = (d + 1.0).min(length);
            let g2 = course.grade_pct_at(d2);
            let jump = (g2 - prev_g).abs();
            max_jump = max_jump.max(jump);
            let dy = f64::from(course.elevation_at(d2) - elev);
            if dy >= 0.0 {
                up += dy;
            } else {
                down -= dy;
            }
            prev_g = g2;
            if d2 <= d {
                break;
            }
            d = d2;
        }
        assert!(
            max_jump <= 1.5,
            "grade jump {max_jump}% exceeds 1.5% per meter (range {min_g}..{max_g})"
        );
        // Samples telescope around a closed loop, so the positive and
        // negative elevation changes differ only by rounding.
        assert!(
            (up - down).abs() < 0.05,
            "ascent {up} m vs descent {down} m"
        );
        let ascent = f64::from(course.total_ascent_m());
        assert!(
            (150.0..=220.0).contains(&ascent),
            "total ascent {ascent} m is outside 150..=220"
        );
        assert!(
            (ascent - up).abs() < 0.5,
            "analytic ascent {ascent} vs sampled {up}"
        );
        assert!(
            min_elev >= -0.05,
            "road drops below the start datum: {min_elev} m"
        );
        assert!(course.elevation_at(0.0).abs() < 1e-3);
    }

    #[test]
    fn penguin_pass_turn_radius_is_at_least_12m() {
        let course = Course::penguin_pass();
        let steps = course.length_m().floor() as usize;
        let mut min_radius = f64::MAX;
        for i in 0..steps {
            let d0 = i as f64;
            let d1 = d0 + 1.0;
            let p0 = course.position_at(d0);
            let p1 = course.position_at(d1);
            let horiz = f64::hypot(f64::from(p1[0] - p0[0]), f64::from(p1[2] - p0[2]));
            let dh = angle_diff(
                heading_of(course.tangent_at(d1)),
                heading_of(course.tangent_at(d0)),
            )
            .abs();
            if horiz < 1e-3 || dh < 1e-6 {
                continue;
            }
            let radius = horiz / dh;
            min_radius = min_radius.min(radius);
        }
        assert!(
            min_radius >= 12.0,
            "minimum horizontal radius {min_radius} m is under 12 m"
        );
    }

    #[test]
    fn penguin_pass_clearance_and_footprint() {
        let course = Course::penguin_pass();
        let length = course.length_m();
        // Points farther than this along the centerline (the short way
        // around the loop) are not a consecutive stretch. 60 m is a few
        // seconds of road, long enough to cover the local neighborhood of a
        // bend. Switchback legs sit much farther apart than 20 m, so they
        // are checked rather than exempted.
        let consecutive_m = 60.0;
        let n = length.round() as usize;
        let step = length / n as f64;
        let points: Vec<(f64, f64)> = (0..n)
            .map(|i| {
                let p = course.position_at(i as f64 * step);
                (f64::from(p[0]), f64::from(p[2]))
            })
            .collect();
        let mut min_sq = f64::MAX;
        let mut min_pair = (0usize, 0usize);
        for i in 0..n {
            for j in (i + 1)..n {
                let along = (j - i) as f64 * step;
                let short = along.min(length - along);
                if short <= consecutive_m {
                    continue;
                }
                let dx = points[j].0 - points[i].0;
                let dz = points[j].1 - points[i].1;
                let dsq = dx * dx + dz * dz;
                if dsq < min_sq {
                    min_sq = dsq;
                    min_pair = (i, j);
                }
            }
        }
        let min_clear = min_sq.sqrt();
        let (i, j) = min_pair;
        assert!(
            min_clear >= 20.0,
            "plan clearance {min_clear} m between {i} m and {j} m (limit 20 m, ignoring stretches within {consecutive_m} m)"
        );

        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_z = f64::MAX;
        let mut max_z = f64::MIN;
        for (x, z) in points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_z = min_z.min(z);
            max_z = max_z.max(z);
        }
        let dx = max_x - min_x;
        let dz = max_z - min_z;
        assert!(dx <= 1_500.0 && dz <= 1_500.0, "footprint {dx} m by {dz} m");
        let cx = 0.5 * (min_x + max_x);
        let cz = 0.5 * (min_z + max_z);
        assert!(
            cx.abs() < 15.0 && cz.abs() < 15.0,
            "footprint center ({cx}, {cz}) is far from the origin"
        );
    }

    #[test]
    fn sampling_wraps_and_grade_matches_elevation() {
        let course = Course::penguin_pass();
        let length = course.length_m();
        let mut d = 0.0;
        while d <= length {
            let p = course.position_at(d);
            let wrapped = course.position_at(d + 2.0 * length);
            let negative = course.position_at(d - length);
            assert!(pos_dist(p, wrapped) < 1e-2, "wrap at {d}");
            assert!(pos_dist(p, negative) < 1e-2, "negative at {d}");
            assert!((p[1] - course.elevation_at(d)).abs() < 1e-4);

            let t = course.tangent_at(d);
            let mag = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
            assert!((mag - 1.0).abs() < 1e-5, "tangent mag {mag} at {d}");

            let ahead = course.position_at(d + 1.0);
            let dx = ahead[0] - p[0];
            let dy = ahead[1] - p[1];
            let dz = ahead[2] - p[2];
            let step = (dx * dx + dy * dy + dz * dz).sqrt();
            let dot = t[0] * dx + t[1] * dy + t[2] * dz;
            assert!(step > 0.5 && dot / step > 0.99, "tangent diverges at {d}");

            // Centered difference over 1 m of 3D distance. Grade changes
            // slowly, so this agrees with the analytic rise/run to well
            // under a tenth of a percent.
            let eps = 0.5;
            let y0 = course.elevation_at(d - eps);
            let y1 = course.elevation_at(d + eps);
            let p0 = course.position_at(d - eps);
            let p1 = course.position_at(d + eps);
            let horiz = f64::hypot(f64::from(p1[0] - p0[0]), f64::from(p1[2] - p0[2]));
            assert!(horiz > 0.2, "horizontal step collapsed at {d}");
            let finite = 100.0 * f64::from(y1 - y0) / horiz;
            let grade = f64::from(course.grade_pct_at(d));
            assert!(
                (grade - finite).abs() < 0.05,
                "grade {grade}% vs finite difference {finite}% at {d} m"
            );

            d += 100.0;
        }

        let profile = course.profile(250.0);
        assert!(profile.len() > 10);
        assert!(profile[0].distance_m.abs() < 1e-9);
        let last = profile.last().expect("profile has a point");
        assert!((last.distance_m - length).abs() < 1e-6);
        for window in profile.windows(2) {
            assert!(window[1].distance_m > window[0].distance_m);
            // The last segment closes the lap, so it can be a hair longer
            // than the requested step when the length is not a multiple.
            assert!(window[1].distance_m - window[0].distance_m <= 250.0 + 1e-3);
            let elev = course.elevation_at(window[0].distance_m);
            assert!((elev - window[0].elevation_m).abs() < 1e-4);
        }
        assert!(course.profile(0.0).is_empty());
        assert!(course.profile(f64::NAN).is_empty());
    }

    #[test]
    fn invalid_descriptions_are_errors() {
        let err = Course::from_str("{").unwrap_err();
        assert!(err.to_string().contains("invalid JSON"), "{err}");

        let err = Course::from_str(
            r#"{"name":"  ","start":{"x":0,"z":0,"heading_rad":0},"segments":[{"type":"line","length_m":10}],"grade":[{"type":"hold","length_m":10,"grade_pct":0}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("name"), "{err}");

        let open = serde_json::json!({
            "name": "Open",
            "start": {"x": 0.0, "z": 0.0, "heading_rad": 0.0},
            "segments": [{"type": "line", "length_m": 100.0}],
            "grade": [{"type": "hold", "length_m": 100.0, "grade_pct": 0.0}]
        });
        let err = Course::from_str(&open.to_string()).unwrap_err();
        assert!(err.to_string().contains("does not close"), "{err}");

        let angle = std::f64::consts::TAU;
        let radius = 40.0;
        let length = radius * angle;
        let jumped = serde_json::json!({
            "name": "Jump",
            "start": {"x": 0.0, "z": 0.0, "heading_rad": 0.0},
            "segments": [{"type": "arc", "radius_m": radius, "angle_rad": angle}],
            "grade": [
                {"type": "hold", "length_m": length / 2.0, "grade_pct": 0.0},
                {"type": "hold", "length_m": length / 2.0, "grade_pct": 5.0}
            ]
        });
        let err = Course::from_str(&jumped.to_string()).unwrap_err();
        assert!(err.to_string().contains("grade jump"), "{err}");

        let climbed = serde_json::json!({
            "name": "Net climb",
            "start": {"x": 0.0, "z": 0.0, "heading_rad": 0.0},
            "segments": [{"type": "arc", "radius_m": radius, "angle_rad": angle}],
            "grade": [
                {"type": "smooth", "length_m": length / 2.0, "to_grade_pct": 4.0},
                {"type": "smooth", "length_m": length / 2.0, "to_grade_pct": 0.0}
            ]
        });
        let err = Course::from_str(&climbed.to_string()).unwrap_err();
        assert!(err.to_string().contains("net elevation"), "{err}");
    }

    fn grade_samples(course: &Course) -> Vec<(f64, f32)> {
        let steps = course.length_m().floor() as usize;
        (0..steps)
            .map(|i| {
                let distance = i as f64;
                (distance, course.grade_pct_at(distance))
            })
            .collect()
    }

    /// Climb from the first sustained rise through the kick, stopping where
    /// the grade falls away onto the ridge.
    fn climb_bounds(samples: &[(f64, f32)]) -> (f64, f64) {
        let start = samples
            .iter()
            .find(|(_, grade)| *grade > 2.0)
            .map(|(distance, _)| *distance)
            .expect("climb starts");
        let mut seen_kick = false;
        let mut end = start;
        for (distance, grade) in samples.iter().copied().filter(|(d, _)| *d >= start) {
            if grade >= 9.0 {
                seen_kick = true;
            }
            if seen_kick && grade < 1.0 {
                end = distance;
                break;
            }
        }
        (start, end)
    }

    fn descent_bounds(samples: &[(f64, f32)], climb_end: f64) -> (f64, f64) {
        // The ridge wave also dips below -2%. The descent is the first stretch
        // that stays steeper than -8% for 40 m.
        let start = samples
            .windows(40)
            .find(|window| {
                window[0].0 >= climb_end && window.iter().all(|(_, grade)| *grade < -8.0)
            })
            .map(|window| window[0].0)
            .expect("descent starts");
        let mut end = samples
            .last()
            .map(|(distance, _)| *distance)
            .unwrap_or(start);
        for (distance, grade) in samples.iter().copied().filter(|(d, _)| *d >= start) {
            if grade > -1.0 {
                end = distance;
                break;
            }
        }
        (start, end)
    }

    fn stddev(values: &[f32]) -> f64 {
        let n = values.len() as f64;
        let mean = values.iter().map(|value| f64::from(*value)).sum::<f64>() / n;
        let variance = values
            .iter()
            .map(|value| {
                let delta = f64::from(*value) - mean;
                delta * delta
            })
            .sum::<f64>()
            / n;
        variance.sqrt()
    }

    /// Runs where the grade stays inside a 0.3 point band for at least `min_len` meters.
    fn plateaus(samples: &[(f64, f32)], min_len: f64) -> Vec<(f64, f64, f32)> {
        let mut found = Vec::new();
        let mut index = 0;
        while index < samples.len() {
            let origin = samples[index].1;
            let mut end = index;
            while end + 1 < samples.len()
                && (samples[end + 1].1 - origin).abs() <= 0.3
                && samples[end + 1].0 - samples[index].0 <= 5_000.0
            {
                end += 1;
            }
            let length = samples[end].0 - samples[index].0;
            if length >= min_len {
                let mean = samples[index..=end]
                    .iter()
                    .map(|(_, grade)| f64::from(*grade))
                    .sum::<f64>()
                    / (end - index + 1) as f64;
                found.push((samples[index].0, samples[end].0, mean as f32));
                index = end + 1;
            } else {
                index += 1;
            }
        }
        found
    }

    fn distinct_pitches(means: &[f32], gap: f32) -> usize {
        let mut ordered = means.to_vec();
        ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut count = 1usize;
        let mut last = ordered[0];
        for value in ordered.iter().skip(1).copied() {
            if value - last >= gap {
                count += 1;
                last = value;
            }
        }
        count
    }

    fn longest_in_band(samples: &[(f64, f32)], lo: f32, hi: f32) -> Option<(f64, f64)> {
        let mut best: Option<(f64, f64)> = None;
        let mut run_start: Option<f64> = None;
        for (distance, grade) in samples {
            if (lo..=hi).contains(grade) {
                if run_start.is_none() {
                    run_start = Some(*distance);
                }
            } else if let Some(start) = run_start.take() {
                best = longer_run(best, start, *distance);
            }
        }
        if let Some(start) = run_start
            && let Some((last, _)) = samples.last()
        {
            best = longer_run(best, start, *last);
        }
        best
    }

    fn longer_run(best: Option<(f64, f64)>, start: f64, end: f64) -> Option<(f64, f64)> {
        match best {
            Some((open, close)) if close - open >= end - start => best,
            _ => Some((start, end)),
        }
    }

    #[test]
    fn penguin_pass_climb_is_not_one_pitch() {
        let course = Course::penguin_pass();
        let samples = grade_samples(&course);
        let (start, end) = climb_bounds(&samples);
        let climb: Vec<(f64, f32)> = samples
            .iter()
            .copied()
            .filter(|(distance, _)| *distance >= start && *distance < end)
            .collect();
        let pitches: Vec<f32> = plateaus(&climb, 80.0)
            .into_iter()
            .map(|(_, _, mean)| mean)
            .filter(|mean| *mean > 2.0)
            .collect();
        let distinct = distinct_pitches(&pitches, 1.5);
        assert!(
            distinct >= 4,
            "climb plateaus {pitches:?} collapse to {distinct} pitches; a constant ramp would be 1"
        );
        let values: Vec<f32> = climb.iter().map(|(_, grade)| *grade).collect();
        let spread = stddev(&values);
        // A single 8% ramp stays near 0.5 once the opening transition is
        // excluded by the plateau count; the spread here is the whole climb,
        // including transitions, and a flat ramp still lands well under 1.2.
        assert!(
            spread > 1.2,
            "climb grade standard deviation {spread:.2} is too small for a varied pitch (bounds {start:.0}..{end:.0})"
        );
    }

    #[test]
    fn penguin_pass_climb_has_a_breather_and_a_kick() {
        let course = Course::penguin_pass();
        let samples = grade_samples(&course);
        let (start, end) = climb_bounds(&samples);
        let span = end - start;
        let climb: Vec<(f64, f32)> = samples
            .iter()
            .copied()
            .filter(|(distance, _)| *distance >= start && *distance < end)
            .collect();
        let breather = longest_in_band(&climb, 3.0, 4.2).expect("breather run");
        let breather_len = breather.1 - breather.0;
        let breather_mid = (breather.0 + breather.1) * 0.5;
        assert!(
            (100.0..=160.0).contains(&breather_len),
            "breather length {breather_len:.0} m ({breather:?})"
        );
        let breather_fraction = (breather_mid - start) / span;
        assert!(
            (0.30..=0.60).contains(&breather_fraction),
            "breather sits at {breather_fraction:.2} of the climb, not around the middle"
        );
        let kick = longest_in_band(&climb, 9.5, 10.5).expect("kick run");
        let kick_len = kick.1 - kick.0;
        let kick_mid = (kick.0 + kick.1) * 0.5;
        assert!(
            (150.0..=260.0).contains(&kick_len),
            "kick length {kick_len:.0} m ({kick:?})"
        );
        let kick_fraction = (kick_mid - start) / span;
        assert!(
            kick_fraction >= 0.66,
            "kick midpoint is at {kick_fraction:.2} of the climb, not in the upper third"
        );
    }

    #[test]
    fn penguin_pass_descent_changes_pitch() {
        let course = Course::penguin_pass();
        let samples = grade_samples(&course);
        let (climb_start, climb_end) = climb_bounds(&samples);
        let (descent_start, descent_end) = descent_bounds(&samples, climb_end);
        let descent: Vec<(f64, f32)> = samples
            .iter()
            .copied()
            .filter(|(distance, _)| *distance >= descent_start && *distance < descent_end)
            .collect();
        let values: Vec<f32> = descent.iter().map(|(_, grade)| *grade).collect();
        let spread = stddev(&values);
        assert!(
            spread > 1.0,
            "descent grade standard deviation {spread:.2} still looks like one ramp"
        );
        let easing = longest_in_band(&descent, -7.5, -4.5).expect("descent easing");
        assert!(
            easing.1 - easing.0 >= 80.0,
            "descent easing is only {:.0} m",
            easing.1 - easing.0
        );
        let steep = longest_in_band(&descent, -12.0, -9.0).expect("steep descent");
        assert!(
            steep.1 - steep.0 >= 100.0,
            "steep descent run is only {:.0} m",
            steep.1 - steep.0
        );
        let ridge: Vec<f32> = samples
            .iter()
            .filter(|(distance, _)| *distance >= climb_end && *distance < descent_start)
            .map(|(_, grade)| *grade)
            .collect();
        let ridge_max = ridge.iter().copied().fold(f32::MIN, f32::max);
        let ridge_min = ridge.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            ridge_max > 4.0 && ridge_min < -4.0,
            "ridge between {climb_end:.0} and {descent_start:.0} spans {ridge_min:.1}..{ridge_max:.1}, climb started at {climb_start:.0}"
        );
    }

    #[test]
    fn reversed_toggles_and_round_trips() {
        let course = Course::penguin_pass();
        assert!(!course.is_reversed());
        assert_eq!(course.name(), "Penguin Pass");
        let backward = course.reversed();
        assert!(backward.is_reversed());
        assert_eq!(backward.name(), course.name());
        assert_eq!(backward.length_m(), course.length_m());
        assert_eq!(backward.total_ascent_m(), course.total_ascent_m());
        let restored = backward.reversed();
        assert!(!restored.is_reversed());
        let length = course.length_m();
        for i in 0..500 {
            let d = length * i as f64 / 500.0;
            assert_eq!(
                course.position_at(d),
                restored.position_at(d),
                "position at {d}"
            );
            assert_eq!(course.elevation_at(d), restored.elevation_at(d));
            assert_eq!(course.tangent_at(d), restored.tangent_at(d));
            assert_eq!(course.grade_pct_at(d), restored.grade_pct_at(d));
        }
    }

    #[test]
    fn reversed_start_line_is_the_datum() {
        let course = Course::penguin_pass().reversed();
        let length = course.length_m();
        assert_eq!(course.position_at(0.0), course.position_at(length));
        assert_eq!(course.elevation_at(0.0), 0.0);
        assert_eq!(course.elevation_at(length), 0.0);
    }

    #[test]
    fn reversed_tangent_matches_the_chord() {
        let course = Course::penguin_pass().reversed();
        let length = course.length_m();
        let n = 200;
        for i in 0..n {
            let d = length * i as f64 / n as f64;
            let t = course.tangent_at(d);
            let mag = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
            assert!((mag - 1.0).abs() < 1e-5, "tangent mag {mag} at {d}");
            let behind = course.position_at(d - 0.5);
            let ahead = course.position_at(d + 0.5);
            let dx = ahead[0] - behind[0];
            let dy = ahead[1] - behind[1];
            let dz = ahead[2] - behind[2];
            let chord = (dx * dx + dy * dy + dz * dz).sqrt();
            assert!(chord > 0.5, "chord {chord} at {d}");
            let dot = (t[0] * dx + t[1] * dy + t[2] * dz) / chord;
            assert!(dot > 0.999, "tangent dot {dot} at {d} m, tangent {t:?}");
        }
    }

    #[test]
    fn reversed_grade_matches_elevation_slope() {
        let forward = Course::penguin_pass();
        let course = forward.reversed();
        let length = course.length_m();
        let steps = length.floor() as usize;
        for i in 0..steps {
            let d = i as f64;
            let wrapped = d.rem_euclid(length);
            let d_fwd = (length - wrapped).rem_euclid(length);
            let stored = -forward.grade_pct_at(d_fwd);
            assert_eq!(course.grade_pct_at(d), stored, "grade sign at {d}");

            let y0 = course.elevation_at(d - 1.0);
            let y1 = course.elevation_at(d + 1.0);
            let p0 = course.position_at(d - 1.0);
            let p1 = course.position_at(d + 1.0);
            let horiz = f64::hypot(f64::from(p1[0] - p0[0]), f64::from(p1[2] - p0[2]));
            assert!(horiz > 0.4, "horizontal step collapsed at {d}");
            // Half the horizontal chord, so `2 * horizontal_step` is the run.
            let horizontal_step = horiz / 2.0;
            let slope = f64::from(y1 - y0) / (2.0 * horizontal_step);
            let grade = f64::from(course.grade_pct_at(d)) / 100.0;
            assert!(
                (grade - slope).abs() < 5.0e-4,
                "grade {grade} vs slope {slope} at {d} m"
            );
        }
    }

    #[test]
    fn reversed_profile_mirrors_forward() {
        let course = Course::penguin_pass();
        let forward = course.profile(64.0);
        let backward = course.reversed().profile(64.0);
        assert_eq!(forward.len(), backward.len());
        assert!(forward.len() > 2);
        let length = course.length_m();
        for (fwd, rev) in forward.iter().zip(&backward) {
            assert!((fwd.distance_m - rev.distance_m).abs() < 1e-9);
            let wrapped = fwd.distance_m.rem_euclid(length);
            let d_fwd = (length - wrapped).rem_euclid(length);
            assert_eq!(
                rev.elevation_m,
                course.elevation_at(d_fwd),
                "profile at {} m",
                fwd.distance_m
            );
        }
    }

    #[test]
    fn reversed_ascent_matches_integrated_climbs() {
        let forward = Course::penguin_pass();
        let course = forward.reversed();
        let length = course.length_m();
        let mut up = 0.0_f64;
        let mut down = 0.0_f64;
        let mut d = 0.0;
        while d < length {
            let d2 = (d + 1.0).min(length);
            let dy = f64::from(course.elevation_at(d2) - course.elevation_at(d));
            if dy >= 0.0 {
                up += dy;
            } else {
                down -= dy;
            }
            if d2 <= d {
                break;
            }
            d = d2;
        }
        assert!(
            (up - down).abs() < 0.05,
            "ascent {up} m vs descent {down} m"
        );
        let ascent = f64::from(course.total_ascent_m());
        assert_eq!(course.total_ascent_m(), forward.total_ascent_m());
        assert!(
            (ascent - up).abs() < 0.5,
            "analytic ascent {ascent} vs sampled {up}"
        );
    }
}
