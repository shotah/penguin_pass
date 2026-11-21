//! The rider: pelvis, torso, head, arms, hands, legs, feet.
//!
//! Each part is built in its own local space. Limbs hang down -Y from the
//! joint at their origin, with the front of the limb toward -Z, so the pose
//! only has to aim them. Parts overlap at rounded joints instead of being
//! skinned.

use std::f32::consts::PI;

use glam::{Quat, Vec2, Vec3};

use crate::geo::{
    Cap, Geo, PartBuilder, Ring, Slot, block, cap, capsule, ellipsoid, keyed, loft, strut, sweep,
    torus,
};
use crate::mesh::{Part, PartId};
use crate::paint::{self, styled};
use crate::rig::{
    FOREARM, HEAD_PIVOT_LOCAL, HIP_HALF, SHIN, SHOULDER_LOCAL, THIGH, UPPER_ARM, WAIST_UP,
};
use crate::style::{RiderStyle, StyleColor};

/// Crease angle for skin and cloth, in degrees.
const SOFT: f32 = 60.0;
/// Crease angle for hard goods: helmet, glasses, shoes.
const HARD: f32 = 38.0;

/// Roughness of cloth.
const CLOTH: f32 = 0.82;
/// Roughness of the bib shorts. Lycra has a sheen, and under the game's sky
/// it is that sheen, not the near-black colour, that shows the legs' shape.
const LYCRA: f32 = 0.55;
/// Roughness of skin.
const SKIN: f32 = 0.62;

/// Build one part of the rider in its own local space.
pub(crate) fn part(style: &RiderStyle, id: PartId) -> Part {
    match id {
        PartId::Pelvis => pelvis(style),
        PartId::Torso => torso(style),
        PartId::Head => head(style),
        PartId::UpperArmL | PartId::UpperArmR => upper_arm(style, id),
        PartId::ForearmL | PartId::ForearmR => forearm(style, id),
        PartId::HandL | PartId::HandR => hand(style, id),
        PartId::ThighL | PartId::ThighR => thigh(style, id),
        PartId::ShinL | PartId::ShinR => shin(style, id),
        PartId::FootL | PartId::FootR => foot(style, id),
        _ => unreachable!("{} is not a part of the rider", id.name()),
    }
}

/// One cross-section of a limb: how far down it is (0 at the joint, 1 at
/// the far end), the material of the skin below it, and extra thickness
/// for cloth, in meters.
type Station = (f32, Slot, f32);

/// A limb hanging down -Y. `keys` shape it: at each `t` the half-width,
/// the half-depth, and how far the section sits toward the back, in meters.
fn limb(
    length: f32,
    keys: &[(f32, [f32; 3])],
    stations: &[Station],
    segs: usize,
    top: Cap,
    bottom: Cap,
) -> Geo {
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(t, _, pad)| {
            let [half_width, half_depth, back] = keyed(keys, t);
            Ring::ellipse(
                Vec3::new(0.0, -t * length, back),
                Vec3::X * (half_width + pad),
                Vec3::Z * (half_depth + pad),
            )
        })
        .collect();
    loft(&rings, segs, top, bottom, |row, _| stations[row].1)
}

fn thigh(style: &RiderStyle, id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let bibs = part.slot(styled(style, StyleColor::Bibs, LYCRA, 0.0));
    let band = part.slot(styled(style, StyleColor::JerseyAccent, CLOTH, 0.0));
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));
    let keys = [
        (0.00, [0.078, 0.088, 0.004]),
        (0.18, [0.088, 0.096, 0.004]),
        (0.40, [0.084, 0.092, 0.000]),
        (0.70, [0.069, 0.074, -0.002]),
        (0.88, [0.059, 0.062, 0.000]),
        (1.00, [0.055, 0.057, 0.000]),
    ];
    let pad = 0.0035;
    let stations = [
        (0.00, bibs, pad),
        (0.10, bibs, pad),
        (0.22, bibs, pad),
        (0.34, bibs, pad),
        (0.46, bibs, pad),
        (0.58, bibs, pad),
        (0.68, band, pad),
        (0.735, band, pad),
        (0.735, skin, 0.0),
        (0.86, skin, 0.0),
        (0.94, skin, 0.0),
        (1.00, skin, 0.0),
    ];
    let geo = limb(
        THIGH,
        &keys,
        &stations,
        12,
        Cap::Round(0.082),
        Cap::Round(0.055),
    );
    part.add(&geo, SOFT);
    part.finish()
}

