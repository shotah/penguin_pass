//! Mesh-building primitives.
//!
//! Every primitive returns a [`Geo`]: bare positions plus triangles, each
//! triangle tagged with the material slot it will end up in. Normals are not
//! authored. [`PartBuilder::add`] derives them from the final positions with a
//! crease angle, so a shape is smooth where it is round and sharp where two
//! faces meet at an edge, including across a colour boundary.
//!
//! All shapes are closed shells with counter-clockwise front faces.

use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Affine3A, Quat, Vec2, Vec3};

use crate::mesh::{Part, Surface};
use crate::paint::Paint;

/// Index of a material within the part being built.
pub(crate) type Slot = u8;

/// Triangles thinner than this are dropped, in square meters.
const MIN_TRIANGLE_AREA: f32 = 1.0e-10;

/// Positions and slot-tagged triangles, before normals exist.
#[derive(Clone, Default)]
pub(crate) struct Geo {
    pub pos: Vec<Vec3>,
    pub tris: Vec<[u32; 3]>,
    pub slots: Vec<Slot>,
}

impl Geo {
    pub fn vert(&mut self, p: Vec3) -> u32 {
        self.pos.push(p);
        (self.pos.len() - 1) as u32
    }

    pub fn tri(&mut self, a: u32, b: u32, c: u32, slot: Slot) {
        if a != b && b != c && a != c {
            self.tris.push([a, b, c]);
            self.slots.push(slot);
        }
    }

    /// Two triangles `(a, b, c)` and `(a, c, d)`.
    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32, slot: Slot) {
        self.tri(a, b, c, slot);
        self.tri(a, c, d, slot);
    }

    /// Append another shape. The two stay separate shells.
    pub fn append(&mut self, other: &Geo) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(&other.pos);
        self.tris.extend(
            other
                .tris
                .iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
        self.slots.extend_from_slice(&other.slots);
    }

    /// Apply an affine map. A mirroring map also flips the winding, so the
    /// faces keep pointing outward.
    pub fn transformed(mut self, m: Affine3A) -> Self {
        for p in &mut self.pos {
            *p = m.transform_point3(*p);
        }
        if m.matrix3.determinant() < 0.0 {
            for t in &mut self.tris {
                t.swap(1, 2);
            }
        }
        self
    }

    pub fn translated(self, by: Vec3) -> Self {
        self.transformed(Affine3A::from_translation(by))
    }

    pub fn rotated(self, q: Quat) -> Self {
        self.transformed(Affine3A::from_quat(q))
    }

    pub fn scaled(self, s: Vec3) -> Self {
        self.transformed(Affine3A::from_scale(s))
    }

    /// Mirror across the YZ plane (left becomes right).
    pub fn mirrored_x(self) -> Self {
        self.scaled(Vec3::new(-1.0, 1.0, 1.0))
    }

    /// Merge vertices that share a position exactly, and drop the triangles
    /// that collapse. Two rings of a loft that coincide over part of their
    /// length then shade as one surface there.
    fn welded(&self) -> Geo {
        let mut first: BTreeMap<[u32; 3], u32> = BTreeMap::new();
        let mut pos = Vec::with_capacity(self.pos.len());
        let remap: Vec<u32> = self
            .pos
            .iter()
            .map(|p| {
                // Adding zero folds -0.0 into 0.0 so the two weld together.
                let key = (*p + Vec3::ZERO).to_array().map(f32::to_bits);
                *first.entry(key).or_insert_with(|| {
                    pos.push(*p);
                    (pos.len() - 1) as u32
                })
            })
            .collect();
        let mut out = Geo {
            pos,
            tris: Vec::with_capacity(self.tris.len()),
            slots: Vec::with_capacity(self.slots.len()),
        };
        for (t, slot) in self.tris.iter().zip(&self.slots) {
            let [a, b, c] = t.map(|i| remap[i as usize]);
            out.tri(a, b, c, *slot);
        }
        out
    }

    /// Retag triangles by the centroid of each one.
    #[cfg(test)]
    pub fn reslotted(mut self, f: impl Fn(Vec3, Slot) -> Slot) -> Self {
        for (t, slot) in self.tris.iter().zip(self.slots.iter_mut()) {
            let c =
                (self.pos[t[0] as usize] + self.pos[t[1] as usize] + self.pos[t[2] as usize]) / 3.0;
            *slot = f(c, *slot);
        }
        self
    }
}

