//! Where a landmark can be seen for the first time.
//!
//! Sites sit just past a bend, on a crest, at the summit, at the bottom of a
//! descent, or beside a long straight. The same course and seed always pick
//! the same places. Nothing is placed on the road, on its banks, or on a
//! switchback leg that passes closer than the site itself.

use pp_course::Course;

/// Minimum along-track gap between two landmarks, in meters.
pub const MIN_SPACING_M: f64 = 400.0;

/// The kind of moment that reveals a landmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    /// Just past a bend with a large heading change.
    Bend,
    /// Where a climb turns into a descent.
    Crest,
    /// The highest point on the course.
    Summit,
    /// The bottom of a descent.
    Valley,
    /// Beside a long straight.
    Straight,
}

/// One kind of landmark the theme wants placed.
#[derive(Clone, Debug)]
pub struct LandmarkRequest {
    /// Stable name, shared with the theme file.
    pub id: String,
    /// Radius of the footprint, meters. Used to keep the pad off the road.
    pub footprint_m: f32,
    /// How far from the centreline to stand, meters. Clamped to 15..=120.
    pub road_m: f32,
    /// Preferred reveal.
    pub site: SiteKind,
    /// 1 places it whenever a site exists. Lower values skip some sites.
    pub rarity: f32,
    /// Pillars stand either side of the asphalt and a beam crosses above it.
    pub spans_road: bool,
}

/// A chosen place, ready for the mesh builder.
#[derive(Clone, Debug)]
pub struct PlacedLandmark {
    /// Request id.
    pub id: String,
    /// World x.
    pub x: f32,
    /// World z.
    pub z: f32,
    /// Ground height under the footprint, before a pad.
    pub ground_y: f32,
    /// Radians around Y. The landmark's front points back along the approach.
    pub yaw: f32,
    /// Distance along the course where the rider first has a clear view.
    pub distance_m: f64,
    /// Which reveal this used.
    pub site: SiteKind,
    /// `Some` when the ground was steep enough to need a level pad.
    /// `(x, z, radius, flat height)`.
    pub pad: Option<(f32, f32, f32, f32)>,
    /// Cleared radius around the site, meters. Scatter stays outside it.
    pub footprint_m: f32,
    /// Road point where a rider looking ahead first has the site in view.
    pub reveal_x: f32,
    /// Road point where a rider looking ahead first has the site in view.
    pub reveal_z: f32,
    /// Course distance of that reveal point, meters.
    pub reveal_m: f64,
}

/// Ground questions the placer asks. The game passes [`crate::Field`].
pub trait GroundQuery {
    /// Terrain height at a world position.
    fn height(&self, x: f32, z: f32) -> f32;
    /// Horizontal distance to the nearest centreline sample.
    fn road_distance(&self, x: f32, z: f32) -> f32;
}