fn shin(style: &RiderStyle, id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));
    let sock = part.slot(styled(style, StyleColor::Socks, CLOTH, 0.0));
    let keys = [
        (0.00, [0.054, 0.056, 0.000]),
        (0.12, [0.052, 0.056, 0.004]),
        (0.30, [0.057, 0.064, 0.015]),
        (0.50, [0.049, 0.053, 0.010]),
        (0.74, [0.036, 0.039, 0.003]),
        (0.92, [0.032, 0.035, 0.000]),
        (1.00, [0.034, 0.038, 0.000]),
    ];
    let pad = 0.004;
    let stations = [
        (0.00, skin, 0.0),
        (0.09, skin, 0.0),
        (0.19, skin, 0.0),
        (0.30, skin, 0.0),
        (0.42, skin, 0.0),
        (0.54, skin, 0.0),
        (0.64, skin, 0.0),
        (0.72, sock, 0.0),
        (0.72, sock, pad),
        (0.82, sock, pad),
        (0.92, sock, pad),
        (1.00, sock, pad),
    ];
    let geo = limb(
        SHIN,
        &keys,
        &stations,
        12,
        Cap::Round(0.054),
        Cap::Round(0.037),
    );
    part.add(&geo, SOFT);
    part.finish()
}

/// Height of the bottom of the shoe's sole above the pedal axle, in meters.
const SOLE_Y: f32 = 0.012;

fn foot(style: &RiderStyle, id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let upper = part.slot(styled(style, StyleColor::Shoes, 0.5, 0.0));
    let sole = part.slot(paint::SOLE);
    let mark = part.slot(paint::DECAL);
    let dark = part.slot(paint::COMPONENT);

    // Toe to heel: half-width, height above the sole, and toe spring.
    let stations: [(f32, f32, f32, f32); 9] = [
        (-0.092, 0.026, 0.028, 0.006),
        (-0.070, 0.040, 0.039, 0.003),
        (-0.030, 0.049, 0.050, 0.000),
        (0.010, 0.049, 0.062, 0.000),
        (0.055, 0.045, 0.078, 0.000),
        (0.100, 0.043, 0.089, 0.000),
        (0.140, 0.042, 0.091, 0.000),
        (0.175, 0.037, 0.083, 0.001),
        (0.196, 0.027, 0.060, 0.003),
    ];
    let sole_thick = 0.013;
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(z, half_width, height, lift)| {
            let above = height - sole_thick;
            Ring {
                c: Vec3::new(0.0, SOLE_Y + sole_thick + lift, z),
                u: Vec3::X * half_width,
                v: Vec3::Y * above,
                vn: sole_thick / above,
                e: [2.2, 4.5],
                phase: 0.0,
            }
        })
        .collect();
    const SEGS: usize = 14;
    let mut geo = loft(
        &rings,
        SEGS,
        Cap::Round(0.014),
        Cap::Round(0.010),
        |_, k| if k < SEGS / 2 { upper } else { sole },
    );
    // Heel tab, closure dial on the outside, and the cleat.
    geo.append(
        &block(Vec3::new(0.011, 0.022, 0.004), 4.0, 8, mark).translated(Vec3::new(
            0.0,
            SOLE_Y + 0.052,
            0.203,
        )),
    );
    geo.append(&capsule(
        Vec3::new(0.034, SOLE_Y + 0.066, 0.045),
        Vec3::new(0.041, SOLE_Y + 0.070, 0.045),
        0.011,
        0.011,
        8,
        dark,
    ));
    geo.append(
        &block(Vec3::new(0.024, 0.007, 0.032), 4.0, 8, sole)
            .translated(Vec3::new(0.0, 0.007, 0.002)),
    );
    if id == PartId::FootL {
        geo = geo.mirrored_x();
    }
    part.add(&geo, HARD);
    part.finish()
}