/// Collects shapes into the surfaces of one part.
pub(crate) struct PartBuilder {
    name: &'static str,
    surfaces: Vec<Surface>,
}

impl PartBuilder {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            surfaces: Vec::new(),
        }
    }

    /// Slot for a paint. Equal paints share one slot, so one surface. Two
    /// style colours that happen to match stay apart, so a recolour of one
    /// never moves triangles between surfaces.
    pub fn slot(&mut self, paint: Paint) -> Slot {
        let found = self
            .surfaces
            .iter()
            .position(|s| s.material == paint.material && s.style_color == paint.style_color);
        if let Some(i) = found {
            return i as Slot;
        }
        self.surfaces.push(Surface {
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            material: paint.material,
            style_color: paint.style_color,
        });
        (self.surfaces.len() - 1) as Slot
    }

    /// Add a shape. Faces that meet at less than `crease_deg` share a smooth
    /// normal. Sharper edges get split normals.
    pub fn add(&mut self, geo: &Geo, crease_deg: f32) {
        let geo = &geo.welded();
        let cos_crease = crease_deg.to_radians().cos();
        let tri_count = geo.tris.len();
        let mut face_n = Vec::with_capacity(tri_count);
        let mut keep = Vec::with_capacity(tri_count);
        let mut corner_angle = Vec::with_capacity(tri_count);
        for t in &geo.tris {
            let [a, b, c] = t.map(|i| geo.pos[i as usize]);
            let cross = (b - a).cross(c - a);
            let area = 0.5 * cross.length();
            keep.push(area >= MIN_TRIANGLE_AREA);
            face_n.push(cross.normalize_or_zero());
            corner_angle.push([
                (b - a).angle_between(c - a),
                (c - b).angle_between(a - b),
                (a - c).angle_between(b - c),
            ]);
        }

        // Corners around each vertex, as a flat list with offsets.
        let mut start = vec![0_u32; geo.pos.len() + 1];
        for (t, tri) in geo.tris.iter().enumerate() {
            if keep[t] {
                for &v in tri {
                    start[v as usize + 1] += 1;
                }
            }
        }
        for i in 0..geo.pos.len() {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut around = vec![(0_u32, 0_u8); start[geo.pos.len()] as usize];
        for (t, tri) in geo.tris.iter().enumerate() {
            if keep[t] {
                for (c, &v) in tri.iter().enumerate() {
                    around[fill[v as usize] as usize] = (t as u32, c as u8);
                    fill[v as usize] += 1;
                }
            }
        }

        // Output vertices already made for each source vertex.
        let mut made: Vec<Vec<(Slot, Vec3, u32)>> = vec![Vec::new(); geo.pos.len()];
        for (t, tri) in geo.tris.iter().enumerate() {
            if !keep[t] {
                continue;
            }
            let slot = geo.slots[t];
            let mut out = [0_u32; 3];
            for (c, &v) in tri.iter().enumerate() {
                let v = v as usize;
                let mut sum = Vec3::ZERO;
                for &(t2, c2) in &around[start[v] as usize..start[v + 1] as usize] {
                    let n2 = face_n[t2 as usize];
                    if n2.dot(face_n[t]) >= cos_crease {
                        sum += n2 * corner_angle[t2 as usize][c2 as usize];
                    }
                }
                let normal = sum.try_normalize().unwrap_or(face_n[t]);
                let found = made[v]
                    .iter()
                    .find(|(s, n, _)| *s == slot && n.dot(normal) > 0.999_9);
                out[c] = if let Some(&(_, _, index)) = found {
                    index
                } else {
                    let surface = &mut self.surfaces[slot as usize];
                    let index = surface.positions.len() as u32;
                    surface.positions.push(geo.pos[v].to_array());
                    surface.normals.push(normal.to_array());
                    made[v].push((slot, normal, index));
                    index
                };
            }
            self.surfaces[slot as usize].indices.extend_from_slice(&out);
        }
    }

    /// Finish the part. Slots that never received a triangle are dropped.
    pub fn finish(self) -> Part {
        Part {
            name: self.name,
            surfaces: self
                .surfaces
                .into_iter()
                .filter(|s| !s.indices.is_empty())
                .collect(),
        }
    }
}

