//! Centreline queries for a road that folds back on itself.
//!
//! The nearest sample is what the terrain conforms to. A second leg within
//! reach imposes a ceiling, so a higher hairpin cannot lift the ground
//! through the road below it.

use std::collections::HashMap;

const CELL_M: f32 = 16.0;

/// Samples of a centreline, indexed on the ground plane.
pub struct RoadIndex {
    samples: Vec<[f32; 3]>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

impl RoadIndex {
    /// Index `samples` as `[x, y, z]` metres.
    #[must_use]
    pub fn build(samples: &[[f32; 3]]) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, sample) in samples.iter().enumerate() {
            let key = (cell_of(sample[0]), cell_of(sample[2]));
            cells
                .entry(key)
                .or_default()
                .push(u32::try_from(index).unwrap_or(u32::MAX));
        }
        Self {
            samples: samples.to_vec(),
            cells,
        }
    }

    /// Smooth road height. `(distance_m, tight_y, wide_y)`.
    ///
    /// Both heights are Gaussians over every sample, so neither has a cutoff
    /// and neither jumps when the closest leg changes.
    #[must_use]
    pub fn blend_heights(&self, x: f32, z: f32) -> Option<(f32, f32, f32)> {
        if self.samples.is_empty() {
            return None;
        }
        let mut best_d = f32::MAX;
        let mut best_y = 0.0_f32;
        let mut near_w = 0.0_f32;
        let mut near_y = 0.0_f32;
        let mut far_w = 0.0_f32;
        let mut far_y = 0.0_f32;
        let inv_near = 1.0 / (2.0 * 10.0 * 10.0);
        let inv_far = 1.0 / (2.0 * 55.0 * 55.0);
        for sample in &self.samples {
            let dx = sample[0] - x;
            let dz = sample[2] - z;
            let d2 = dx * dx + dz * dz;
            if d2 < best_d {
                best_d = d2;
                best_y = sample[1];
            }
            let wn = (-d2 * inv_near).exp();
            let wf = (-d2 * inv_far).exp();
            near_w += wn;
            near_y += wn * sample[1];
            far_w += wf;
            far_y += wf * sample[1];
        }
        let wide = if far_w > 1.0e-20 {
            far_y / far_w
        } else {
            best_y
        };
        let tight = if near_w > 1.0e-20 {
            near_y / near_w
        } else {
            wide
        };
        Some((best_d.sqrt(), tight, wide))
    }

    /// Closest sample on the whole centreline, as `(distance_m, height_m)`.
    #[must_use]
    pub fn nearest_anywhere(&self, x: f32, z: f32) -> Option<(f32, f32)> {
        let mut best_d = f32::MAX;
        let mut best_y = 0.0;
        for sample in &self.samples {
            let dx = sample[0] - x;
            let dz = sample[2] - z;
            let d = dx * dx + dz * dz;
            if d < best_d {
                best_d = d;
                best_y = sample[1];
            }
        }
        (best_d < 1.0e20).then_some((best_d.sqrt(), best_y))
    }

    /// A nearby sample, searched sparsely. Used when the point is far from every road.
    #[must_use]
    pub fn rough_nearest(&self, x: f32, z: f32) -> Option<(f32, f32)> {
        if self.samples.is_empty() {
            return None;
        }
        let stride = (self.samples.len() / 500).max(1);
        let mut best_i = 0_usize;
        let mut best_d = f32::MAX;
        let mut index = 0_usize;
        while index < self.samples.len() {
            let sample = self.samples[index];
            let dx = sample[0] - x;
            let dz = sample[2] - z;
            let d = dx * dx + dz * dz;
            if d < best_d {
                best_d = d;
                best_i = index;
            }
            index += stride;
        }
        let start = best_i.saturating_sub(stride);
        let end = (best_i + stride + 1).min(self.samples.len());
        let mut best_y = self.samples[best_i][1];
        for sample in &self.samples[start..end] {
            let dx = sample[0] - x;
            let dz = sample[2] - z;
            let d = dx * dx + dz * dz;
            if d < best_d {
                best_d = d;
                best_y = sample[1];
            }
        }
        Some((best_d.sqrt(), best_y))
    }

    /// Closest sample within `radius_m`, as `(distance_m, height_m)`.
    #[must_use]
    pub fn nearest(&self, x: f32, z: f32, radius_m: f32) -> Option<(f32, f32)> {
        let mut best_d = radius_m;
        let mut best_y = 0.0;
        let mut found = false;
        self.visit_within(x, z, radius_m, |distance, height| {
            if !found || distance < best_d {
                best_d = distance;
                best_y = height;
                found = true;
            }
        });
        found.then_some((best_d, best_y))
    }

    /// Every sample within `radius_m`. `visit` receives distance and height.
    pub fn visit_within(&self, x: f32, z: f32, radius_m: f32, mut visit: impl FnMut(f32, f32)) {
        if !x.is_finite() || !z.is_finite() || !radius_m.is_finite() || radius_m < 0.0 {
            return;
        }
        let radius_sq = radius_m * radius_m;
        let min_x = cell_of(x - radius_m);
        let max_x = cell_of(x + radius_m);
        let min_z = cell_of(z - radius_m);
        let max_z = cell_of(z + radius_m);
        for iz in min_z..=max_z {
            for ix in min_x..=max_x {
                let Some(bucket) = self.cells.get(&(ix, iz)) else {
                    continue;
                };
                for &index in bucket {
                    let Some(sample) = self.samples.get(index as usize) else {
                        continue;
                    };
                    let dx = sample[0] - x;
                    let dz = sample[2] - z;
                    let distance_sq = dx * dx + dz * dz;
                    if distance_sq <= radius_sq {
                        visit(distance_sq.sqrt(), sample[1]);
                    }
                }
            }
        }
    }
}