fn pelvis(style: &RiderStyle) -> Part {
    let mut part = PartBuilder::new(PartId::Pelvis.name());
    let bibs = part.slot(styled(style, StyleColor::Bibs, LYCRA, 0.0));
    // Up the spine: half-width, half-depth, and how far back the section sits.
    let stations: [(f32, f32, f32, f32); 6] = [
        (-0.080, 0.080, 0.064, 0.000),
        (-0.045, 0.132, 0.092, 0.004),
        (0.005, HIP_HALF + 0.062, 0.100, 0.006),
        (0.055, 0.138, 0.094, 0.004),
        (WAIST_UP - 0.005, 0.124, 0.088, 0.000),
        (WAIST_UP + 0.030, 0.104, 0.076, 0.000),
    ];
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(y, half_width, half_depth, back)| {
            Ring::squircle(
                Vec3::new(0.0, y, back),
                Vec3::X * half_width,
                -Vec3::Z * half_depth,
                2.4,
            )
        })
        .collect();
    part.add(
        &loft(&rings, 16, Cap::Round(0.025), Cap::Round(0.030), |_, _| {
            bibs
        }),
        SOFT,
    );
    part.finish()
}

/// Shape of the torso at height `y` above the waist pivot: half-width,
/// half-depth, and the centre's Z (negative is forward), in meters.
fn torso_section(y: f32) -> [f32; 3] {
    const KEYS: [(f32, [f32; 3]); 9] = [
        (-0.072, [0.128, 0.090, 0.004]),
        (-0.060, [0.143, 0.101, 0.004]),
        (0.000, [0.139, 0.100, 0.000]),
        (0.100, [0.139, 0.101, -0.012]),
        (0.200, [0.149, 0.106, -0.040]),
        (0.300, [0.159, 0.108, -0.082]),
        (0.370, [0.157, 0.099, -0.116]),
        (0.410, [0.120, 0.082, -0.138]),
        (0.440, [0.072, 0.064, -0.152]),
    ];
    keyed(&KEYS, y)
}

/// The torso's cross-section at height `y`, square to its curved centreline.
/// `bulge` pushes the back out, in meters, for the jersey pockets.
fn torso_ring(y: f32, bulge: f32) -> Ring {
    let [half_width, half_depth, z] = torso_section(y);
    let step = 0.004;
    let slope = (torso_section(y + step)[2] - torso_section(y - step)[2]) / (2.0 * step);
    let tangent = Vec3::new(0.0, 1.0, slope).normalize();
    let front = Vec3::new(0.0, tangent.z, -tangent.y);
    Ring {
        c: Vec3::new(0.0, y, z),
        u: Vec3::X * half_width,
        v: front * half_depth,
        vn: 1.0 + bulge / half_depth,
        e: [2.6, 2.6],
        phase: 0.0,
    }
}