/// How a loft is closed at one end.
#[derive(Clone, Copy)]
pub(crate) enum Cap {
    /// A flat fan across the end ring.
    Flat,
    /// A dome that bulges out by this many meters.
    Round(f32),
    /// A coarser dome for small parts: one ring between the rim and the pole.
    Blunt(f32),
}

/// One cross-section of a loft: a superellipse in the plane of `u` and `v`.
///
/// `u × v` must point along the loft, toward the next ring.
#[derive(Clone, Copy)]
pub(crate) struct Ring {
    /// Center of the section, in meters.
    pub c: Vec3,
    /// Half-extent along the first axis.
    pub u: Vec3,
    /// Half-extent along the second axis, on its positive side.
    pub v: Vec3,
    /// Scale of `v` on its negative side. 1 makes the section symmetric.
    pub vn: f32,
    /// Superellipse exponent on the positive and negative `v` side.
    /// 2 is an ellipse. Larger is boxier.
    pub e: [f32; 2],
    /// Offset of the first sample, as a fraction of one step. 0.5 with four
    /// samples and a large exponent puts the samples on the corners of a box.
    pub phase: f32,
}

impl Ring {
    /// An ellipse.
    pub fn ellipse(c: Vec3, u: Vec3, v: Vec3) -> Self {
        Self {
            c,
            u,
            v,
            vn: 1.0,
            e: [2.0, 2.0],
            phase: 0.0,
        }
    }

    /// A superellipse with one exponent all round.
    pub fn squircle(c: Vec3, u: Vec3, v: Vec3, e: f32) -> Self {
        Self {
            c,
            u,
            v,
            vn: 1.0,
            e: [e, e],
            phase: 0.0,
        }
    }

    /// Point on the section at parameter `a`, in radians from `u` toward `v`.
    pub fn point(&self, a: f32) -> Vec3 {
        let (s, c) = a.sin_cos();
        let (e, scale) = if s >= 0.0 {
            (self.e[0], 1.0)
        } else {
            (self.e[1], self.vn)
        };
        let power = 2.0 / e;
        let x = c.signum() * c.abs().powf(power);
        let y = s.signum() * s.abs().powf(power) * scale;
        self.c + self.u * x + self.v * y
    }

    /// Sample the section at `segs` evenly spaced parameters.
    pub fn points(&self, segs: usize) -> Vec<Vec3> {
        (0..segs)
            .map(|k| self.point(TAU * (k as f32 + self.phase) / segs as f32))
            .collect()
    }
}

/// Skin a stack of point rings. Every ring has the same number of points, and
/// the points run counter-clockwise when seen from the end of the loft.
///
/// `slot(row, column)` picks the material of each quad. The caps use the row
/// next to them.
pub(crate) fn loft_points(
    rings: &[Vec<Vec3>],
    start: Cap,
    end: Cap,
    slot: impl Fn(usize, usize) -> Slot,
) -> Geo {
    let mut geo = Geo::default();
    let segs = rings[0].len();
    let rows = rings.len();
    let mut first = Vec::with_capacity(rows);
    for ring in rings {
        debug_assert_eq!(ring.len(), segs);
        first.push(geo.pos.len() as u32);
        geo.pos.extend_from_slice(ring);
    }
    for row in 0..rows - 1 {
        for k in 0..segs {
            let k2 = (k + 1) % segs;
            let a = first[row] + k as u32;
            let b = first[row] + k2 as u32;
            let c = first[row + 1] + k2 as u32;
            let d = first[row + 1] + k as u32;
            geo.quad(a, b, c, d, slot(row, k));
        }
    }
    cap(&mut geo, first[0], segs, start, true, |k| slot(0, k));
    cap(&mut geo, first[rows - 1], segs, end, false, |k| {
        slot(rows - 2, k)
    });
    geo
}