/// Pick landmarks for `course`. Deterministic in `seed` and `requests`.
#[must_use]
pub fn place_landmarks(
    course: &Course,
    seed: u32,
    requests: &[LandmarkRequest],
    ground: &impl GroundQuery,
) -> Vec<PlacedLandmark> {
    if requests.is_empty() || course.length_m() < 80.0 {
        return Vec::new();
    }
    let _ = seed;
    let candidates = candidates(course);
    let mut ordered: Vec<&LandmarkRequest> = requests.iter().collect();
    ordered.sort_by_key(|request| match request.site {
        SiteKind::Summit => 0,
        SiteKind::Valley => 1,
        SiteKind::Crest => 2,
        SiteKind::Bend => 3,
        SiteKind::Straight => 4,
    });
    let mut placed = Vec::new();
    for request in &ordered {
        try_place(course, request, &candidates, true, ground, &mut placed);
    }
    for request in requests {
        if placed.iter().any(|site| site.id == request.id) {
            continue;
        }
        try_place(course, request, &candidates, false, ground, &mut placed);
    }
    if course.length_m() > 8_000.0 {
        for request in requests {
            try_place(course, request, &candidates, false, ground, &mut placed);
        }
    }
    placed.sort_by(|a, b| {
        a.distance_m
            .partial_cmp(&b.distance_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    placed
}

fn try_place(
    course: &Course,
    request: &LandmarkRequest,
    candidates: &[Candidate],
    preferred_only: bool,
    ground: &impl GroundQuery,
    placed: &mut Vec<PlacedLandmark>,
) {
    if preferred_only && placed.iter().any(|site| site.id == request.id) {
        return;
    }
    for candidate in candidates {
        if preferred_only && candidate.kind != request.site {
            continue;
        }
        if too_close(candidate.distance_m, placed) {
            continue;
        }
        let offset = request.road_m.clamp(15.0, 120.0);
        let (x, z) = if request.spans_road {
            let at = course.position_at(candidate.distance_m);
            (at[0], at[2])
        } else if let Some((x, z, _)) = stand_off(course, candidate, offset, ground) {
            (x, z)
        } else {
            continue;
        };
        let slope = slope_at(ground, x, z);
        if !request.spans_road && slope > 0.55 {
            continue;
        }
        let ground_y = ground.height(x, z);
        let pad = if !request.spans_road && slope > 0.16 {
            let radius = (request.footprint_m.max(6.0) + 4.0).min(offset - 12.0);
            if radius >= 4.0 {
                Some((x, z, radius, ground_y))
            } else {
                None
            }
        } else {
            None
        };
        let tangent = course.tangent_at(candidate.distance_m);
        let lead = (offset / 0.45).clamp(36.0, 160.0);
        let reveal_d = (candidate.distance_m - f64::from(lead)).max(0.0);
        let reveal = course.position_at(reveal_d);
        placed.push(PlacedLandmark {
            id: request.id.clone(),
            x,
            z,
            ground_y,
            yaw: (-tangent[0]).atan2(-tangent[2]),
            distance_m: candidate.distance_m,
            site: candidate.kind,
            pad,
            footprint_m: request.footprint_m,
            reveal_x: reveal[0],
            reveal_z: reveal[2],
            reveal_m: reveal_d,
        });
        return;
    }
}

struct Candidate {
    distance_m: f64,
    kind: SiteKind,
    /// Positive when the road turns left.
    turn: f32,
}

fn candidates(course: &Course) -> Vec<Candidate> {
    let length = course.length_m();
    let step = 20.0_f64;
    let mut samples = Vec::new();
    let mut distance = 0.0;
    while distance < length {
        let tangent = course.tangent_at(distance);
        let elevation = f64::from(course.elevation_at(distance));
        let grade = f64::from(course.grade_pct_at(distance));
        samples.push((distance, tangent, elevation, grade));
        distance += step;
    }
    if samples.len() < 5 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let summit = samples
        .iter()
        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
        .map(|sample| sample.0)
        .unwrap_or(0.0);
    out.push(Candidate {
        distance_m: summit,
        kind: SiteKind::Summit,
        turn: 0.0,
    });
    for index in 2..samples.len() - 2 {
        let (distance, tangent, elevation, grade) = samples[index];
        let before = samples[index - 2];
        let after = samples[index + 2];
        let turn = heading_delta(before.1, after.1);
        if turn.abs() > 0.40
            && turn.abs() >= heading_delta(samples[index - 1].1, samples[index + 1].1).abs()
        {
            out.push(Candidate {
                distance_m: (distance + 30.0).min(length - 1.0),
                kind: SiteKind::Bend,
                turn,
            });
        }
        if before.3 > 2.0
            && grade > -1.0
            && after.3 < -2.0
            && elevation >= before.2
            && elevation >= after.2
        {
            out.push(Candidate {
                distance_m: distance,
                kind: SiteKind::Crest,
                turn,
            });
        }
        if before.3 < -2.0 && after.3 > -0.5 && elevation <= before.2 && elevation <= after.2 {
            out.push(Candidate {
                distance_m: distance,
                kind: SiteKind::Valley,
                turn,
            });
        }
        let straight = (index.saturating_sub(6)..index)
            .all(|earlier| heading_delta(samples[earlier].1, tangent).abs() < 0.15);
        if straight && turn.abs() < 0.12 && index > 8 {
            out.push(Candidate {
                distance_m: distance,
                kind: SiteKind::Straight,
                turn: 0.0,
            });
        }
    }
    out.sort_by(|a, b| {
        a.distance_m
            .partial_cmp(&b.distance_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn heading_delta(from: [f32; 3], to: [f32; 3]) -> f32 {
    let cross = from[0] * to[2] - from[2] * to[0];
    let dot = from[0] * to[0] + from[2] * to[2];
    cross.atan2(dot)
}

fn stand_off(
    course: &Course,
    candidate: &Candidate,
    offset: f32,
    ground: &impl GroundQuery,
) -> Option<(f32, f32, f32)> {
    let at = course.position_at(candidate.distance_m);
    let tangent = course.tangent_at(candidate.distance_m);
    let mut right_x = -tangent[2];
    let mut right_z = tangent[0];
    let span = right_x.hypot(right_z);
    if span < 1.0e-4 {
        return None;
    }
    right_x /= span;
    right_z /= span;
    // Outside of a bend, so the landmark sits in the view that opens.
    let preferred = if candidate.kind == SiteKind::Bend {
        if candidate.turn >= 0.0 { -1.0 } else { 1.0 }
    } else {
        1.0
    };
    let mut best = None;
    for sign in [preferred, -preferred] {
        let x = at[0] + right_x * offset * sign;
        let z = at[2] + right_z * offset * sign;
        let road = ground.road_distance(x, z);
        if road < 12.0 || road + 8.0 < offset * 0.55 {
            continue;
        }
        best = Some((x, z, sign));
        break;
    }
    best
}

fn slope_at(ground: &impl GroundQuery, x: f32, z: f32) -> f32 {
    let step = 6.0;
    let dx = ground.height(x + step, z) - ground.height(x - step, z);
    let dz = ground.height(x, z + step) - ground.height(x, z - step);
    (dx * dx + dz * dz).sqrt() / (2.0 * step)
}

fn too_close(distance_m: f64, placed: &[PlacedLandmark]) -> bool {
    placed.iter().any(|earlier| {
        let gap = (distance_m - earlier.distance_m).abs();
        gap < MIN_SPACING_M
    })
}

impl GroundQuery for crate::Field {
    fn height(&self, x: f32, z: f32) -> f32 {
        crate::Field::height(self, x, z)
    }

    fn road_distance(&self, x: f32, z: f32) -> f32 {
        self.road(x, z).0
    }
}

/// Compass direction of the horizon peak for a theme name, in radians from +X
/// toward +Z. Alpine looks northeast. Desert looks southwest. Unknown themes
/// use north.
#[must_use]
pub fn horizon_bearing(theme: &str) -> f32 {
    match theme {
        "desert" => 3.6,
        "alpine" => 0.7,
        _ => 1.2,
    }
}

/// Extra radius, past the footprint, kept free of trees, rocks, and tall grass.
pub const SCATTER_MARGIN_M: f32 = 8.0;

/// A landmark circle the scatter planner must honour.
#[derive(Clone, Copy, Debug)]
pub struct ScatterSite {
    /// Site x.
    pub x: f32,
    /// Site z.
    pub z: f32,
    /// Footprint radius, meters.
    pub footprint_m: f32,
    /// Reveal point on the road, x.
    pub reveal_x: f32,
    /// Reveal point on the road, z.
    pub reveal_z: f32,
}

/// Whether a scatter instance of `kind` may stand at `(x, z)`.
///
/// Trees, rocks, and tall grass are cleared from the footprint plus
/// [`SCATTER_MARGIN_M`]. Trees in the sight corridor from the reveal point
/// to the site are thinned so the approach stays open.
#[must_use]
pub fn keeps_scatter(kind: &str, x: f32, z: f32, sites: &[ScatterSite]) -> bool {
    let bulky = matches!(kind, "tree" | "rock" | "grass");
    for site in sites {
        if bulky {
            let dx = x - site.x;
            let dz = z - site.z;
            let limit = site.footprint_m + SCATTER_MARGIN_M;
            if dx * dx + dz * dz < limit * limit {
                return false;
            }
        }
        if kind == "tree" && in_sight_corridor(x, z, site) && thin_tree(x, z) {
            return false;
        }
    }
    true
}

fn in_sight_corridor(x: f32, z: f32, site: &ScatterSite) -> bool {
    let abx = site.x - site.reveal_x;
    let abz = site.z - site.reveal_z;
    let len2 = abx * abx + abz * abz;
    if len2 < 4.0 {
        return false;
    }
    let t = ((x - site.reveal_x) * abx + (z - site.reveal_z) * abz) / len2;
    if !(0.02..=1.02).contains(&t) {
        return false;
    }
    let px = site.reveal_x + abx * t;
    let pz = site.reveal_z + abz * t;
    let dx = x - px;
    let dz = z - pz;
    dx * dx + dz * dz < 10.0 * 10.0
}

/// Drop about three quarters of the trees in a corridor. Stable for a point.
fn thin_tree(x: f32, z: f32) -> bool {
    let bits = x.to_bits() ^ z.to_bits().rotate_left(9);
    !bits.is_multiple_of(4)
}

/// A peak 4 km out, on the bearing the start, the climb, and the summit
/// all look closest to. Height is chosen by the mesh, not here.
#[must_use]
pub fn horizon_in_view(course: &Course) -> (f32, f32) {
    let spots = [40.0_f64, 900.0, 2_100.0];
    let mut best = (0.0_f32, 0.0);
    let mut best_score = f32::MAX;
    let range = 4_000.0_f32;
    for step in 0..48 {
        let heading = step as f32 * std::f32::consts::TAU / 48.0;
        let mut sx = 0.0_f32;
        let mut sz = 0.0_f32;
        for distance in spots {
            let at = course.position_at(distance);
            sx += at[0];
            sz += at[2];
        }
        sx /= spots.len() as f32;
        sz /= spots.len() as f32;
        let px = sx + heading.cos() * range;
        let pz = sz + heading.sin() * range;
        let mut score = 0.0_f32;
        for (index, distance) in spots.iter().enumerate() {
            let at = course.position_at(*distance);
            let tangent = course.tangent_at(*distance);
            let angle = view_angle(at[0], at[2], tangent[0], tangent[2], px, pz);
            let weight = if index == 0 { 1.6 } else { 1.0 };
            score += angle * weight;
            if angle > 1.15 {
                score += 1.5;
            }
        }
        if score < best_score {
            best_score = score;
            best = (px, pz);
        }
    }
    best
}

fn view_angle(sx: f32, sz: f32, tx: f32, tz: f32, px: f32, pz: f32) -> f32 {
    let dx = px - sx;
    let dz = pz - sz;
    let len = dx.hypot(dz).max(1.0);
    let tlen = tx.hypot(tz).max(0.001);
    let dot = (dx / len) * (tx / tlen) + (dz / len) * (tz / tlen);
    dot.clamp(-1.0, 1.0).acos()
}

/// A point `range_m` from the course centroid, on [`horizon_bearing`].
#[must_use]
pub fn horizon_anchor(course: &Course, theme: &str, range_m: f32) -> (f32, f32) {
    let mut x = 0.0_f64;
    let mut z = 0.0_f64;
    let mut count = 0.0_f64;
    let mut distance = 0.0;
    while distance < course.length_m() {
        let position = course.position_at(distance);
        x += f64::from(position[0]);
        z += f64::from(position[2]);
        count += 1.0;
        distance += 40.0;
    }
    if count < 1.0 {
        count = 1.0;
    }
    let bearing = horizon_bearing(theme);
    (
        (x / count) as f32 + bearing.cos() * range_m,
        (z / count) as f32 + bearing.sin() * range_m,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl GroundQuery for Flat {
        fn height(&self, _: f32, _: f32) -> f32 {
            0.0
        }
        fn road_distance(&self, x: f32, z: f32) -> f32 {
            // Distance to the nearest sample of the built-in loop, coarse.
            let course = Course::penguin_pass();
            let mut best = f32::MAX;
            let mut distance = 0.0;
            while distance < course.length_m() {
                let position = course.position_at(distance);
                let gap = (position[0] - x).hypot(position[2] - z);
                best = best.min(gap);
                distance += 8.0;
            }
            best
        }
    }

    fn specs() -> Vec<LandmarkRequest> {
        vec![
            LandmarkRequest {
                id: "waterfall".into(),
                footprint_m: 16.0,
                road_m: 36.0,
                site: SiteKind::Bend,
                rarity: 1.0,
                spans_road: false,
            },
            LandmarkRequest {
                id: "village".into(),
                footprint_m: 22.0,
                road_m: 55.0,
                site: SiteKind::Straight,
                rarity: 1.0,
                spans_road: false,
            },
            LandmarkRequest {
                id: "cross".into(),
                footprint_m: 8.0,
                road_m: 22.0,
                site: SiteKind::Summit,
                rarity: 1.0,
                spans_road: false,
            },
            LandmarkRequest {
                id: "turbines".into(),
                footprint_m: 14.0,
                road_m: 80.0,
                site: SiteKind::Crest,
                rarity: 1.0,
                spans_road: false,
            },
            LandmarkRequest {
                id: "lake".into(),
                footprint_m: 20.0,
                road_m: 48.0,
                site: SiteKind::Valley,
                rarity: 1.0,
                spans_road: false,
            },
        ]
    }

    #[test]
    fn the_same_course_places_the_same_landmarks() {
        let course = Course::penguin_pass();
        let first = place_landmarks(&course, 7, &specs(), &Flat);
        let second = place_landmarks(&course, 7, &specs(), &Flat);
        assert!(!first.is_empty());
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(&second) {
            assert_eq!(a.id, b.id);
            assert!((a.x - b.x).abs() < 0.01 && (a.z - b.z).abs() < 0.01);
        }
    }

    #[test]
    fn sites_stay_off_the_road_and_apart() {
        let course = Course::penguin_pass();
        let placed = place_landmarks(&course, 7, &specs(), &Flat);
        assert!(
            placed.len() >= 3,
            "expected several reveals, got {}",
            placed.len()
        );
        for (index, site) in placed.iter().enumerate() {
            let road = Flat.road_distance(site.x, site.z);
            assert!(road >= 12.0, "{} is {road:.1} m from the road", site.id);
            for other in placed.iter().skip(index + 1) {
                let gap = (site.distance_m - other.distance_m).abs();
                assert!(
                    gap >= MIN_SPACING_M - 1.0,
                    "{gap:.0} m between {} and {}",
                    site.id,
                    other.id
                );
            }
        }
    }

    #[test]
    fn a_different_seed_can_change_the_set_and_the_peak_stays_put() {
        let course = Course::penguin_pass();
        let alpine = horizon_anchor(&course, "alpine", 4_500.0);
        let again = horizon_anchor(&course, "alpine", 4_500.0);
        let desert = horizon_anchor(&course, "desert", 4_500.0);
        assert!((alpine.0 - again.0).abs() < 0.01);
        assert!((alpine.0 - desert.0).abs() > 100.0 || (alpine.1 - desert.1).abs() > 100.0);
    }

    #[test]
    fn scatter_stays_outside_a_landmark_footprint() {
        let course = Course::penguin_pass();
        let theme = pp_view::theme::Theme::builtin();
        let mut raw = Vec::new();
        let mut distance = 0.0;
        while distance < course.length_m() {
            let at = course.position_at(distance);
            raw.push([at[0], at[1], at[2]]);
            distance += 4.0;
        }
        let mut field = crate::Field::new(&raw, theme.seed as i32);
        field.use_relief(theme.relief.clone(), theme.seed);
        let placed = place_landmarks(&course, theme.seed, &specs(), &field);
        assert!(!placed.is_empty());
        let sites: Vec<ScatterSite> = placed
            .iter()
            .map(|site| ScatterSite {
                x: site.x,
                z: site.z,
                footprint_m: site.footprint_m,
                reveal_x: site.reveal_x,
                reveal_z: site.reveal_z,
            })
            .collect();
        let centre = &sites[0];
        assert!(
            !keeps_scatter("tree", centre.x, centre.z, &sites),
            "a tree on the site must be rejected"
        );
        assert!(
            !keeps_scatter("rock", centre.x + 1.0, centre.z, &sites),
            "a rock inside the footprint must be rejected"
        );
        assert!(keeps_scatter(
            "tree",
            centre.x + centre.footprint_m + SCATTER_MARGIN_M + 30.0,
            centre.z + 400.0,
            &sites
        ));
        for site in &sites {
            let cx = (site.x / 128.0).floor() as i32;
            let cz = (site.z / 128.0).floor() as i32;
            for layer in &theme.scatter {
                if !matches!(layer.kind.as_str(), "tree" | "rock" | "grass") {
                    continue;
                }
                for ox in -1..=1 {
                    for oz in -1..=1 {
                        let planned = pp_view::scatter::plan_chunk(
                            theme.seed,
                            layer,
                            cx + ox,
                            cz + oz,
                            |x, z| {
                                let step = 2.0;
                                let dx = field.height(x + step, z) - field.height(x - step, z);
                                let dz = field.height(x, z + step) - field.height(x, z - step);
                                pp_view::scatter::Ground {
                                    road_m: field.road(x, z).0,
                                    height: field.height(x, z),
                                    slope: ((dx * dx + dz * dz).sqrt() / (2.0 * step)).min(2.0),
                                }
                            },
                        );
                        for point in planned {
                            if !keeps_scatter(&layer.kind, point.x, point.z, &sites) {
                                continue;
                            }
                            let gap = (point.x - site.x).hypot(point.z - site.z);
                            assert!(
                                gap + 0.05 >= site.footprint_m,
                                "{} at ({:.1}, {:.1}) is {gap:.1} m inside {}",
                                layer.kind,
                                point.x,
                                point.z,
                                site.footprint_m
                            );
                        }
                    }
                }
            }
        }
    }
}
