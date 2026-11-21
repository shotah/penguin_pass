//! The bicycle: frame, fork and bars, wheels, crankset, pedals.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use glam::{Quat, Vec2, Vec3};

use crate::geo::{
    Cap, Geo, PartBuilder, Ring, block, capsule, circle, cylinder, extrude, extrude_ring, lathe,
    loft, path_frames, rounded_rect, spline, strut, sweep, torus, tube, turned,
};
use crate::mesh::{Part, PartId};
use crate::paint::{self, styled};
use crate::rig::{CHAINLINE, CRANK_LENGTH, PEDAL_X, Rig, TYRE_RADIUS, WHEEL_RADIUS};
use crate::style::{RiderStyle, StyleColor};

/// Crease angle for machined and moulded parts, in degrees.
const HARD: f32 = 35.0;
/// Crease angle for rounded tubes, in degrees.
const SOFT: f32 = 50.0;

/// Segments round a wheel.
const WHEEL_SEGS: usize = 36;
/// Radius of the rim's outer edge, in meters.
const RIM_OUTER: f32 = WHEEL_RADIUS - TYRE_RADIUS - 0.006;
/// Rim depth, in meters.
const RIM_DEPTH: f32 = 0.045;
/// Half the rim's outer width, in meters.
const RIM_HALF_WIDTH: f32 = 0.0125;
/// Number of sprockets modelled on the cassette.
const COGS: usize = 7;
/// Radius of the largest and the smallest sprocket, in meters.
const COG_RADII: (f32, f32) = (0.058, 0.024);
/// Sideways position of the inner face of the largest sprocket.
const CASSETTE_INNER: f32 = 0.0278;
/// Sideways pitch between sprockets, in meters.
const COG_PITCH: f32 = 0.0045;
/// Which sprocket the chain runs on, counted from the largest.
const CHAIN_COG: usize = 3;
/// Radius of the chain's centreline on the big chainring, in meters.
const CHAINRING_RADIUS: f32 = 0.1005;
/// Radius of the chain's centreline on a derailleur pulley, in meters.
const PULLEY_RADIUS: f32 = 0.0215;

/// Turn a shape made about the Y axis so its axis is X.
fn about_x(geo: Geo) -> Geo {
    geo.rotated(Quat::from_rotation_z(-FRAC_PI_2))
}

fn cog_radius(k: usize) -> f32 {
    COG_RADII.0 + (COG_RADII.1 - COG_RADII.0) * k as f32 / (COGS - 1) as f32
}

/// Upper derailleur pulley centre, in model space.
fn guide_pulley(rig: &Rig) -> Vec3 {
    rig.rear_axle + Vec3::new(CHAINLINE, -0.078, -0.012)
}

/// Lower derailleur pulley centre, in model space.
fn tension_pulley(rig: &Rig) -> Vec3 {
    guide_pulley(rig) + Vec3::new(0.0, -0.062, -0.034)
}

