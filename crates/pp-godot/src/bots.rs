//! Computer riders on the road. Each one is a real rider mesh in that bot's kit.

use godot::classes::{Label3D, Node3D};
use godot::obj::Gd;
use godot::prelude::*;
use pp_session::BotState;

use crate::rider::Rider;
use crate::road::flat_right;

const TAG_M: f64 = 55.0;

struct Live {
    id: u32,
    node: Gd<Rider>,
    tag: Gd<Label3D>,
    preset: String,
    kit: String,
}

/// The bots currently on the course.
pub struct BotField {
    root: Gd<Node3D>,
    live: Vec<Live>,
}

impl BotField {
    /// Parent the field under `parent`.
    pub fn new(parent: &mut Gd<Node3D>) -> Self {
        let mut root = Node3D::new_alloc();
        root.set_name("Bots");
        parent.add_child(&root);
        Self {
            root,
            live: Vec::new(),
        }
    }

    /// Move the bots that are still in the session's list.
    pub fn sync(
        &mut self,
        bots: &[BotState],
        course: &pp_course::Course,
        lane: pp_view::lane::LaneSpec,
        rider_distance_m: f64,
        dt: f64,
    ) {
        let mut keep = Vec::new();
        for bot in bots {
            if let Some(pos) = self.live.iter().position(|live| live.id == bot.id) {
                let mut live = self.live.swap_remove(pos);
                if live.preset != bot.preset {
                    live.node.bind_mut().set_preset(&bot.preset);
                    live.preset.clone_from(&bot.preset);
                }
                if live.kit != bot.kit {
                    live.node.bind_mut().set_kit(&bot.kit);
                    live.kit.clone_from(&bot.kit);
                }
                place(&mut live, bot, course, lane, rider_distance_m, dt);
                keep.push(live);
            } else {
                keep.push(self.spawn(bot, course, lane, rider_distance_m, dt));
            }
        }
        for mut old in std::mem::take(&mut self.live) {
            old.node.queue_free();
        }
        self.live = keep;
    }

    fn spawn(
        &mut self,
        bot: &BotState,
        course: &pp_course::Course,
        lane: pp_view::lane::LaneSpec,
        rider_distance_m: f64,
        dt: f64,
    ) -> Live {
        let mut node = Rider::new_alloc();
        node.set_name(&format!("Bot{}", bot.id));
        {
            let mut rider = node.bind_mut();
            rider.set_preset(&bot.preset);
            rider.set_kit(&bot.kit);
        }
        let mut tag = Label3D::new_alloc();
        tag.set_name("Tag");
        tag.set_font_size(32);
        tag.set_pixel_size(0.02);
        tag.set_position(Vector3::new(0.0, 2.15, 0.0));
        tag.set_billboard_mode(godot::classes::base_material_3d::BillboardMode::ENABLED);
        tag.set_modulate(crate::ui_style::SNOW);
        tag.set_outline_modulate(crate::ui_style::INK);
        tag.set_outline_size(8);
        node.add_child(&tag);
        self.root.add_child(&node);
        let mut live = Live {
            id: bot.id,
            node,
            tag,
            preset: bot.preset.clone(),
            kit: bot.kit.clone(),
        };
        place(&mut live, bot, course, lane, rider_distance_m, dt);
        live
    }
}

fn place(
    live: &mut Live,
    bot: &BotState,
    course: &pp_course::Course,
    lane: pp_view::lane::LaneSpec,
    rider_distance_m: f64,
    dt: f64,
) {
    let slot = bot.slot.clamp(0.0, 4.0);
    let inner = lane.offset_for_slot(0);
    let outer = lane.offset_for_slot(4);
    let offset = inner + (outer - inner) * (slot / 4.0);
    let position = course.position_at(bot.total_distance_m);
    let tangent = course.tangent_at(bot.total_distance_m);
    let forward = Vector3::new(tangent[0], tangent[1], tangent[2]);
    let origin = Vector3::new(position[0], position[1], position[2]) + flat_right(forward) * offset;
    let grade = if bot.standing { 9.0 } else { 0.0 };
    let cadence = if bot.standing {
        bot.cadence_rpm.min(60.0)
    } else {
        bot.cadence_rpm
    };
    {
        let mut rider = live.node.bind_mut();
        rider.set_ride_state(bot.power_w, 200.0, grade, bot.speed_mps);
        rider.place(
            origin,
            forward,
            bot.total_distance_m,
            Some(cadence),
            dt,
            0.0,
        );
    }
    let gap = (bot.total_distance_m - rider_distance_m).abs();
    live.tag
        .set_text(&pp_view::extras::bot_tag(&bot.name, bot.pace_wkg));
    live.tag.set_visible(gap <= TAG_M);
}