/// Close one end of a loft whose ring of `segs` points starts at vertex
/// `first`. `at_start` caps face against the loft direction.
pub(crate) fn cap(
    geo: &mut Geo,
    first: u32,
    segs: usize,
    kind: Cap,
    at_start: bool,
    slot: impl Fn(usize) -> Slot,
) {
    let ring: Vec<Vec3> = (0..segs).map(|k| geo.pos[first as usize + k]).collect();
    let center = ring.iter().copied().sum::<Vec3>() / segs as f32;
    // Newell normal: along the loft for a counter-clockwise ring.
    let mut axis = Vec3::ZERO;
    for k in 0..segs {
        let p = ring[k] - center;
        let q = ring[(k + 1) % segs] - center;
        axis += p.cross(q);
    }
    let axis = axis.normalize_or_zero() * if at_start { -1.0 } else { 1.0 };

    let (depth, steps) = match kind {
        Cap::Flat => (0.0, 1),
        Cap::Round(depth) => (depth, 3),
        Cap::Blunt(depth) => (depth, 2),
    };
    let mut prev = first;
    for j in 1..steps {
        let phi = FRAC_PI_2 * j as f32 / steps as f32;
        let (s, c) = phi.sin_cos();
        let next = geo.pos.len() as u32;
        for p in &ring {
            geo.pos
                .push(center + (*p - center) * c + axis * (depth * s));
        }
        for k in 0..segs {
            let k2 = ((k + 1) % segs) as u32;
            let k = k as u32;
            if at_start {
                geo.quad(next + k, next + k2, prev + k2, prev + k, slot(k as usize));
            } else {
                geo.quad(prev + k, prev + k2, next + k2, next + k, slot(k as usize));
            }
        }
        prev = next;
    }
    let pole = geo.vert(center + axis * depth);
    for k in 0..segs {
        let k2 = ((k + 1) % segs) as u32;
        let k = k as u32;
        if at_start {
            geo.tri(pole, prev + k2, prev + k, slot(k as usize));
        } else {
            geo.tri(pole, prev + k, prev + k2, slot(k as usize));
        }
    }
}

/// Skin a stack of superellipse sections.
pub(crate) fn loft(
    rings: &[Ring],
    segs: usize,
    start: Cap,
    end: Cap,
    slot: impl Fn(usize, usize) -> Slot,
) -> Geo {
    let points: Vec<Vec<Vec3>> = rings.iter().map(|r| r.points(segs)).collect();
    loft_points(&points, start, end, slot)
}

/// Tangent and a rotation-minimising frame at each point of a path.
///
/// Returns `(tangent, u, v)` with `u × v = tangent`. `v` starts as close to
/// `up` as the first tangent allows.
pub(crate) fn path_frames(path: &[Vec3], up: Vec3) -> Vec<(Vec3, Vec3, Vec3)> {
    let n = path.len();
    let tangent = |i: usize| {
        let a = path[i.saturating_sub(1)];
        let b = path[(i + 1).min(n - 1)];
        (b - a).normalize_or_zero()
    };
    let t0 = tangent(0);
    let mut v = (up - t0 * up.dot(t0)).try_normalize().unwrap_or_else(|| {
        let other = if t0.x.abs() < 0.9 { Vec3::X } else { Vec3::Z };
        (other - t0 * other.dot(t0)).normalize()
    });
    let mut u = v.cross(t0);
    let mut frames = Vec::with_capacity(n);
    frames.push((t0, u, v));
    let mut prev = t0;
    for i in 1..n {
        let t = tangent(i);
        let q = Quat::from_rotation_arc(prev, t);
        v = q * v;
        // Remove drift so the frame stays orthonormal.
        v = (v - t * v.dot(t)).normalize();
        u = v.cross(t);
        frames.push((t, u, v));
        prev = t;
    }
    frames
}