/// Build one wheel about its own axle: the axle is the X axis and the wheel
/// lies in the YZ plane.
pub(crate) fn wheel(style: &RiderStyle, id: PartId) -> Part {
    let rear = id == PartId::RearWheel;
    let mut part = PartBuilder::new(id.name());
    let tread = part.slot(styled(style, StyleColor::Tyre, 0.9, 0.0));
    let wall = part.slot(styled(style, StyleColor::TyreWall, 0.85, 0.0));
    let rim = part.slot(paint::CARBON);
    let decal = part.slot(paint::DECAL);
    let steel = part.slot(paint::STEEL);
    let dark = part.slot(paint::GUNMETAL);

    // Tyre: an arc of a circle, closed by a chord hidden inside the rim.
    let centre = WHEEL_RADIUS - TYRE_RADIUS;
    const ARC: usize = 8;
    let arc_half = 130.0_f32.to_radians();
    let profile: Vec<Vec2> = (0..=ARC)
        .map(|j| {
            let phi = -arc_half + 2.0 * arc_half * j as f32 / ARC as f32;
            Vec2::new(
                centre + TYRE_RADIUS * phi.cos(),
                TYRE_RADIUS * 1.04 * phi.sin(),
            )
        })
        .collect();
    let tyre = lathe(&profile, true, WHEEL_SEGS, |j, _| {
        if (2..6).contains(&j) { tread } else { wall }
    });
    part.add(&about_x(tyre), 60.0);

    // Rim: flat brake track, then a V down to a rounded nose. Two decal
    // patches on each side make the spin visible.
    let ro = RIM_OUTER;
    let ri = RIM_OUTER - RIM_DEPTH;
    let w = RIM_HALF_WIDTH;
    let profile = [
        Vec2::new(ro, -w),
        Vec2::new(ro, w),
        Vec2::new(ro - 0.014, w),
        Vec2::new(ro - 0.034, 0.0075),
        Vec2::new(ri + 0.003, 0.0035),
        Vec2::new(ri, 0.0),
        Vec2::new(ri + 0.003, -0.0035),
        Vec2::new(ro - 0.034, -0.0075),
        Vec2::new(ro - 0.014, -w),
    ];
    let rim_geo = lathe(&profile, true, WHEEL_SEGS, |j, k| {
        let on_wall = j == 2 || j == 7;
        let patch = (6..12).contains(&k) || (24..30).contains(&k);
        if on_wall && patch { decal } else { rim }
    });
    part.add(&about_x(rim_geo), 30.0);

    // Hub: a spindle with two flanges.
    let (flange_l, flange_r, half) = if rear {
        (-0.030, 0.018, 0.066)
    } else {
        (-0.030, 0.030, 0.050)
    };
    let profile = [
        Vec2::new(0.0, -half),
        Vec2::new(0.008, -half),
        Vec2::new(0.009, flange_l - 0.009),
        Vec2::new(0.021, flange_l - 0.003),
        Vec2::new(0.021, flange_l + 0.003),
        Vec2::new(0.011, flange_l + 0.010),
        Vec2::new(0.011, flange_r - 0.010),
        Vec2::new(0.021, flange_r - 0.003),
        Vec2::new(0.021, flange_r + 0.003),
        Vec2::new(0.009, flange_r + 0.009),
        Vec2::new(0.008, half),
        Vec2::new(0.0, half),
    ];
    part.add(&about_x(turned(&profile, false, 12, dark)), HARD);

    // Spokes, alternating flanges, laced a little off radial.
    const SPOKES: usize = 20;
    for i in 0..SPOKES {
        let at_rim = (i as f32 + 0.5) * TAU / SPOKES as f32;
        let lead = if (i / 2) % 2 == 0 { 1.0 } else { -1.0 };
        let at_hub = at_rim + lead;
        let flange = if i % 2 == 0 { flange_l } else { flange_r };
        let hub_point = Vec3::new(flange, 0.0195 * at_hub.cos(), 0.0195 * at_hub.sin());
        let rim_r = ri + 0.0015;
        let rim_point = Vec3::new(0.0, rim_r * at_rim.cos(), rim_r * at_rim.sin());
        let r = Vec2::splat(0.0013);
        part.add(&strut(hub_point, rim_point, r, r, Vec3::X, 4, steel), HARD);
    }

    // Brake rotor on the left.
    let x = flange_l - 0.013;
    let rotor = [
        Vec2::new(0.030, x - 0.001),
        Vec2::new(0.078, x - 0.001),
        Vec2::new(0.078, x + 0.001),
        Vec2::new(0.030, x + 0.001),
    ];
    part.add(&about_x(turned(&rotor, true, 20, steel)), HARD);

    // Valve stem, pointing at the hub from the top of the rim.
    part.add(&cylinder(0.0028, ri - 0.030, ri + 0.002, 6, steel), HARD);

    if rear {
        // Cassette: a staircase of sprockets, largest inboard.
        let mut profile = vec![Vec2::new(0.0, CASSETTE_INNER - 0.006)];
        profile.push(Vec2::new(0.030, CASSETTE_INNER - 0.006));
        profile.push(Vec2::new(0.030, CASSETTE_INNER));
        for k in 0..COGS {
            let a = CASSETTE_INNER + COG_PITCH * k as f32;
            profile.push(Vec2::new(cog_radius(k), a));
            profile.push(Vec2::new(cog_radius(k), a + COG_PITCH));
        }
        let end = CASSETTE_INNER + COG_PITCH * COGS as f32;
        profile.push(Vec2::new(0.014, end));
        profile.push(Vec2::new(0.014, end + 0.004));
        profile.push(Vec2::new(0.0, end + 0.004));
        part.add(&about_x(turned(&profile, false, 20, steel)), HARD);
    }
    part.finish()
}