fn torso(style: &RiderStyle) -> Part {
    let mut part = PartBuilder::new(PartId::Torso.name());
    let primary = part.slot(styled(style, StyleColor::JerseyPrimary, CLOTH, 0.0));
    let secondary = part.slot(styled(style, StyleColor::JerseySecondary, CLOTH, 0.0));
    let accent = part.slot(styled(style, StyleColor::JerseyAccent, CLOTH, 0.0));
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));
    let seam = part.slot(paint::SEAM);

    // Heights of the sections, and how far the pockets push the back out.
    const POCKET: f32 = 0.009;
    let stations: [(f32, f32); 15] = [
        (-0.072, 0.0),
        (-0.060, 0.5 * POCKET),
        (-0.046, POCKET),
        (-0.010, POCKET),
        (0.030, POCKET),
        (0.080, POCKET),
        (0.115, POCKET),
        (0.135, POCKET),
        (0.135, 0.0),
        (0.200, 0.0),
        (0.260, 0.0),
        (0.310, 0.0),
        (0.370, 0.0),
        (0.410, 0.0),
        (0.440, 0.0),
    ];
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(y, bulge)| torso_ring(y, bulge))
        .collect();
    const SEGS: usize = 20;
    // Columns run from the right side (0) round the chest (5) to the left
    // side (10) and across the back (15).
    let body = loft(
        &rings,
        SEGS,
        Cap::Round(0.020),
        Cap::Round(0.012),
        |row, k| {
            let back = (11..=18).contains(&k);
            if row == 0 || row >= 11 {
                // Tucked under the hem, and the yoke over the shoulders.
                primary
            } else if row == 1 || !back {
                // A pale hem all the way round, and the front panel.
                secondary
            } else if row == 6 || row == 7 {
                accent
            } else {
                primary
            }
        },
    );
    part.add(&body, SOFT);

    // Two seams split the back into three pockets.
    for angle in [242.0_f32, 298.0] {
        let path: Vec<Vec3> = rings[2..=7]
            .iter()
            .map(|ring| ring.point(angle.to_radians()))
            .collect();
        part.add(
            &sweep(
                &path,
                Vec3::X,
                |_| Vec2::splat(0.0028),
                4,
                Cap::Flat,
                Cap::Flat,
                |_, _| seam,
            ),
            SOFT,
        );
    }

    // A ball round each shoulder joint, a little larger than the sleeve. The
    // arm turns inside it, so no notch opens between the yoke and the sleeve
    // when the arm reaches for the bars.
    for side in [-1.0, 1.0] {
        let joint = Vec3::new(side * SHOULDER_LOCAL.x, SHOULDER_LOCAL.y, SHOULDER_LOCAL.z);
        part.add(
            &ellipsoid(Vec3::splat(0.064), 10, 6, primary).translated(joint),
            SOFT,
        );
    }

    // Neck, ending in a ball inside the head, and the collar round its base.
    let neck_base = Vec3::new(0.0, 0.412, -0.146);
    part.add(
        &capsule(neck_base, HEAD_PIVOT_LOCAL, 0.052, 0.048, 10, skin),
        SOFT,
    );
    let neck_dir = (HEAD_PIVOT_LOCAL - neck_base).normalize();
    part.add(
        &torus(0.058, 0.013, 12, 5, accent)
            .rotated(Quat::from_rotation_arc(Vec3::Y, neck_dir))
            .translated(neck_base + neck_dir * 0.030),
        SOFT,
    );
    part.finish()
}

fn upper_arm(style: &RiderStyle, id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let sleeve = part.slot(styled(style, StyleColor::JerseyPrimary, CLOTH, 0.0));
    let cuff = part.slot(styled(style, StyleColor::JerseyAccent, CLOTH, 0.0));
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));
    let keys = [
        (0.00, [0.055, 0.060, 0.000]),
        (0.20, [0.055, 0.059, 0.002]),
        (0.55, [0.048, 0.051, 0.000]),
        (1.00, [0.041, 0.043, 0.000]),
    ];
    let pad = 0.004;
    let stations = [
        (0.00, sleeve, pad),
        (0.13, sleeve, pad),
        (0.27, sleeve, pad),
        (0.40, cuff, pad),
        (0.47, cuff, pad),
        (0.47, skin, 0.0),
        (0.64, skin, 0.0),
        (0.82, skin, 0.0),
        (1.00, skin, 0.0),
    ];
    let geo = limb(
        UPPER_ARM,
        &keys,
        &stations,
        10,
        Cap::Round(0.062),
        Cap::Round(0.041),
    );
    part.add(&geo, SOFT);
    part.finish()
}

fn forearm(style: &RiderStyle, id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));
    let keys = [
        (0.00, [0.041, 0.043, 0.000]),
        (0.22, [0.046, 0.049, 0.002]),
        (0.65, [0.036, 0.038, 0.000]),
        (1.00, [0.030, 0.032, 0.000]),
    ];
    let stations = [
        (0.00, skin, 0.0),
        (0.15, skin, 0.0),
        (0.32, skin, 0.0),
        (0.52, skin, 0.0),
        (0.72, skin, 0.0),
        (0.88, skin, 0.0),
        (1.00, skin, 0.0),
    ];
    let geo = limb(
        FOREARM,
        &keys,
        &stations,
        10,
        Cap::Round(0.041),
        Cap::Round(0.031),
    );
    part.add(&geo, SOFT);
    part.finish()
}

