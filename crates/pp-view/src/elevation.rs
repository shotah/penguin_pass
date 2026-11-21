//! Elevation-strip geometry. Points in, a polyline in a rectangle out.

/// One point in strip pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    /// Pixels from the left.
    pub x: f32,
    /// Pixels from the top.
    pub y: f32,
}

/// Everything the strip draws for one frame, in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    /// Filled profile, closed along the bottom.
    pub fill: Vec<Pt>,
    /// The whole profile line.
    pub line: Vec<Pt>,
    /// The next 400 m, recoloured by grade.
    pub ahead: Vec<Pt>,
    /// Rider marker, top to bottom.
    pub marker_top: Pt,
    /// Rider marker, bottom.
    pub marker_bottom: Pt,
}

/// Map the lap profile into `width` by `height` pixels.
///
/// `None` when the rectangle or the profile is too small to draw.
pub fn layout(
    width: f32,
    height: f32,
    points: &[(f64, f32)],
    lap_m: f64,
    marker_m: f64,
) -> Option<Strip> {
    if width < 4.0 || height < 4.0 || points.len() < 2 || lap_m <= 0.0 {
        return None;
    }
    let pad = 8.0_f32;
    let inner_w = (width - pad * 2.0).max(1.0);
    let inner_h = (height - pad * 2.0).max(1.0);
    let mut min_e = f32::MAX;
    let mut max_e = f32::MIN;
    for (_, elevation) in points {
        min_e = min_e.min(*elevation);
        max_e = max_e.max(*elevation);
    }
    let span = (max_e - min_e).max(1.0);
    let point = |distance: f64, elevation: f32| -> Pt {
        let x = pad + (distance as f32 / lap_m as f32).clamp(0.0, 1.0) * inner_w;
        let y = pad + inner_h - ((elevation - min_e) / span) * inner_h;
        Pt { x, y }
    };
    let mut fill = vec![Pt {
        x: pad,
        y: pad + inner_h,
    }];
    let mut line = Vec::with_capacity(points.len());
    for (distance, elevation) in points {
        let plotted = point(*distance, *elevation);
        fill.push(plotted);
        line.push(plotted);
    }
    fill.push(Pt {
        x: pad + inner_w,
        y: pad + inner_h,
    });
    let marker_x = pad + (marker_m as f32 / lap_m as f32).clamp(0.0, 1.0) * inner_w;
    Some(Strip {
        ahead: upcoming(points, marker_m, lap_m, &point),
        fill,
        line,
        marker_top: Pt {
            x: marker_x,
            y: pad,
        },
        marker_bottom: Pt {
            x: marker_x,
            y: pad + inner_h,
        },
    })
}

fn upcoming(
    points: &[(f64, f32)],
    marker: f64,
    lap: f64,
    point: &impl Fn(f64, f32) -> Pt,
) -> Vec<Pt> {
    let end = (marker + 400.0).min(lap);
    let mut line = Vec::new();
    for window in points.windows(2) {
        let (d0, e0) = window[0];
        let (d1, e1) = window[1];
        if d1 < marker || d0 > end {
            continue;
        }
        let from = d0.max(marker);
        let to = d1.min(end);
        let span_d = (d1 - d0).max(1.0e-3);
        let e_from = e0 + (e1 - e0) * ((from - d0) / span_d) as f32;
        let e_to = e0 + (e1 - e0) * ((to - d0) / span_d) as f32;
        if line.is_empty() {
            line.push(point(from, e_from));
        }
        line.push(point(to, e_to));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_tracks_distance_and_the_line_stays_inside() {
        let points = [(0.0, 0.0), (500.0, 40.0), (1000.0, 10.0)];
        assert!(layout(2.0, 40.0, &points, 1000.0, 0.0).is_none());
        let strip = layout(200.0, 40.0, &points, 1000.0, 500.0).unwrap();
        assert!(strip.line.len() >= 2);
        assert!(strip.fill.len() == strip.line.len() + 2);
        assert!(strip.marker_top.x > strip.line[0].x);
        assert!(strip.marker_top.x < strip.line.last().unwrap().x);
        assert!(strip.ahead.len() >= 2);
        let wrapped = layout(200.0, 40.0, &points, 1000.0, 0.0).unwrap();
        assert!(wrapped.marker_top.x < strip.marker_top.x);
    }

    #[test]
    fn a_flat_profile_still_has_height() {
        let points = [(0.0, 5.0), (10.0, 5.0)];
        let strip = layout(100.0, 30.0, &points, 10.0, 10.0).unwrap();
        assert!(strip.line[0].y.is_finite());
        assert!(strip.marker_top.y < strip.marker_bottom.y);
    }
}