/// Build the frame with everything bolted to it.
pub(crate) fn frame(style: &RiderStyle, rig: &Rig) -> Part {
    let mut part = PartBuilder::new(PartId::Frame.name());
    let paint_slot = part.slot(styled(style, StyleColor::Frame, 0.32, 0.25));
    let component = part.slot(paint::COMPONENT);
    let rubber = part.slot(paint::RUBBER);
    let steel = part.slot(paint::STEEL);
    let dark = part.slot(paint::GUNMETAL);
    let chain_slot = part.slot(paint::CHAIN);
    let bottle = part.slot(paint::BOTTLE);
    let accent = part.slot(styled(style, StyleColor::JerseyAccent, 0.45, 0.0));
    let light = part.slot(paint::TAIL_LIGHT);
    let panel = part.slot(paint::DECAL);

    let bb = rig.bb;
    let up = Vec3::Y;
    let head_low = rig.head_bottom + rig.steer_axis * 0.042;
    let head_high = rig.head_top - rig.steer_axis * 0.032;
    let seat_join = rig.seat_top - rig.seat_dir * 0.030;

    // Main triangle.
    part.add(
        &strut(
            rig.head_bottom,
            rig.head_top,
            Vec2::splat(0.0275),
            Vec2::splat(0.0235),
            -Vec3::Z,
            14,
            paint_slot,
        ),
        SOFT,
    );
    // The down tube carries a white panel, as a painted logo band would.
    let down_tube: Vec<Vec3> = [0.0, 0.34, 0.74, 1.0]
        .iter()
        .map(|t| bb.lerp(head_low, *t))
        .collect();
    part.add(
        &sweep(
            &down_tube,
            up,
            |i| Vec2::new(0.029, 0.025).lerp(Vec2::new(0.026, 0.0235), [0.0, 0.34, 0.74, 1.0][i]),
            12,
            Cap::Flat,
            Cap::Flat,
            |row, _| if row == 1 { panel } else { paint_slot },
        ),
        SOFT,
    );
    part.add(
        &strut(
            bb,
            rig.seat_top,
            Vec2::new(0.019, 0.021),
            Vec2::new(0.0165, 0.018),
            Vec3::Z,
            12,
            paint_slot,
        ),
        SOFT,
    );
    part.add(
        &strut(
            seat_join,
            head_high,
            Vec2::new(0.018, 0.0165),
            Vec2::new(0.0225, 0.0195),
            up,
            12,
            paint_slot,
        ),
        SOFT,
    );
    part.add(
        &about_x(cylinder(0.0275, -0.036, 0.036, 14, paint_slot)).translated(bb),
        SOFT,
    );

    // Rear triangle.
    for side in [-1.0_f32, 1.0] {
        let dropout = rig.rear_axle + Vec3::new(side * 0.066, 0.0, 0.0);
        let stay = [
            bb + Vec3::new(side * 0.024, 0.0, 0.012),
            bb + Vec3::new(side * 0.027, 0.012, 0.130),
            bb + Vec3::new(side * 0.050, 0.040, 0.280),
            dropout + Vec3::new(0.0, -0.004, -0.006),
        ];
        let path = spline(&stay, 3);
        let last = (path.len() - 1) as f32;
        let chainstay = sweep(
            &path,
            up,
            |i| Vec2::new(0.0095, 0.0165).lerp(Vec2::new(0.0075, 0.0095), i as f32 / last),
            8,
            Cap::Flat,
            Cap::Flat,
            |_, _| paint_slot,
        );
        part.add(&chainstay, SOFT);
        let seatstay = strut(
            seat_join - rig.seat_dir * 0.075 + Vec3::new(side * 0.013, 0.0, 0.004),
            dropout + Vec3::new(0.0, 0.010, -0.002),
            Vec2::new(0.008, 0.0095),
            Vec2::new(0.007, 0.008),
            up,
            8,
            paint_slot,
        );
        part.add(&seatstay, SOFT);
        part.add(
            &about_x(cylinder(0.015, -0.007, 0.007, 10, paint_slot)).translated(dropout),
            HARD,
        );
    }

    // Seatpost with its collar.
    let post_top = rig.saddle_top - rig.seat_dir * 0.046;
    let r = Vec2::splat(0.0136);
    part.add(
        &strut(
            rig.seat_top - rig.seat_dir * 0.02,
            post_top,
            r,
            r,
            Vec3::Z,
            12,
            component,
        ),
        SOFT,
    );
    let r = Vec2::splat(0.0205);
    part.add(
        &strut(
            rig.seat_top - rig.seat_dir * 0.012,
            rig.seat_top + rig.seat_dir * 0.006,
            r,
            r,
            Vec3::Z,
            12,
            component,
        ),
        HARD,
    );

    // Saddle: a narrow nose widening to the wings, with a kicked-up tail.
    let stations: [(f32, f32, f32, f32); 7] = [
        (-0.130, 0.015, -0.005, 0.011),
        (-0.090, 0.021, -0.002, 0.013),
        (-0.030, 0.031, 0.000, 0.014),
        (0.030, 0.053, 0.000, 0.015),
        (0.075, 0.069, 0.003, 0.015),
        (0.106, 0.066, 0.008, 0.013),
        (0.124, 0.046, 0.011, 0.010),
    ];
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(z, half_width, top, thick)| Ring {
            c: rig.saddle_top + Vec3::new(0.0, top - thick, z),
            u: Vec3::X * half_width,
            v: Vec3::Y * thick,
            vn: 0.75,
            e: [2.6, 2.2],
            phase: 0.0,
        })
        .collect();
    part.add(
        &loft(&rings, 14, Cap::Round(0.008), Cap::Round(0.006), |_, _| {
            rubber
        }),
        SOFT,
    );
    for side in [-1.0_f32, 1.0] {
        let rail = [
            Vec3::new(side * 0.010, -0.014, -0.100),
            Vec3::new(side * 0.019, -0.038, -0.060),
            Vec3::new(side * 0.023, -0.038, 0.035),
            Vec3::new(side * 0.034, -0.016, 0.092),
        ];
        let path: Vec<Vec3> = spline(&rail, 3)
            .into_iter()
            .map(|p| p + rig.saddle_top)
            .collect();
        part.add(&tube(&path, 0.0036, 5, steel), SOFT);
    }
    part.add(
        &block(Vec3::new(0.021, 0.011, 0.024), 4.0, 12, component)
            .translated(rig.saddle_top + Vec3::new(0.0, -0.041, -0.010)),
        HARD,
    );

    // Tail light on the seatpost, facing the chase camera.
    let light_at = rig.seat_top + rig.seat_dir * 0.105;
    part.add(
        &extrude(
            &rounded_rect(Vec2::new(0.014, 0.024), 0.007, 3),
            -0.008,
            0.008,
            0.003,
            light,
        )
        .rotated(Quat::from_rotation_arc(Vec3::Y, rig.seat_dir))
        .translated(light_at + Vec3::new(0.0, -0.004, 0.019)),
        HARD,
    );

    // Bottle and cage on the down tube.
    let down = (head_low - bb).normalize();
    let inward = Vec3::new(0.0, -down.z, down.y);
    let to_tube = Quat::from_rotation_arc(Vec3::Y, down);
    let bottle_base = bb + down * 0.215 + inward * (0.0235 + 0.0425);
    let profile = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.033, 0.0),
        Vec2::new(0.0365, 0.006),
        Vec2::new(0.0365, 0.128),
        Vec2::new(0.031, 0.150),
        Vec2::new(0.022, 0.160),
        Vec2::new(0.022, 0.178),
        Vec2::new(0.013, 0.184),
        Vec2::new(0.013, 0.198),
        Vec2::new(0.0, 0.198),
    ];
    let bottle_geo = lathe(
        &profile,
        false,
        12,
        |j, _| if j >= 4 { accent } else { bottle },
    );
    part.add(&bottle_geo.rotated(to_tube).translated(bottle_base), HARD);
    for y in [0.030, 0.100] {
        part.add(
            &torus(0.0385, 0.0032, 12, 4, component)
                .translated(Vec3::Y * y)
                .rotated(to_tube)
                .translated(bottle_base),
            SOFT,
        );
    }
    let r = Vec2::new(0.008, 0.004);
    part.add(
        &strut(
            bottle_base - inward * 0.039 - down * 0.006,
            bottle_base - inward * 0.039 + down * 0.135,
            r,
            r,
            inward,
            6,
            component,
        ),
        HARD,
    );

    // Rear derailleur: hanger, body, cage with two pulleys.
    let guide = guide_pulley(rig);
    let tension = tension_pulley(rig);
    let hanger_top = rig.rear_axle + Vec3::new(0.070, -0.004, 0.004);
    let hanger_low = rig.rear_axle + Vec3::new(0.072, -0.036, 0.010);
    let r = Vec2::new(0.004, 0.009);
    part.add(&strut(hanger_top, hanger_low, r, r, Vec3::Z, 6, dark), HARD);
    part.add(
        &strut(
            hanger_low + Vec3::new(0.004, 0.006, 0.0),
            guide + Vec3::new(0.022, 0.004, 0.004),
            Vec2::new(0.010, 0.014),
            Vec2::new(0.009, 0.012),
            Vec3::Z,
            8,
            component,
        ),
        HARD,
    );
    for (centre, slot) in [(guide, dark), (tension, dark)] {
        part.add(
            &about_x(cylinder(PULLEY_RADIUS - 0.004, -0.0035, 0.0035, 10, slot)).translated(centre),
            HARD,
        );
    }
    for x in [-0.0075_f32, 0.0085] {
        let offset = Vec3::X * x;
        let lean = (tension - guide).normalize();
        let r = Vec2::new(0.0015, 0.0105);
        part.add(
            &strut(
                guide - lean * 0.012 + offset,
                tension + lean * 0.012 + offset,
                r,
                r,
                Vec3::Z,
                6,
                component,
            ),
            HARD,
        );
    }

    // Front derailleur cage over the big ring, clamped to the seat tube.
    let cage_at = bb + Vec3::new(CHAINLINE + 0.002, 0.123, -0.010);
    part.add(
        &block(Vec3::new(0.0075, 0.011, 0.034), 5.0, 8, dark).translated(cage_at),
        HARD,
    );
    let r = Vec2::new(0.007, 0.010);
    part.add(
        &strut(
            cage_at + Vec3::new(-0.006, 0.006, 0.028),
            bb + rig.seat_dir * 0.135,
            r,
            r,
            Vec3::Y,
            6,
            dark,
        ),
        HARD,
    );

    // Rear brake caliper on the left chainstay.
    part.add(
        &block(Vec3::new(0.011, 0.016, 0.024), 4.0, 8, component)
            .rotated(Quat::from_rotation_x(0.5))
            .translated(rig.rear_axle + Vec3::new(-0.046, 0.030, -0.078)),
        HARD,
    );

    // Chain: one closed run round the big ring, both pulleys and a sprocket.
    let cog_r = cog_radius(CHAIN_COG) + 0.003;
    let wheels = [
        (flat(bb), CHAINRING_RADIUS, -1.0),
        (flat(tension), PULLEY_RADIUS, -1.0),
        (flat(guide), PULLEY_RADIUS, 1.0),
        (flat(rig.rear_axle), cog_r, -1.0),
    ];
    let path: Vec<Vec3> = belt(&wheels)
        .into_iter()
        .map(|p| Vec3::new(CHAINLINE, p.y, -p.x))
        .collect();
    let frames = path_frames(&path, Vec3::X);
    let rings: Vec<Ring> = frames
        .iter()
        .zip(&path)
        .map(|(&(_, u, v), &c)| Ring {
            phase: 0.5,
            ..Ring::squircle(c, u * 0.0046, v * 0.0034, 40.0)
        })
        .collect();
    part.add(
        &loft(&rings, 4, Cap::Flat, Cap::Flat, |_, _| chain_slot),
        HARD,
    );

    part.finish()
}