/// Hands are drawn a little large, so they read from the chase camera.
const HAND_SCALE: f32 = 1.15;

/// A hand built for the right side: the wrist at the origin, the fingers
/// toward -Z, the palm facing -Y, the thumb on the -X side.
fn hand(style: &RiderStyle, id: PartId) -> Part {
    let mut part = PartBuilder::new(id.name());
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));

    // Palm, from the wrist to the knuckles.
    let stations: [(f32, f32, f32); 4] = [
        (0.012, 0.025, 0.017),
        (-0.022, 0.036, 0.017),
        (-0.060, 0.042, 0.016),
        (-0.090, 0.040, 0.0135),
    ];
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(z, half_width, half_thick)| {
            Ring::squircle(
                Vec3::new(0.0, 0.0, z),
                Vec3::Y * half_thick,
                Vec3::X * half_width,
                2.8,
            )
        })
        .collect();
    let mut geo = loft(&rings, 10, Cap::Blunt(0.014), Cap::Blunt(0.008), |_, _| {
        skin
    });

    // Four fingers curling down round the hood.
    for (i, x) in [-0.0305_f32, -0.0102, 0.0102, 0.0305]
        .into_iter()
        .enumerate()
    {
        let reach = [1.0, 1.06, 1.0, 0.88][i];
        let curl = [
            Vec3::new(x, -0.001, -0.080),
            Vec3::new(x, -0.005, -0.080 - 0.030 * reach),
            Vec3::new(x, -0.026, -0.080 - 0.048 * reach),
            Vec3::new(x, -0.052, -0.080 - 0.044 * reach),
            Vec3::new(x, -0.068, -0.080 - 0.024 * reach),
        ];
        geo.append(&sweep(
            &curl,
            Vec3::Y,
            |_| Vec2::splat(0.0108),
            6,
            Cap::Flat,
            Cap::Blunt(0.008),
            |_, _| skin,
        ));
    }
    // Thumb, hooked round the inside of the hood.
    geo.append(&sweep(
        &[
            Vec3::new(-0.030, -0.002, -0.016),
            Vec3::new(-0.047, -0.010, -0.048),
            Vec3::new(-0.050, -0.026, -0.080),
        ],
        Vec3::Y,
        |i| Vec2::splat([0.0145, 0.0130, 0.0115][i]),
        6,
        Cap::Blunt(0.010),
        Cap::Blunt(0.009),
        |_, _| skin,
    ));
    geo = geo.scaled(Vec3::splat(HAND_SCALE));
    if id == PartId::HandL {
        geo = geo.mirrored_x();
    }
    // Six-sided fingers need a wide crease angle to shade round.
    part.add(&geo, 80.0);
    part.finish()
}

/// The helmet shell: arches lofted from the brow to the tail, with vent
/// slots sunk between the ribs.
fn helmet(shell: Slot, stripe: Slot, dark: Slot) -> Geo {
    /// Segments across each arch, from the right brim over to the left.
    const ARCH: usize = 13;
    // Front to back: z, half-width, height of the crown, height of the brim.
    let stations: [(f32, f32, f32, f32); 8] = [
        (-0.142, 0.068, 0.046, 0.124),
        (-0.124, 0.090, 0.072, 0.113),
        (-0.078, 0.106, 0.104, 0.102),
        (-0.018, 0.114, 0.128, 0.094),
        (0.046, 0.113, 0.134, 0.086),
        (0.100, 0.102, 0.126, 0.072),
        (0.138, 0.078, 0.102, 0.062),
        (0.160, 0.048, 0.066, 0.068),
    ];
    let mut geo = Geo::default();
    let ring = ARCH + 1;
    for &(z, half_width, height, brim) in &stations {
        for j in 0..=ARCH {
            let (sin, cos) = (PI * j as f32 / ARCH as f32).sin_cos();
            let rise = sin.max(0.0).powf(0.85);
            geo.vert(Vec3::new(half_width * cos, brim + height * rise, z));
        }
    }
    let rows = stations.len() - 1;
    for row in 0..rows {
        for j in 0..ring {
            let j2 = (j + 1) % ring;
            let quad = [
                (row * ring + j) as u32,
                (row * ring + j2) as u32,
                ((row + 1) * ring + j2) as u32,
                ((row + 1) * ring + j) as u32,
            ];
            let slot_channel = matches!(j, 2 | 4 | 6 | 8 | 10);
            let long_vent = slot_channel && (1..=4).contains(&row);
            let exhaust = matches!(j, 4 | 6 | 8) && row == 5;
            if long_vent || exhaust {
                pit(&mut geo, quad, 0.20, 0.013, dark);
            } else {
                let slot = if j == 5 || j == 7 { stripe } else { shell };
                geo.quad(quad[0], quad[1], quad[2], quad[3], slot);
            }
        }
    }
    cap(&mut geo, 0, ring, Cap::Round(0.014), true, |_| shell);
    cap(
        &mut geo,
        (rows * ring) as u32,
        ring,
        Cap::Round(0.012),
        false,
        |_| shell,
    );
    geo
}

