//! Map and elevation profile as a hand-written SVG.

use pp_course::Course;

/// Draw one course: plan view on top, elevation profile below.
///
/// The road is coloured by grade. The sheet includes a start marker, a
/// direction arrow, a scale bar, and the length, ascent, and max grade.
#[must_use]
pub fn render_svg(course: &Course) -> String {
    let length = course.length_m();
    let samples = ((length / 12.0).round() as usize).clamp(400, 1_600);
    let mut pts = Vec::with_capacity(samples);
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    let mut max_g = 0.0_f32;
    let mut min_g = 0.0_f32;
    for i in 0..samples {
        let d = length * (i as f64) / (samples as f64);
        let p = course.position_at(d);
        let x = f64::from(p[0]);
        let z = f64::from(p[2]);
        let g = course.grade_pct_at(d);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
        max_g = max_g.max(g);
        min_g = min_g.min(g);
        pts.push((x, z, g, d));
    }
    let span = (max_x - min_x).max(max_z - min_z).max(1.0);
    let pad = span * 0.08;
    min_x -= pad;
    max_x += pad;
    min_z -= pad;
    max_z += pad;
    let map_w = 920.0;
    let map_h = 560.0;
    let profile_h = 220.0;
    let width = 980.0;
    let height = 40.0 + map_h + 36.0 + profile_h + 48.0;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    );
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#f3efe4\"/>\n");
    let title = xml_escape(course.name());
    out.push_str(&format!(
        "<text x=\"24\" y=\"28\" font-family=\"sans-serif\" font-size=\"18\" fill=\"#2b2a26\">{title}</text>\n"
    ));

    let ox = 30.0;
    let oy = 44.0;
    let sx = map_w / (max_x - min_x).max(1.0);
    let sy = map_h / (max_z - min_z).max(1.0);
    let s = sx.min(sy);
    let used_w = (max_x - min_x) * s;
    let used_h = (max_z - min_z) * s;
    let x0 = ox + (map_w - used_w) * 0.5;
    let y0 = oy + (map_h - used_h) * 0.5;
    let map_xy = |x: f64, z: f64| -> (f64, f64) {
        (x0 + (x - min_x) * s, y0 + (max_z - z) * s)
    };

    // Casing, then a coloured centerline.
    let mut casing = String::from("<polyline fill=\"none\" stroke=\"#4a4338\" stroke-width=\"7\" stroke-linejoin=\"round\" stroke-linecap=\"round\" points=\"");
    for (x, z, _, _) in &pts {
        let (px, py) = map_xy(*x, *z);
        casing.push_str(&format!("{px:.1},{py:.1} "));
    }
    casing.push_str("\"/>\n");
    out.push_str(&casing);
    for w in pts.windows(2) {
        let (x0p, y0p) = map_xy(w[0].0, w[0].1);
        let (x1p, y1p) = map_xy(w[1].0, w[1].1);
        let color = grade_color(w[0].2);
        out.push_str(&format!(
            "<line x1=\"{x0p:.1}\" y1=\"{y0p:.1}\" x2=\"{x1p:.1}\" y2=\"{y1p:.1}\" stroke=\"{color}\" stroke-width=\"3.5\" stroke-linecap=\"round\"/>\n"
        ));
    }

    let (sx0, sy0) = map_xy(pts[0].0, pts[0].1);
    out.push_str(&format!(
        "<circle cx=\"{sx0:.1}\" cy=\"{sy0:.1}\" r=\"7\" fill=\"#fffef8\" stroke=\"#1d1c19\" stroke-width=\"2\"/>\n"
    ));
    let aim = course.position_at(length * 0.012);
    let (ax, ay) = map_xy(f64::from(aim[0]), f64::from(aim[2]));
    out.push_str(&format!(
        "<line x1=\"{sx0:.1}\" y1=\"{sy0:.1}\" x2=\"{ax:.1}\" y2=\"{ay:.1}\" stroke=\"#1d1c19\" stroke-width=\"2\" marker-end=\"url(#arrow)\"/>\n"
    ));
    out.push_str("<defs><marker id=\"arrow\" markerWidth=\"8\" markerHeight=\"8\" refX=\"6\" refY=\"3\" orient=\"auto\"><path d=\"M0,0 L7,3 L0,6 Z\" fill=\"#1d1c19\"/></marker></defs>\n");

    let bar = nice_bar((max_x - min_x).max(max_z - min_z));
    let bar_px = bar * s;
    let bx = ox + 8.0;
    let by = oy + map_h - 16.0;
    out.push_str(&format!(
        "<line x1=\"{bx:.1}\" y1=\"{by:.1}\" x2=\"{:.1}\" y2=\"{by:.1}\" stroke=\"#1d1c19\" stroke-width=\"2\"/>\n",
        bx + bar_px
    ));
    out.push_str(&format!(
        "<text x=\"{bx:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"12\" fill=\"#1d1c19\">{label}</text>\n",
        by - 6.0,
        label = bar_label(bar),
    ));

    let prof_y = oy + map_h + 28.0;
    let profile = course.profile((length / 500.0).clamp(8.0, 80.0));
    let max_e = profile
        .iter()
        .fold(1.0_f32, |acc, p| acc.max(p.elevation_m))
        .max(1.0);
    let mut poly = format!("M {ox:.1} {:.1}", prof_y + profile_h);
    for (i, point) in profile.iter().enumerate() {
        let px = ox + map_w * (i as f64) / (profile.len().saturating_sub(1).max(1) as f64);
        let py = prof_y + profile_h
            - f64::from(point.elevation_m) / f64::from(max_e) * (profile_h - 16.0);
        poly.push_str(&format!(" L {px:.1} {py:.1}"));
    }
    poly.push_str(&format!(
        " L {:.1} {:.1} Z",
        ox + map_w,
        prof_y + profile_h
    ));
    out.push_str(&format!(
        "<path d=\"{poly}\" fill=\"#d9c7a2\" stroke=\"#6b4f2a\" stroke-width=\"1.5\"/>\n"
    ));
    let ascent = course.total_ascent_m();
    out.push_str(&format!(
        "<text x=\"24\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"14\" fill=\"#2b2a26\">{} km    ascent {:.0} m    grade {:.1}% to {:.1}%</text>\n",
        height - 18.0,
        length / 1000.0,
        ascent,
        min_g,
        max_g,
    ));
    out.push_str("</svg>\n");
    out
}