/// A model-space point in the chain's plane: forward and up.
fn flat(p: Vec3) -> Vec2 {
    Vec2::new(-p.z, p.y)
}

/// Path of a belt round a ring of wheels, in a plane.
///
/// Each wheel is `(centre, radius, turn)` with `turn` +1 when the belt wraps
/// it counter-clockwise and -1 for clockwise. The path starts and ends at
/// the same point, half-way along the straight into the first wheel.
fn belt(wheels: &[(Vec2, f32, f32)]) -> Vec<Vec2> {
    let n = wheels.len();
    // Tangent from wheel i to the next: where it leaves and where it lands.
    let tangent = |i: usize| -> (Vec2, Vec2) {
        let (ca, ra, sa) = wheels[i];
        let (cb, rb, sb) = wheels[(i + 1) % n];
        let d = cb - ca;
        let (rho_a, rho_b) = (sa * ra, sb * rb);
        let phi = ((rho_a - rho_b) / d.length()).clamp(-1.0, 1.0).acos();
        let angle = d.y.atan2(d.x) - phi;
        let normal = Vec2::new(angle.cos(), angle.sin());
        (ca + normal * rho_a, cb + normal * rho_b)
    };
    let mut out = Vec::new();
    let (leave_last, land_first) = tangent(n - 1);
    let start = (leave_last + land_first) * 0.5;
    out.push(start);
    for (i, &(centre, radius, turn)) in wheels.iter().enumerate() {
        let (_, land) = tangent((i + n - 1) % n);
        let (leave, next_land) = tangent(i);
        let a0 = (land - centre).y.atan2((land - centre).x);
        let a1 = (leave - centre).y.atan2((leave - centre).x);
        let mut sweep_angle = (a1 - a0) * turn;
        while sweep_angle < 0.0 {
            sweep_angle += TAU;
        }
        let steps = (sweep_angle / 0.30).ceil().max(1.0) as usize;
        for s in 0..=steps {
            let a = a0 + turn * sweep_angle * s as f32 / steps as f32;
            out.push(centre + Vec2::new(a.cos(), a.sin()) * radius);
        }
        if i + 1 < n {
            // A mid-point keeps the straight from sagging in the sweep.
            out.push((leave + next_land) * 0.5);
        }
    }
    out.push(start);
    out
}

