//! Course skyline and the next-kilometre chart. Pixels in, no Godot types.

use crate::elevation::Pt;

/// One gradient column. `y0` and `y1` are the top edge; the fill goes down to the base.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// Left edge, in pixels.
    pub x0: f32,
    /// Right edge, in pixels.
    pub x1: f32,
    /// Top at the left edge. Smaller is higher on the screen.
    pub y0: f32,
    /// Top at the right edge.
    pub y1: f32,
    /// Ramp step `0..6`.
    pub ramp: u8,
}

/// A profile drawn into a rectangle. Vectors keep their capacity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chart {
    /// Gradient columns. Empty when there is nothing to draw.
    pub columns: Vec<Column>,
    /// Elevation line, left to right.
    pub top: Vec<Pt>,
    /// Distance along the course for each [`Self::top`] point, in metres.
    pub top_m: Vec<f64>,
    /// Ridden portion, closed by the caller along the base.
    pub ridden: Vec<Pt>,
    /// The next kilometre of [`Self::top`], for the heavier line.
    pub near: Vec<Pt>,
    /// Rider position on the line.
    pub marker: Pt,
    /// Y of the base the columns stand on.
    pub base_y: f32,
    /// Left edge.
    pub left: f32,
    /// Right edge.
    pub right: f32,
}

impl Chart {
    fn clear(&mut self) {
        self.columns.clear();
        self.top.clear();
        self.top_m.clear();
        self.ridden.clear();
        self.near.clear();
    }
}

/// Where a chart sits. `base_y` is the bottom edge. Y grows downward.
#[derive(Clone, Copy, Debug)]
pub struct Plot {
    /// Left edge, in pixels.
    pub left: f32,
    /// Bottom edge, in pixels.
    pub base_y: f32,
    /// Width, in pixels.
    pub width: f32,
    /// Height, in pixels.
    pub height: f32,
    /// Column width, in pixels.
    pub bin_px: f32,
}

/// Ramp step for a grade, matching the gradient scale in `docs/ui.md`.
///
/// `0` is flat, `1` is downhill, then green through violet as the road rises.
#[must_use]
pub fn grade_ramp(grade_pct: f32) -> u8 {
    if grade_pct < -1.0 {
        1
    } else if grade_pct < 1.0 {
        0
    } else if grade_pct < 3.0 {
        2
    } else if grade_pct < 6.0 {
        3
    } else if grade_pct < 9.0 {
        4
    } else if grade_pct < 12.0 {
        5
    } else {
        6
    }
}

/// Whole-lap skyline standing on `plot.base_y`.
///
/// The floor sits 22 % of the range (at least 60 m) below the lowest point.
/// Columns behind the rider are omitted; [`Chart::ridden`] covers that part.
pub fn fill_skyline(
    points: &[(f64, f32)],
    length_m: f64,
    marker_m: f64,
    plot: Plot,
    chart: &mut Chart,
) {
    chart.clear();
    chart.base_y = plot.base_y;
    chart.left = plot.left;
    chart.right = plot.left + plot.width;
    if !drawable(points, length_m, plot) {
        return;
    }
    let columns = bins(plot);
    let mut raw = Vec::with_capacity(columns + 1);
    for index in 0..=columns {
        let distance = length_m * index as f64 / columns as f64;
        raw.push((distance, elevation_at(points, distance)));
    }
    let (floor, span) = skyline_scale(raw.iter().map(|(_, elevation)| *elevation));
    let point = |index: usize, elevation: f32| Pt {
        x: plot.left + plot.width * index as f32 / columns as f32,
        y: y_of(plot, floor, span, elevation, 1.0),
    };
    for (index, (_, elevation)) in raw.iter().enumerate() {
        chart.top.push(point(index, *elevation));
        chart.top_m.push(raw[index].0);
    }
    let marker_m = marker_m.clamp(0.0, length_m);
    let marker_e = elevation_at(points, marker_m);
    chart.marker = Pt {
        x: plot.left + plot.width * (marker_m as f32 / length_m as f32),
        y: y_of(plot, floor, span, marker_e, 1.0),
    };
    push_columns(
        &raw,
        plot,
        floor,
        span,
        1.0,
        Some(marker_m),
        &mut chart.columns,
    );
    chart.ridden.push(Pt {
        x: plot.left,
        y: plot.base_y,
    });
    for point in &chart.top {
        if point.x <= chart.marker.x + 0.01 {
            chart.ridden.push(*point);
        }
    }
    chart.ridden.push(chart.marker);
    chart.ridden.push(Pt {
        x: chart.marker.x,
        y: plot.base_y,
    });
    let near_end = (marker_m + 1000.0).min(length_m);
    chart.near.push(chart.marker);
    for (point, distance) in chart.top.iter().zip(chart.top_m.iter()) {
        if *distance > marker_m && *distance < near_end {
            chart.near.push(*point);
        }
    }
    if near_end < length_m - 1.0 {
        let index = (near_end / length_m * columns as f64).clamp(0.0, columns as f64);
        let elevation = elevation_at(points, near_end);
        chart.near.push(Pt {
            x: plot.left + plot.width * (index as f32 / columns as f32),
            y: y_of(plot, floor, span, elevation, 1.0),
        });
    } else if let Some(last) = chart.top.last() {
        chart.near.push(*last);
    }
}