/// Sweep an elliptical section along a path.
///
/// `radii(i)` gives the half-extents along the frame's `u` and `v` axes at
/// path point `i`. `v` starts near `up`.
pub(crate) fn sweep(
    path: &[Vec3],
    up: Vec3,
    radii: impl Fn(usize) -> Vec2,
    segs: usize,
    start: Cap,
    end: Cap,
    slot: impl Fn(usize, usize) -> Slot,
) -> Geo {
    let frames = path_frames(path, up);
    let rings: Vec<Ring> = frames
        .iter()
        .enumerate()
        .map(|(i, &(_, u, v))| {
            let r = radii(i);
            Ring::ellipse(path[i], u * r.x, v * r.y)
        })
        .collect();
    loft(&rings, segs, start, end, slot)
}

/// A round tube of constant radius along a path, with flat ends.
pub(crate) fn tube(path: &[Vec3], radius: f32, segs: usize, slot: Slot) -> Geo {
    sweep(
        path,
        Vec3::Y,
        |_| Vec2::splat(radius),
        segs,
        Cap::Flat,
        Cap::Flat,
        |_, _| slot,
    )
}

/// A straight tapered tube between two points.
///
/// `ra` and `rb` are the half-extents at each end: `x` sideways and `y` along
/// the direction nearest `up`.
pub(crate) fn strut(
    a: Vec3,
    b: Vec3,
    ra: Vec2,
    rb: Vec2,
    up: Vec3,
    segs: usize,
    slot: Slot,
) -> Geo {
    sweep(
        &[a, b],
        up,
        |i| if i == 0 { ra } else { rb },
        segs,
        Cap::Flat,
        Cap::Flat,
        |_, _| slot,
    )
}

/// A capsule: a tube with domed ends, the radius changing linearly.
pub(crate) fn capsule(a: Vec3, b: Vec3, ra: f32, rb: f32, segs: usize, slot: Slot) -> Geo {
    sweep(
        &[a, b],
        Vec3::Y,
        |i| Vec2::splat(if i == 0 { ra } else { rb }),
        segs,
        Cap::Round(ra),
        Cap::Round(rb),
        |_, _| slot,
    )
}

/// Revolve a profile about the Y axis.
///
/// Profile points are `(radius, y)`. Run the profile counter-clockwise in
/// that plane (up the outside, down the inside) for outward faces. An open
/// profile must start and end on the axis. `slot(profile_segment,
/// angle_segment)` picks the material of each quad.
pub(crate) fn lathe(
    profile: &[Vec2],
    closed: bool,
    segs: usize,
    slot: impl Fn(usize, usize) -> Slot,
) -> Geo {
    const AXIS_EPS: f32 = 1.0e-6;
    let mut geo = Geo::default();
    let n = profile.len();
    let mut first = Vec::with_capacity(n);
    let mut is_pole = Vec::with_capacity(n);
    for p in profile {
        first.push(geo.pos.len() as u32);
        if p.x <= AXIS_EPS {
            is_pole.push(true);
            geo.pos.push(Vec3::new(0.0, p.y, 0.0));
        } else {
            is_pole.push(false);
            for k in 0..segs {
                let (s, c) = (TAU * k as f32 / segs as f32).sin_cos();
                geo.pos.push(Vec3::new(p.x * c, p.y, p.x * s));
            }
        }
    }
    let spans = if closed { n } else { n - 1 };
    for j in 0..spans {
        let j2 = (j + 1) % n;
        for k in 0..segs {
            let k2 = ((k + 1) % segs) as u32;
            let k1 = k as u32;
            let s = slot(j, k);
            match (is_pole[j], is_pole[j2]) {
                (true, true) => {}
                (true, false) => geo.tri(first[j], first[j2] + k1, first[j2] + k2, s),
                (false, true) => geo.tri(first[j] + k1, first[j2], first[j] + k2, s),
                (false, false) => geo.quad(
                    first[j] + k1,
                    first[j2] + k1,
                    first[j2] + k2,
                    first[j] + k2,
                    s,
                ),
            }
        }
    }
    geo
}

/// A solid of revolution about the Y axis from a closed or on-axis profile,
/// all in one material.
pub(crate) fn turned(profile: &[Vec2], closed: bool, segs: usize, slot: Slot) -> Geo {
    lathe(profile, closed, segs, |_, _| slot)
}