/// The handlebar's centreline for the right side, relative to the clamp.
fn bar_path() -> Vec<Vec3> {
    let points = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.070, 0.0, 0.0),
        Vec3::new(0.140, 0.0, 0.0),
        Vec3::new(0.178, 0.0, -0.006),
        Vec3::new(0.198, -0.002, -0.030),
        Vec3::new(0.200, -0.006, -0.066),
        Vec3::new(0.200, -0.030, -0.094),
        Vec3::new(0.201, -0.068, -0.090),
        Vec3::new(0.202, -0.100, -0.064),
        Vec3::new(0.204, -0.122, -0.022),
        Vec3::new(0.205, -0.128, 0.030),
    ];
    spline(&points, 3)
}

/// Build the fork, stem, bars, and brake hoods, with the bars straight.
pub(crate) fn fork_bars(style: &RiderStyle, rig: &Rig) -> Part {
    let mut part = PartBuilder::new(PartId::ForkBars.name());
    let paint_slot = part.slot(styled(style, StyleColor::Frame, 0.32, 0.25));
    let tape = part.slot(styled(style, StyleColor::BarTape, 0.9, 0.0));
    let component = part.slot(paint::COMPONENT);
    let rubber = part.slot(paint::RUBBER);
    let carbon = part.slot(paint::CARBON);
    let screen = part.slot(paint::SCREEN);
    let lever_slot = part.slot(paint::ALLOY);

    let axis = rig.steer_axis;
    let fwd = rig.steer_fwd;
    let to_axis = Quat::from_rotation_arc(Vec3::Y, axis);

    // Fork crown and blades.
    let crown = rig.head_bottom - axis * 0.017;
    part.add(
        &block(Vec3::new(0.054, 0.015, 0.024), 3.0, 14, paint_slot)
            .rotated(to_axis)
            .translated(crown),
        SOFT,
    );
    for side in [-1.0_f32, 1.0] {
        let top = crown + Vec3::new(side * 0.041, 0.0, 0.0);
        let tip = rig.front_axle + Vec3::new(side * 0.053, 0.0, 0.0);
        part.add(
            &strut(
                top,
                tip,
                Vec2::new(0.012, 0.021),
                Vec2::new(0.0085, 0.011),
                fwd,
                10,
                paint_slot,
            ),
            SOFT,
        );
        part.add(
            &about_x(cylinder(0.013, -0.006, 0.006, 10, paint_slot)).translated(tip),
            HARD,
        );
    }
    // Front brake caliper behind the left blade.
    part.add(
        &block(Vec3::new(0.011, 0.024, 0.014), 4.0, 8, component)
            .rotated(to_axis)
            .translated(rig.front_axle + Vec3::new(-0.046, 0.078, 0.036)),
        HARD,
    );

    // Headset cover, spacers, stem clamp, top cap: one turned stack.
    let clamp = (rig.stem_base - rig.head_top).length();
    let profile = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.0245, 0.0),
        Vec2::new(0.0185, 0.012),
        Vec2::new(0.0185, clamp - 0.021),
        Vec2::new(0.022, clamp - 0.021),
        Vec2::new(0.022, clamp + 0.021),
        Vec2::new(0.017, clamp + 0.021),
        Vec2::new(0.015, clamp + 0.026),
        Vec2::new(0.0, clamp + 0.026),
    ];
    part.add(
        &turned(&profile, false, 14, component)
            .rotated(to_axis)
            .translated(rig.head_top),
        HARD,
    );

    // Stem and faceplate.
    part.add(
        &strut(
            rig.stem_base,
            rig.bar_clamp,
            Vec2::new(0.0165, 0.0185),
            Vec2::new(0.0185, 0.0175),
            axis,
            10,
            component,
        ),
        SOFT,
    );
    part.add(
        &about_x(cylinder(0.0215, -0.022, 0.022, 12, component)).translated(rig.bar_clamp),
        HARD,
    );

    // Head unit on an out-front mount.
    let unit = rig.bar_clamp + Vec3::new(0.0, 0.014, -0.078);
    let r = Vec2::new(0.007, 0.005);
    part.add(
        &strut(
            rig.bar_clamp + Vec3::new(0.012, 0.0, -0.015),
            unit + Vec3::new(0.0, -0.008, 0.02),
            r,
            r,
            Vec3::Y,
            6,
            component,
        ),
        HARD,
    );
    // A bevelled slab lying flat, nose up a little, with the screen on top.
    let lay_flat = Quat::from_rotation_x(0.22 - FRAC_PI_2);
    part.add(
        &extrude(
            &rounded_rect(Vec2::new(0.027, 0.043), 0.007, 3),
            -0.0075,
            0.0075,
            0.002,
            component,
        )
        .rotated(lay_flat)
        .translated(unit),
        HARD,
    );
    part.add(
        &extrude(
            &rounded_rect(Vec2::new(0.021, 0.035), 0.004, 2),
            0.0070,
            0.0086,
            0.0,
            screen,
        )
        .rotated(lay_flat)
        .translated(unit),
        HARD,
    );

    // Handlebar: bare by the stem, taped from the tops to the drop ends.
    let path = bar_path();
    let bar = sweep(
        &path,
        Vec3::Y,
        |i| {
            let x = path[i].x;
            let r = if x < 0.045 {
                0.0158
            } else if x < 0.075 {
                0.0128
            } else {
                0.0155
            };
            Vec2::splat(r)
        },
        8,
        Cap::Flat,
        Cap::Round(0.006),
        |row, _| if path[row].x < 0.075 { carbon } else { tape },
    );
    // Brake hood: a rubber body rising to a horn, with the lever below.
    let hood_path = [
        Vec3::new(0.200, -0.004, -0.050),
        Vec3::new(0.200, 0.002, -0.090),
        Vec3::new(0.200, 0.010, -0.118),
        Vec3::new(0.200, 0.032, -0.140),
        Vec3::new(0.200, 0.054, -0.149),
    ];
    let hood_radii = [
        Vec2::new(0.0180, 0.021),
        Vec2::new(0.0180, 0.024),
        Vec2::new(0.0175, 0.023),
        Vec2::new(0.0160, 0.018),
        Vec2::new(0.0130, 0.012),
    ];
    let hood = sweep(
        &hood_path,
        Vec3::Y,
        |i| hood_radii[i],
        10,
        Cap::Round(0.010),
        Cap::Round(0.008),
        |_, _| rubber,
    );
    let lever_path = spline(
        &[
            Vec3::new(0.2015, 0.004, -0.146),
            Vec3::new(0.2030, -0.045, -0.152),
            Vec3::new(0.2060, -0.090, -0.138),
            Vec3::new(0.2080, -0.124, -0.116),
        ],
        2,
    );
    let last = (lever_path.len() - 1) as f32;
    let lever = sweep(
        &lever_path,
        Vec3::X,
        |i| Vec2::new(0.0085, 0.0085).lerp(Vec2::new(0.0050, 0.0060), i as f32 / last),
        6,
        Cap::Blunt(0.004),
        Cap::Blunt(0.004),
        |_, _| lever_slot,
    );
    for mirror in [false, true] {
        for geo in [&bar, &hood, &lever] {
            let mut geo = geo.clone();
            if mirror {
                geo = geo.mirrored_x();
            }
            part.add(&geo.translated(rig.bar_clamp), SOFT);
        }
    }
    part.finish()
}