/// Next kilometre. `samples` are `(metres ahead, elevation)`.
///
/// The vertical scale spans at least 60 m, so a flat road stays flat.
/// A grade steeper than 40 % is treated as a lap-line jump and drawn flat-coloured.
pub fn fill_next_km(samples: &[(f64, f32)], plot: Plot, chart: &mut Chart) {
    chart.clear();
    chart.base_y = plot.base_y;
    chart.left = plot.left;
    chart.right = plot.left + plot.width;
    if samples.len() < 2 || plot.width < 4.0 || plot.height < 4.0 {
        return;
    }
    let window = samples
        .last()
        .map(|(ahead, _)| *ahead)
        .unwrap_or(1000.0)
        .max(1.0);
    let columns = bins(plot);
    let mut raw = Vec::with_capacity(columns + 1);
    for index in 0..=columns {
        let ahead = window * index as f64 / columns as f64;
        raw.push((ahead, elevation_along(samples, ahead)));
    }
    let low = raw
        .iter()
        .map(|(_, elevation)| *elevation)
        .fold(f32::MAX, f32::min);
    let high = raw
        .iter()
        .map(|(_, elevation)| *elevation)
        .fold(f32::MIN, f32::max);
    let span = (high - low).max(60.0);
    for (index, (ahead, elevation)) in raw.iter().enumerate() {
        chart.top.push(Pt {
            x: plot.left + plot.width * index as f32 / columns as f32,
            y: y_of(plot, low, span, *elevation, 0.92),
        });
        chart.top_m.push(*ahead);
    }
    chart.marker = chart.top.first().copied().unwrap_or(Pt {
        x: plot.left,
        y: plot.base_y,
    });
    push_columns(&raw, plot, low, span, 0.92, None, &mut chart.columns);
}

/// Resample the lap profile into `out` as `(metres ahead, elevation)`, wrapping at the start line.
pub fn fill_window(
    points: &[(f64, f32)],
    length_m: f64,
    marker_m: f64,
    window_m: f64,
    step_m: f64,
    out: &mut Vec<(f64, f32)>,
) {
    out.clear();
    if points.len() < 2 || length_m <= 0.0 || window_m <= 0.0 || step_m <= 0.0 {
        return;
    }
    let mut ahead = 0.0;
    loop {
        let distance = (marker_m + ahead).rem_euclid(length_m);
        out.push((ahead, elevation_at(points, distance)));
        if ahead >= window_m - 1.0e-6 || out.len() >= 4096 {
            break;
        }
        let next = (ahead + step_m).min(window_m);
        if next <= ahead {
            break;
        }
        ahead = next;
    }
}

fn drawable(points: &[(f64, f32)], length_m: f64, plot: Plot) -> bool {
    points.len() >= 2 && length_m > 0.0 && plot.width >= 4.0 && plot.height >= 4.0
}

fn bins(plot: Plot) -> usize {
    (plot.width / plot.bin_px.max(1.0)).round().max(2.0) as usize
}

fn skyline_scale(elevations: impl Iterator<Item = f32>) -> (f32, f32) {
    let mut low = f32::MAX;
    let mut high = f32::MIN;
    for elevation in elevations {
        low = low.min(elevation);
        high = high.max(elevation);
    }
    if !low.is_finite() {
        return (0.0, 60.0);
    }
    let floor = low - 0.22 * (high - low).max(60.0);
    let span = (high - floor).max(60.0);
    (floor, span)
}

fn y_of(plot: Plot, floor: f32, span: f32, elevation: f32, headroom: f32) -> f32 {
    let span = span.max(1.0);
    plot.base_y - (elevation - floor) / span * plot.height * headroom
}