/// A cylinder along Y from `y0` to `y1`.
pub(crate) fn cylinder(radius: f32, y0: f32, y1: f32, segs: usize, slot: Slot) -> Geo {
    turned(
        &[
            Vec2::new(0.0, y0),
            Vec2::new(radius, y0),
            Vec2::new(radius, y1),
            Vec2::new(0.0, y1),
        ],
        false,
        segs,
        slot,
    )
}

/// An ellipsoid centered on the origin.
pub(crate) fn ellipsoid(radii: Vec3, segs: usize, rings: usize, slot: Slot) -> Geo {
    let profile: Vec<Vec2> = (0..=rings)
        .map(|j| {
            let phi = -FRAC_PI_2 + std::f32::consts::PI * j as f32 / rings as f32;
            if j == 0 || j == rings {
                Vec2::new(0.0, phi.sin())
            } else {
                Vec2::new(phi.cos(), phi.sin())
            }
        })
        .collect();
    turned(&profile, false, segs, slot).scaled(radii)
}

/// A torus about the Y axis.
pub(crate) fn torus(major: f32, minor: f32, segs: usize, tube_segs: usize, slot: Slot) -> Geo {
    let profile: Vec<Vec2> = (0..tube_segs)
        .map(|j| {
            let (s, c) = (TAU * j as f32 / tube_segs as f32).sin_cos();
            Vec2::new(major + minor * c, minor * s)
        })
        .collect();
    turned(&profile, true, segs, slot)
}

/// Extrude an outline along Z from `z0` to `z1`, with chamfered rims.
///
/// The outline runs counter-clockwise seen from +Z and must be star-shaped
/// about its centroid. `bevel` is the chamfer size in meters; 0 gives square
/// edges.
pub(crate) fn extrude(outline: &[Vec2], z0: f32, z1: f32, bevel: f32, slot: Slot) -> Geo {
    let centroid = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
    let ring = |z: f32, inset: f32| -> Vec<Vec3> {
        outline
            .iter()
            .map(|p| {
                let d = *p - centroid;
                let len = d.length();
                let q = if len > inset {
                    centroid + d * ((len - inset) / len)
                } else {
                    *p
                };
                q.extend(z)
            })
            .collect()
    };
    let rings = if bevel > 0.0 {
        vec![
            ring(z0, bevel),
            ring(z0 + bevel, 0.0),
            ring(z1 - bevel, 0.0),
            ring(z1, bevel),
        ]
    } else {
        vec![ring(z0, 0.0), ring(z1, 0.0)]
    };
    loft_points(&rings, Cap::Flat, Cap::Flat, |_, _| slot)
}

/// Extrude the band between two outlines along Z from `z0` to `z1`.
///
/// Both outlines have the same number of points and run counter-clockwise
/// seen from +Z, the inner one inside the outer.
pub(crate) fn extrude_ring(outer: &[Vec2], inner: &[Vec2], z0: f32, z1: f32, slot: Slot) -> Geo {
    let mut geo = Geo::default();
    let n = outer.len();
    debug_assert_eq!(n, inner.len());
    let mut base = [0_u32; 4];
    for (i, (outline, z)) in [(outer, z0), (outer, z1), (inner, z0), (inner, z1)]
        .into_iter()
        .enumerate()
    {
        base[i] = geo.pos.len() as u32;
        geo.pos.extend(outline.iter().map(|p| p.extend(z)));
    }
    let [o0, o1, i0, i1] = base;
    for k in 0..n {
        let k2 = ((k + 1) % n) as u32;
        let k = k as u32;
        geo.quad(o0 + k, o0 + k2, o1 + k2, o1 + k, slot);
        geo.quad(i0 + k2, i0 + k, i1 + k, i1 + k2, slot);
        geo.quad(o1 + k, o1 + k2, i1 + k2, i1 + k, slot);
        geo.quad(o0 + k2, o0 + k, i0 + k, i0 + k2, slot);
    }
    geo
}