/// Build the crankset about the bottom bracket: the axle is the X axis and
/// the right crank points to -Z.
pub(crate) fn crankset() -> Part {
    let mut part = PartBuilder::new(PartId::Crankset.name());
    let carbon = part.slot(paint::CARBON);
    let dark = part.slot(paint::GUNMETAL);
    let steel = part.slot(paint::STEEL);

    part.add(&about_x(cylinder(0.012, -0.060, 0.060, 10, dark)), HARD);

    // Right arm, from behind the axle out to the pedal eye.
    let length = CRANK_LENGTH;
    let stations: [(f32, f32, f32); 6] = [
        (-0.022, 0.012, 0.0065),
        (0.0, 0.021, 0.0085),
        (0.035, 0.0175, 0.0080),
        (length - 0.030, 0.0125, 0.0070),
        (length, 0.0135, 0.0070),
        (length + 0.013, 0.0085, 0.0060),
    ];
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(s, half_width, half_thick)| {
            let x = 0.059 + 0.009 * (s / length).clamp(0.0, 1.0);
            Ring::squircle(
                Vec3::new(x, 0.0, -s),
                Vec3::Y * half_width,
                Vec3::X * half_thick,
                3.0,
            )
        })
        .collect();
    let arm = loft(&rings, 10, Cap::Round(0.006), Cap::Round(0.005), |_, _| {
        carbon
    });
    part.add(&arm, SOFT);
    part.add(&arm.clone().rotated(Quat::from_rotation_y(PI)), SOFT);

    // Spider and chainrings on the right.
    part.add(
        &about_x(cylinder(0.030, CHAINLINE - 0.004, 0.058, 14, carbon)),
        HARD,
    );
    for k in 0..4 {
        let a = FRAC_PI_2 * (k as f32 + 0.5);
        let dir = Vec3::new(0.0, a.sin(), a.cos());
        part.add(
            &strut(
                Vec3::new(CHAINLINE + 0.004, 0.0, 0.0) + dir * 0.015,
                Vec3::new(CHAINLINE + 0.0025, 0.0, 0.0) + dir * 0.089,
                Vec2::new(0.012, 0.0035),
                Vec2::new(0.008, 0.0025),
                Vec3::X,
                6,
                carbon,
            ),
            HARD,
        );
    }
    const TEETH: usize = 26;
    let outer: Vec<Vec2> = (0..TEETH * 2)
        .map(|i| {
            let a = TAU * i as f32 / (TEETH * 2) as f32;
            let r = if i % 2 == 0 { 0.1070 } else { 0.1010 };
            Vec2::new(r * a.cos(), r * a.sin())
        })
        .collect();
    let inner = circle(0.084, TEETH * 2);
    let ring = extrude_ring(&outer, &inner, -0.0018, 0.0018, dark)
        .rotated(Quat::from_rotation_y(FRAC_PI_2))
        .translated(Vec3::X * CHAINLINE);
    part.add(&ring, HARD);
    let x = CHAINLINE - 0.009;
    let small = [
        Vec2::new(0.058, x - 0.0015),
        Vec2::new(0.0715, x - 0.0015),
        Vec2::new(0.0715, x + 0.0015),
        Vec2::new(0.058, x + 0.0015),
    ];
    part.add(&about_x(turned(&small, true, 24, steel)), HARD);

    part.finish()
}

