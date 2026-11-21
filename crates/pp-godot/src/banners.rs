//! Start and finish arches for climbs and sprints. The world crate is not touched.

use godot::classes::{Label3D, Node3D, StandardMaterial3D};
use godot::obj::Gd;
use godot::prelude::*;
use pp_session::{SegmentInfo, SegmentKind};

use crate::mesh_util::{add_box, standard_material};
use crate::road::flat_right;

/// Banner arches for one course.
pub struct Banners {
    root: Gd<Node3D>,
    key: String,
    ink: Gd<StandardMaterial3D>,
    snow: Gd<StandardMaterial3D>,
    dark: Gd<StandardMaterial3D>,
}

impl Banners {
    /// Parent the arches under `parent`.
    pub fn new(parent: &mut Gd<Node3D>) -> Self {
        let mut root = Node3D::new_alloc();
        root.set_name("Banners");
        parent.add_child(&root);
        Self {
            root,
            key: String::new(),
            ink: standard_material(crate::ui_style::INK, 0.6, 0.0),
            snow: standard_material(crate::ui_style::SNOW, 0.4, 0.0),
            dark: standard_material(Color::from_rgb(0.08, 0.08, 0.1), 0.5, 0.0),
        }
    }

    /// Rebuild when the segment list changes. Laps have no arch.
    pub fn sync(&mut self, segments: &[SegmentInfo], course: &pp_course::Course) {
        let key = segments
            .iter()
            .map(|segment| format!("{}:{:.1}:{:.1}", segment.id, segment.start_m, segment.end_m))
            .collect::<Vec<_>>()
            .join("|");
        if key == self.key {
            return;
        }
        self.key = key;
        let children = self.root.get_children();
        for child in children.iter_shared() {
            self.root.remove_child(&child);
            child.free();
        }
        for segment in segments {
            if matches!(segment.kind, SegmentKind::Lap) {
                continue;
            }
            self.arch(course, segment.start_m, Some(&segment.name), false);
            self.arch(course, segment.end_m, None, true);
        }
    }

    fn arch(
        &mut self,
        course: &pp_course::Course,
        distance_m: f64,
        name: Option<&str>,
        chequer: bool,
    ) {
        let position = course.position_at(distance_m);
        let tangent = course.tangent_at(distance_m);
        let forward = Vector3::new(tangent[0], tangent[1], tangent[2]);
        let right = flat_right(forward);
        let mut up = right.cross(forward);
        if up.length_squared() < 1.0e-8 {
            up = Vector3::UP;
        } else {
            up = up.normalized();
            if up.y < 0.0 {
                up = -up;
            }
        }
        let mut node = Node3D::new_alloc();
        node.set_name(if chequer { "Finish" } else { "Start" });
        let origin = Vector3::new(position[0], position[1], position[2]);
        node.set_transform(Transform3D::new(
            Basis::from_cols(right, up, -forward),
            origin,
        ));
        let span = 3.6;
        let height = 4.6;
        add_box(
            &mut node,
            "PostL",
            Vector3::new(0.28, height, 0.28),
            Vector3::new(-span, height * 0.5, 0.0),
            &self.ink,
        );
        add_box(
            &mut node,
            "PostR",
            Vector3::new(0.28, height, 0.28),
            Vector3::new(span, height * 0.5, 0.0),
            &self.ink,
        );
        if chequer {
            let cells = 10;
            let width = span * 2.0 / cells as f32;
            for cell in 0..cells {
                let x = -span + width * (cell as f32 + 0.5);
                let material = if cell % 2 == 0 {
                    &self.snow
                } else {
                    &self.dark
                };
                add_box(
                    &mut node,
                    &format!("Chequer{cell}"),
                    Vector3::new(width, 0.45, 0.22),
                    Vector3::new(x, height, 0.0),
                    material,
                );
            }
        } else {
            add_box(
                &mut node,
                "Bar",
                Vector3::new(span * 2.0, 0.28, 0.22),
                Vector3::new(0.0, height, 0.0),
                &self.ink,
            );
            if let Some(name) = name {
                let mut label = Label3D::new_alloc();
                label.set_name("Name");
                label.set_text(name);
                label.set_font_size(52);
                label.set_pixel_size(0.016);
                label.set_width(400.0);
                label.set_autowrap_mode(godot::classes::text_server::AutowrapMode::WORD_SMART);
                label.set_horizontal_alignment(godot::global::HorizontalAlignment::CENTER);
                label.set_vertical_alignment(godot::global::VerticalAlignment::CENTER);
                label.set_position(Vector3::new(0.0, height - 0.15, 0.25));
                label.set_modulate(crate::ui_style::SNOW);
                label.set_outline_modulate(crate::ui_style::INK);
                label.set_outline_size(18);
                node.add_child(&label);
            }
        }
        self.root.add_child(&node);
    }
}