/// A box with slightly rounded long edges: a superellipse section lofted
/// along Z, centered on the origin. `half` is the half-size on each axis.
pub(crate) fn block(half: Vec3, exponent: f32, segs: usize, slot: Slot) -> Geo {
    let ring = |z: f32| {
        Ring::squircle(
            Vec3::new(0.0, 0.0, z),
            Vec3::X * half.x,
            Vec3::Y * half.y,
            exponent,
        )
    };
    loft(
        &[ring(-half.z), ring(half.z)],
        segs,
        Cap::Flat,
        Cap::Flat,
        |_, _| slot,
    )
}

/// Points along a Catmull-Rom spline through `points`, `per` samples per
/// span, ending exactly on the last point.
pub(crate) fn spline(points: &[Vec3], per: usize) -> Vec<Vec3> {
    let n = points.len();
    let at = |i: isize| points[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::with_capacity((n - 1) * per + 1);
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (
            at(i as isize - 1),
            at(i as isize),
            at(i as isize + 1),
            at(i as isize + 2),
        );
        for s in 0..per {
            let t = s as f32 / per as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            out.push(
                0.5 * ((2.0 * p1)
                    + (p2 - p0) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3),
            );
        }
    }
    out.push(points[n - 1]);
    out
}

/// Smooth interpolation through keyed values: a Catmull-Rom curve over
/// unevenly spaced keys. `t` outside the keys clamps to the end values.
pub(crate) fn keyed<const N: usize>(keys: &[(f32, [f32; N])], t: f32) -> [f32; N] {
    let n = keys.len();
    if t <= keys[0].0 {
        return keys[0].1;
    }
    if t >= keys[n - 1].0 {
        return keys[n - 1].1;
    }
    let i = (0..n - 1).find(|&i| t < keys[i + 1].0).unwrap_or(n - 2);
    let (t1, p1) = keys[i];
    let (t2, p2) = keys[i + 1];
    let span = t2 - t1;
    let s = (t - t1) / span;
    let s2 = s * s;
    let s3 = s2 * s;
    let mut out = [0.0; N];
    for c in 0..N {
        let slope = |a: usize, b: usize| (keys[b].1[c] - keys[a].1[c]) / (keys[b].0 - keys[a].0);
        let m1 = if i == 0 {
            slope(0, 1)
        } else {
            slope(i - 1, i + 1)
        };
        let m2 = if i + 2 >= n {
            slope(n - 2, n - 1)
        } else {
            slope(i, i + 2)
        };
        out[c] = (2.0 * s3 - 3.0 * s2 + 1.0) * p1[c]
            + (s3 - 2.0 * s2 + s) * span * m1
            + (-2.0 * s3 + 3.0 * s2) * p2[c]
            + (s3 - s2) * span * m2;
    }
    out
}

/// A rectangle with rounded corners in the XY plane, counter-clockwise seen
/// from +Z. `half` is the half-size, `radius` the corner radius, and each
/// corner gets `steps + 1` points.
pub(crate) fn rounded_rect(half: Vec2, radius: f32, steps: usize) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(4 * (steps + 1));
    for (corner, (sx, sy)) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
        .into_iter()
        .enumerate()
    {
        let centre = Vec2::new(sx * (half.x - radius), sy * (half.y - radius));
        for step in 0..=steps {
            let a = FRAC_PI_2 * (corner as f32 + step as f32 / steps as f32);
            out.push(centre + Vec2::new(a.cos(), a.sin()) * radius);
        }
    }
    out
}