/// Build a pedal about its axle. The origin is the middle of the pedal,
/// where the cleat sits; the crank is on the inboard side.
pub(crate) fn pedal(id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let component = part.slot(paint::COMPONENT);
    let steel = part.slot(paint::STEEL);
    let inboard = -(PEDAL_X - 0.072);
    let mut geo = about_x(cylinder(0.0065, inboard, -0.020, 8, steel));
    geo.append(
        &block(Vec3::new(0.033, 0.009, 0.042), 4.0, 12, component)
            .translated(Vec3::new(0.0, -0.004, 0.004)),
    );
    geo.append(&capsule(
        Vec3::new(-0.030, -0.003, 0.0),
        Vec3::new(-0.018, -0.003, 0.0),
        0.011,
        0.011,
        8,
        component,
    ));
    if id == PartId::PedalL {
        geo = geo.mirrored_x();
    }
    part.add(&geo, HARD);
    part.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn belt_is_a_closed_loop_that_hugs_each_wheel() {
        let wheels = [
            (Vec2::new(0.0, 0.0), 0.10, -1.0),
            (Vec2::new(-0.36, -0.07), 0.02, -1.0),
            (Vec2::new(-0.39, -0.01), 0.02, 1.0),
            (Vec2::new(-0.40, 0.07), 0.04, -1.0),
        ];
        let path = belt(&wheels);
        assert!(path[0].distance(path[path.len() - 1]) < 1.0e-6);
        for (centre, radius, _) in wheels {
            let touching = path
                .iter()
                .filter(|p| (p.distance(centre) - radius).abs() < 1.0e-5)
                .count();
            assert!(touching >= 2, "belt misses the wheel at {centre:?}");
        }
        // No point of the belt cuts inside a wheel.
        for p in &path {
            for (centre, radius, _) in wheels {
                assert!(p.distance(centre) > radius - 1.0e-4);
            }
        }
        // Consecutive points never double back.
        for w in path.windows(3) {
            let (a, b) = (w[1] - w[0], w[2] - w[1]);
            assert!(a.dot(b) > 0.0, "belt folds back at {:?}", w[1]);
        }
    }
}