/// Replace a quad with a sunken pocket: four walls down to a smaller floor.
fn pit(geo: &mut Geo, quad: [u32; 4], inset: f32, depth: f32, slot: Slot) {
    let p = quad.map(|i| geo.pos[i as usize]);
    let centre = (p[0] + p[1] + p[2] + p[3]) * 0.25;
    let normal = (p[2] - p[0]).cross(p[3] - p[1]).normalize();
    let floor = p.map(|q| geo.vert(q + (centre - q) * inset - normal * depth));
    for e in 0..4 {
        let f = (e + 1) % 4;
        geo.quad(quad[e], quad[f], floor[f], floor[e], slot);
    }
    geo.quad(floor[0], floor[1], floor[2], floor[3], slot);
}

/// The head is drawn a little large, as a stylized character's is.
const HEAD_SCALE: f32 = 1.08;

/// A part builder that scales every shape about the part's origin.
struct Scaled {
    part: PartBuilder,
    scale: f32,
}

impl Scaled {
    fn slot(&mut self, paint: paint::Paint) -> Slot {
        self.part.slot(paint)
    }

    fn add(&mut self, geo: &Geo, crease_deg: f32) {
        self.part
            .add(&geo.clone().scaled(Vec3::splat(self.scale)), crease_deg);
    }
}