/// Ground height following one leg. Matches the single-road blend: the bed
/// sits 0.4 m under the ribbon inside 5.2 m, then eases out to `free` by 42 m.
#[must_use]
pub fn conform_height(distance_m: f32, road_y: f32, free: f32) -> f32 {
    if distance_m <= 5.2 {
        return road_y - 0.4;
    }
    // A shallow bank. The old 0.55 rise, on a coarse quad, stood up as a spike.
    let allowance = (distance_m - 5.2) * 0.28;
    let capped = free.clamp(road_y - allowance, road_y + allowance);
    let blend = smoothstep(5.2, 70.0, distance_m);
    (road_y - 0.4) + (capped - (road_y - 0.4)) * blend
}

/// Ceiling from one leg. `None` inside the road bed, where [`conform_height`]
/// already owns the surface. Outside it, the ground may not rise through the
/// deck of this leg.
#[must_use]
pub fn leg_cap(distance_m: f32, road_y: f32) -> Option<f32> {
    if distance_m <= 5.0 {
        return None;
    }
    Some(road_y - 0.02 + (distance_m - 5.0) * 1.8)
}

fn cell_of(value: f32) -> i32 {
    (value / CELL_M).floor() as i32
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ladder() -> RoadIndex {
        let mut samples = Vec::new();
        for step in 0..50 {
            let z = step as f32 * 4.0;
            samples.push([0.0, 0.0, z]);
            samples.push([67.0, 28.0, z]);
        }
        RoadIndex::build(&samples)
    }

    fn height_at(index: &RoadIndex, x: f32, z: f32) -> f32 {
        let (distance, road_y) = index.nearest(x, z, 80.0).unwrap();
        let mut height = conform_height(distance, road_y, 90.0);
        index.visit_within(x, z, 80.0, |leg_distance, leg_y| {
            if let Some(cap) = leg_cap(leg_distance, leg_y) {
                height = height.min(cap);
            }
        });
        height
    }

    #[test]
    fn parallel_legs_keep_their_own_height() {
        let index = ladder();
        let low = height_at(&index, 0.0, 100.0);
        let high = height_at(&index, 67.0, 100.0);
        assert!(low < 1.0, "lower leg buried at {low}");
        assert!(high > 26.0, "upper leg dropped to {high}");
    }

    #[test]
    fn a_point_on_either_leg_is_inside_the_keep_out() {
        let index = ladder();
        let (low, _) = index.nearest(3.0, 80.0, 40.0).unwrap();
        let (high, _) = index.nearest(64.0, 80.0, 40.0).unwrap();
        let (middle, _) = index.nearest(33.5, 80.0, 40.0).unwrap();
        assert!(low < 5.0, "{low}");
        assert!(high < 5.0, "{high}");
        assert!(middle > 30.0, "{middle}");
    }

    #[test]
    fn a_hairpin_uses_the_near_end_not_the_far_one() {
        let mut samples = Vec::new();
        let steps = 24;
        for step in 0..=steps {
            let turns = step as f32 / steps as f32;
            let angle = turns * std::f32::consts::PI;
            let radius = 34.0;
            samples.push([angle.cos() * radius, turns * 16.0, angle.sin() * radius]);
        }
        let index = RoadIndex::build(&samples);
        let (distance, height) = index.nearest(34.0, 2.0, 50.0).unwrap();
        assert!(distance < 4.0, "{distance}");
        assert!(height < 3.0, "low end took the high end's height {height}");
        let (distance, height) = index.nearest(-34.0, 2.0, 50.0).unwrap();
        assert!(distance < 4.0, "{distance}");
        assert!(height > 13.0, "high end took the low end's height {height}");
    }
}