fn push_columns(
    raw: &[(f64, f32)],
    plot: Plot,
    floor: f32,
    span: f32,
    headroom: f32,
    skip_until: Option<f64>,
    out: &mut Vec<Column>,
) {
    let columns = raw.len().saturating_sub(1);
    for index in 0..columns {
        let (d0, e0) = raw[index];
        let (d1, e1) = raw[index + 1];
        if skip_until.is_some_and(|marker| d1 <= marker) {
            continue;
        }
        let run = (d1 - d0).max(1.0e-3);
        let mut grade = (e1 - e0) / run as f32 * 100.0;
        if grade.abs() > 40.0 {
            grade = 0.0;
        }
        out.push(Column {
            x0: plot.left + plot.width * index as f32 / columns as f32,
            x1: plot.left + plot.width * (index + 1) as f32 / columns as f32,
            y0: y_of(plot, floor, span, e0, headroom),
            y1: y_of(plot, floor, span, e1, headroom),
            ramp: grade_ramp(grade),
        });
    }
}

fn elevation_at(points: &[(f64, f32)], distance: f64) -> f32 {
    elevation_along(points, distance)
}

fn elevation_along(points: &[(f64, f32)], distance: f64) -> f32 {
    let Some((first_d, first_e)) = points.first().copied() else {
        return 0.0;
    };
    if distance <= first_d {
        return first_e;
    }
    for window in points.windows(2) {
        let (d0, e0) = window[0];
        let (d1, e1) = window[1];
        if distance <= d1 {
            let span = (d1 - d0).max(1.0e-6);
            let t = ((distance - d0) / span) as f32;
            return e0 + (e1 - e0) * t;
        }
    }
    points
        .last()
        .map(|(_, elevation)| *elevation)
        .unwrap_or(first_e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot() -> Plot {
        Plot {
            left: 10.0,
            base_y: 100.0,
            width: 200.0,
            height: 30.0,
            bin_px: 4.0,
        }
    }

    #[test]
    fn grade_steps_match_the_ramp() {
        assert_eq!(grade_ramp(-1.5), 1);
        assert_eq!(grade_ramp(-1.0), 0);
        assert_eq!(grade_ramp(0.0), 0);
        assert_eq!(grade_ramp(1.0), 2);
        assert_eq!(grade_ramp(4.0), 3);
        assert_eq!(grade_ramp(7.0), 4);
        assert_eq!(grade_ramp(10.0), 5);
        assert_eq!(grade_ramp(12.0), 6);
    }

    #[test]
    fn the_marker_tracks_the_lap_and_a_flat_road_stays_flat() {
        let points = [(0.0, 0.0), (500.0, 80.0), (1000.0, 10.0)];
        let mut chart = Chart::default();
        fill_skyline(&points, 1000.0, 500.0, plot(), &mut chart);
        assert!(chart.top.len() >= 2);
        assert!((chart.marker.x - (10.0 + 100.0)).abs() < 1.5);
        assert!(chart.marker.y < chart.base_y);
        assert!(chart.columns.iter().any(|column| column.ramp > 0));
        let ridden_right = chart.ridden.iter().map(|point| point.x).fold(0.0, f32::max);
        assert!(ridden_right <= chart.marker.x + 0.2);

        let flat = [(0.0, 5.0), (1000.0, 5.0)];
        fill_skyline(&flat, 1000.0, 0.0, plot(), &mut chart);
        let y0 = chart.top[0].y;
        assert!(chart.top.iter().all(|point| (point.y - y0).abs() < 0.5));
        let rise = chart.base_y - y0;
        assert!(
            rise < plot().height * 0.5,
            "a flat road should not fill the skyline"
        );
    }

    #[test]
    fn the_next_kilometre_wraps_and_colours_a_climb() {
        let points = [(0.0, 0.0), (100.0, 0.0), (200.0, 20.0), (1000.0, 20.0)];
        let mut window = Vec::new();
        fill_window(&points, 1000.0, 900.0, 1000.0, 50.0, &mut window);
        assert!(window.len() >= 3);
        assert!(window.last().unwrap().0 >= 999.0);
        let mut chart = Chart::default();
        fill_next_km(&window, plot(), &mut chart);
        assert!(chart.columns.len() >= 2);
        assert_eq!(chart.marker.x, chart.left);
        let climbed = chart.columns.iter().any(|column| column.ramp >= 2);
        assert!(climbed);
    }
}