/// Points on a circle in the XY plane, counter-clockwise seen from +Z.
pub(crate) fn circle(radius: f32, segs: usize) -> Vec<Vec2> {
    (0..segs)
        .map(|k| {
            let (s, c) = (TAU * k as f32 / segs as f32).sin_cos();
            Vec2::new(radius * c, radius * s)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check;

    fn part_of(geo: &Geo, crease: f32) -> Part {
        let mut builder = PartBuilder::new("test");
        let slot = builder.slot(crate::paint::fixed([0.5, 0.5, 0.5], 0.5, 0.0));
        let geo = geo.clone().reslotted(|_, _| slot);
        builder.add(&geo, crease);
        builder.finish()
    }

    fn assert_sound(name: &str, geo: &Geo) {
        let part = part_of(geo, 40.0);
        check::assert_part_valid(&part);
        let shells = check::shells(&part);
        assert!(!shells.is_empty(), "{name}: no shells");
        for shell in shells {
            assert!(shell.closed, "{name}: shell is not closed");
            assert!(
                shell.volume_m3 > 0.0,
                "{name}: shell faces inward (volume {})",
                shell.volume_m3
            );
        }
    }

    #[test]
    fn primitives_are_closed_and_face_outward() {
        let up = Vec3::Y;
        let path = spline(
            &[
                Vec3::ZERO,
                Vec3::new(0.1, 0.0, 0.0),
                Vec3::new(0.2, 0.1, 0.0),
                Vec3::new(0.2, 0.2, 0.1),
            ],
            6,
        );
        assert_sound("tube", &tube(&path, 0.012, 8, 0));
        assert_sound(
            "capsule",
            &capsule(Vec3::ZERO, Vec3::new(0.0, -0.4, 0.05), 0.05, 0.03, 12, 0),
        );
        assert_sound(
            "strut",
            &strut(
                Vec3::ZERO,
                Vec3::new(0.3, 0.3, 0.0),
                Vec2::new(0.02, 0.03),
                Vec2::new(0.01, 0.01),
                up,
                10,
                0,
            ),
        );
        assert_sound("cylinder", &cylinder(0.05, -0.1, 0.1, 12, 0));
        assert_sound("ellipsoid", &ellipsoid(Vec3::new(0.1, 0.2, 0.15), 12, 6, 0));
        assert_sound("torus", &torus(0.3, 0.014, 24, 8, 0));
        assert_sound("extrude", &extrude(&circle(0.1, 10), -0.01, 0.01, 0.0, 0));
        assert_sound(
            "bevelled extrude",
            &extrude(&circle(0.1, 10), -0.01, 0.01, 0.003, 0),
        );
        assert_sound(
            "rounded slab",
            &extrude(
                &rounded_rect(Vec2::new(0.03, 0.04), 0.008, 3),
                -0.01,
                0.01,
                0.002,
                0,
            ),
        );
        assert_sound(
            "ring",
            &extrude_ring(&circle(0.1, 16), &circle(0.07, 16), 0.0, 0.004, 0),
        );
        assert_sound("block", &block(Vec3::new(0.03, 0.01, 0.05), 6.0, 12, 0));
        assert_sound(
            "mirrored",
            &capsule(Vec3::X * 0.1, Vec3::new(0.2, -0.3, 0.0), 0.04, 0.03, 10, 0).mirrored_x(),
        );
    }

    #[test]
    fn cylinder_keeps_its_rim_sharp_and_its_wall_smooth() {
        let part = part_of(&cylinder(0.05, 0.0, 0.2, 16, 0), 40.0);
        let surface = &part.surfaces()[0];
        // 16 wall vertices at each end, 16 cap-rim vertices at each end, 2 poles.
        assert_eq!(surface.positions.len(), 16 * 4 + 2);
        for (p, n) in surface.positions.iter().zip(&surface.normals) {
            let radial = n[1].abs() < 1.0e-4;
            let axial = (n[1].abs() - 1.0).abs() < 1.0e-4;
            assert!(radial || axial, "normal {n:?} at {p:?} is neither");
        }
    }

    #[test]
    fn keyed_passes_through_its_keys() {
        let keys = [(0.0, [1.0, 0.0]), (0.5, [2.0, 1.0]), (1.0, [0.5, 4.0])];
        for (t, v) in keys {
            assert_eq!(keyed(&keys, t), v);
        }
        assert_eq!(keyed(&keys, -1.0), [1.0, 0.0]);
        assert_eq!(keyed(&keys, 2.0), [0.5, 4.0]);
    }

    #[test]
    fn spline_passes_through_its_points() {
        let points = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0)];
        let out = spline(&points, 4);
        assert_eq!(out.len(), 9);
        assert!(out[0].distance(points[0]) < 1.0e-6);
        assert!(out[4].distance(points[1]) < 1.0e-6);
        assert!(out[8].distance(points[2]) < 1.0e-6);
    }
}