fn head(style: &RiderStyle) -> Part {
    let mut part = Scaled {
        part: PartBuilder::new(PartId::Head.name()),
        scale: HEAD_SCALE,
    };
    let skin = part.slot(styled(style, StyleColor::Skin, SKIN, 0.0));
    let shell = part.slot(styled(style, StyleColor::Helmet, 0.35, 0.0));
    let stripe = part.slot(styled(style, StyleColor::JerseySecondary, 0.35, 0.0));
    let dark = part.slot(paint::HELMET_DARK);
    let lens = part.slot(paint::LENS);
    let hair = part.slot(paint::HAIR);
    let lips = part.slot(paint::LIPS);

    // Skull, chin to crown: half-width, half-depth, centre Z.
    let stations: [(f32, f32, f32, f32); 7] = [
        (-0.058, 0.046, 0.052, -0.044),
        (-0.028, 0.070, 0.078, -0.030),
        (0.016, 0.089, 0.099, -0.017),
        (0.060, 0.096, 0.108, -0.011),
        (0.104, 0.097, 0.110, -0.009),
        (0.148, 0.088, 0.101, -0.009),
        (0.180, 0.064, 0.075, -0.009),
    ];
    let rings: Vec<Ring> = stations
        .iter()
        .map(|&(y, half_width, half_depth, z)| {
            Ring::squircle(
                Vec3::new(0.0, y, z),
                Vec3::X * half_width,
                -Vec3::Z * half_depth,
                2.3,
            )
        })
        .collect();
    part.add(
        &loft(&rings, 16, Cap::Round(0.014), Cap::Round(0.022), |_, _| {
            skin
        }),
        SOFT,
    );
    // Nose and ears.
    part.add(
        &ellipsoid(Vec3::new(0.012, 0.019, 0.017), 8, 4, skin)
            .translated(Vec3::new(0.0, 0.040, -0.119)),
        SOFT,
    );
    for side in [-1.0_f32, 1.0] {
        part.add(
            &ellipsoid(Vec3::new(0.008, 0.021, 0.013), 8, 4, skin).translated(Vec3::new(
                side * 0.095,
                0.058,
                0.004,
            )),
            SOFT,
        );
    }

    // A small, calm mouth, set into the face.
    let mouth: Vec<Vec3> = [
        (-0.021, -0.0085, -0.1075),
        (-0.011, -0.0125, -0.1100),
        (0.000, -0.0138, -0.1108),
        (0.011, -0.0125, -0.1100),
        (0.021, -0.0085, -0.1075),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z))
    .collect();
    part.add(
        &sweep(
            &mouth,
            Vec3::Y,
            |_| Vec2::new(0.0026, 0.0032),
            5,
            Cap::Blunt(0.002),
            Cap::Blunt(0.002),
            |_, _| lips,
        ),
        SOFT,
    );

    // Sunglasses: one wrap-round shield and two arms.
    let lens_y = 0.080;
    let wrap = |deg: f32| {
        let (sin, cos) = deg.to_radians().sin_cos();
        Vec3::new(0.101 * sin, lens_y, -0.011 - 0.113 * cos)
    };
    let path: Vec<Vec3> = (0..=8).map(|i| wrap(-78.0 + 19.5 * i as f32)).collect();
    part.add(
        &sweep(
            &path,
            Vec3::Y,
            |i| {
                let centre = 1.0 - (i as f32 - 4.0).abs() / 4.0;
                Vec2::new(0.0055, 0.012 + 0.006 * centre)
            },
            6,
            Cap::Blunt(0.004),
            Cap::Blunt(0.004),
            |_, _| lens,
        ),
        HARD,
    );
    for side in [-1.0_f32, 1.0] {
        let r = Vec2::new(0.0025, 0.0045);
        part.add(
            &strut(
                wrap(side * 74.0),
                Vec3::new(side * 0.099, lens_y - 0.006, 0.014),
                r,
                r,
                Vec3::Y,
                4,
                dark,
            ),
            HARD,
        );
    }

    // Helmet, its straps, and the retention dial at the nape.
    part.add(&helmet(shell, stripe, dark), 52.0);
    for side in [-1.0_f32, 1.0] {
        let under_ear = Vec3::new(side * 0.090, 0.012, -0.012);
        let r = Vec2::new(0.0065, 0.0022);
        for from in [
            Vec3::new(side * 0.106, 0.108, -0.050),
            Vec3::new(side * 0.106, 0.098, 0.050),
        ] {
            part.add(&strut(from, under_ear, r, r, Vec3::X, 4, dark), HARD);
        }
        part.add(
            &strut(
                under_ear,
                Vec3::new(side * 0.034, -0.060, -0.058),
                r,
                r,
                Vec3::X,
                4,
                dark,
            ),
            HARD,
        );
        part.add(
            &strut(
                Vec3::new(side * 0.062, 0.080, 0.100),
                Vec3::new(side * 0.014, 0.044, 0.103),
                r,
                r,
                Vec3::Z,
                4,
                dark,
            ),
            HARD,
        );
    }
    part.add(
        &ellipsoid(Vec3::new(0.017, 0.013, 0.008), 8, 4, dark)
            .translated(Vec3::new(0.0, 0.040, 0.104)),
        HARD,
    );
    // Hair: a shell just proud of the skull at the back and sides, showing
    // at the nape and round the ears under the helmet.
    let hair_rings: Vec<Ring> = stations[2..=5]
        .iter()
        .map(|&(y, half_width, half_depth, z)| Ring {
            c: Vec3::new(0.0, y, z),
            u: Vec3::X * (half_width * 1.04),
            v: -Vec3::Z * (half_depth * 0.22),
            vn: 1.07 / 0.22,
            e: [2.3, 2.3],
            phase: 0.0,
        })
        .collect();
    part.add(
        &loft(
            &hair_rings,
            16,
            Cap::Blunt(0.006),
            Cap::Blunt(0.006),
            |_, _| hair,
        ),
        SOFT,
    );
    part.part.finish()
}