/// Six maps of the same distance and terrain, one per seed label.
#[must_use]
pub fn render_contact_sheet(panels: &[(String, &Course)]) -> String {
    let cols = 3;
    let rows = panels.len().div_ceil(cols).max(1);
    let cell_w = 460.0;
    let cell_h = 340.0;
    let width = cols as f64 * cell_w + 20.0;
    let height = rows as f64 * cell_h + 20.0;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    );
    out.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#f3efe4\"/>\n");
    for (i, (label, course)) in panels.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let ox = 10.0 + col as f64 * cell_w;
        let oy = 10.0 + row as f64 * cell_h;
        out.push_str(&panel(ox, oy, cell_w - 16.0, cell_h - 16.0, label, course));
    }
    out.push_str("</svg>\n");
    out
}

fn panel(ox: f64, oy: f64, w: f64, h: f64, label: &str, course: &Course) -> String {
    let length = course.length_m();
    let n = 500;
    let mut pts = Vec::with_capacity(n);
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for i in 0..n {
        let p = course.position_at(length * (i as f64) / (n as f64));
        let x = f64::from(p[0]);
        let z = f64::from(p[2]);
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
        pts.push((x, z, course.grade_pct_at(length * (i as f64) / (n as f64))));
    }
    let map_h = h * 0.72;
    let s = ((w - 16.0) / (max_x - min_x).max(1.0)).min(map_h / (max_z - min_z).max(1.0));
    let mut body = format!(
        "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"13\" fill=\"#2b2a26\">{}</text>\n",
        ox + 4.0,
        oy + 16.0,
        xml_escape(label),
    );
    let y_base = oy + 24.0;
    for win in pts.windows(2) {
        let x0 = ox + 8.0 + (win[0].0 - min_x) * s;
        let y0 = y_base + (max_z - win[0].1) * s;
        let x1 = ox + 8.0 + (win[1].0 - min_x) * s;
        let y1 = y_base + (max_z - win[1].1) * s;
        body.push_str(&format!(
            "<line x1=\"{x0:.1}\" y1=\"{y0:.1}\" x2=\"{x1:.1}\" y2=\"{y1:.1}\" stroke=\"{}\" stroke-width=\"2.2\"/>\n",
            grade_color(win[0].2)
        ));
    }
    let (sx, sy) = (
        ox + 8.0 + (pts[0].0 - min_x) * s,
        y_base + (max_z - pts[0].1) * s,
    );
    body.push_str(&format!(
        "<circle cx=\"{sx:.1}\" cy=\"{sy:.1}\" r=\"3.5\" fill=\"#1d1c19\"/>\n"
    ));
    body
}

fn grade_color(grade: f32) -> String {
    let t = (grade / 8.0).clamp(-1.0, 1.0);
    if t >= 0.0 {
        mix("#7d8c74", "#b13328", t)
    } else {
        mix("#7d8c74", "#1f6aa5", -t)
    }
}

fn mix(a: &str, b: &str, t: f32) -> String {
    let (ar, ag, ab) = hex(a);
    let (br, bg, bb) = hex(b);
    let lerp = |p: u8, q: u8| -> u8 { (f32::from(p) + (f32::from(q) - f32::from(p)) * t).round() as u8 };
    format!("#{:02x}{:02x}{:02x}", lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

fn hex(s: &str) -> (u8, u8, u8) {
    let n = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0);
    (((n >> 16) & 0xff) as u8, ((n >> 8) & 0xff) as u8, (n & 0xff) as u8)
}

fn nice_bar(span_m: f64) -> f64 {
    for bar in [100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0] {
        if bar < span_m * 0.28 {
            continue;
        }
        return bar;
    }
    10_000.0
}

fn bar_label(meters: f64) -> String {
    if meters >= 1000.0 {
        format!("{} km", (meters / 1000.0).round())
    } else {
        format!("{meters:.0} m")
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
