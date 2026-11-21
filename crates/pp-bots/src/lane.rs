//! Lane choice. Slot 0 is the left asphalt edge, slot 7 is the right edge.
//!
//! Bots stay on the paved slots. Slot 8 is the gravel right shoulder, and
//! it is left clear for the player. A pass moves left, toward lower slot
//! numbers, which from the right lane crosses toward the centre line. The
//! rider eases back toward a home slot once the wheel ahead is clear.
//! Nobody is left in the same slot within two bike lengths of another
//! rider or the player.

use crate::rider::Rider;
use crate::types::PlayerInput;

/// Paved lane fractions the bots can hold. Slot 0 is the left asphalt edge
/// and slot 7 is the right edge. The player's gravel shoulder is slot 8,
/// and bots do not use it.
pub const SLOT_COUNT: u8 = 8;

/// Length of a bike, in metres, used as the spacing unit.
pub const BIKE_LENGTH_M: f64 = 1.8;

/// Minimum gap, in metres, between two riders in the same slot.
///
/// Two bike lengths. Side by side in neighbouring slots is allowed.
pub const MIN_SAME_SLOT_M: f64 = BIKE_LENGTH_M * 2.0;

/// How far a slot eases, as a time constant, in seconds.
///
/// Three time constants is about a second, which is one slot change.
const LANE_TAU_S: f32 = 0.34;

/// Same-slot gap that already counts as sitting on the wheel.
const TOO_CLOSE_M: f64 = 6.0;

/// Gap used when asking whether a destination slot is free.
const CLEAR_M: f64 = 8.0;

/// Closing speed, in metres per second, that starts a pass.
const CLOSING_MPS: f32 = 0.3;

const SLOT_MIN: f32 = 0.0;
const SLOT_MAX: f32 = 7.0;

/// Pick a target slot for every rider, then ease toward it.
pub(crate) fn advance_lanes(riders: &mut [Rider], player: &PlayerInput, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    let mut order: Vec<usize> = (0..riders.len()).collect();
    order.sort_by(|&a, &b| {
        riders[b]
            .distance_m
            .total_cmp(&riders[a].distance_m)
            .then(riders[a].id.cmp(&riders[b].id))
    });
    for &index in &order {
        let target = choose_slot(index, riders, player);
        riders[index].target_slot = target;
    }
    let gain = 1.0 - (-dt / LANE_TAU_S).exp();
    for rider in riders.iter_mut() {
        let next = rider.slot + (rider.target_slot - rider.slot) * gain;
        rider.slot = next.clamp(SLOT_MIN, SLOT_MAX);
    }
}

fn choose_slot(index: usize, riders: &[Rider], player: &PlayerInput) -> f32 {
    let me = &riders[index];
    if should_pass(index, riders, player) {
        if me.target_slot < me.home_slot - 0.1
            && !crowded(index, riders, player, me.target_slot, CLEAR_M)
        {
            return me.target_slot;
        }
        if let Some(slot) = find_clear(index, riders, player, me.slot) {
            return slot;
        }
        return me.target_slot;
    }
    let home = me.home_slot;
    if (me.target_slot - home).abs() < 0.1 {
        return home;
    }
    let step = if me.target_slot < home { 1.0 } else { -1.0 };
    let next = (me.target_slot + step).clamp(SLOT_MIN, SLOT_MAX);
    if crowded(index, riders, player, next, CLEAR_M) {
        me.target_slot
    } else {
        next
    }
}

fn should_pass(index: usize, riders: &[Rider], player: &PlayerInput) -> bool {
    let me = &riders[index];
    for other in bodies(index, riders, player) {
        if !same_slot(me.slot, other.slot) && !same_slot(me.target_slot, other.slot) {
            continue;
        }
        let gap = other.distance_m - me.distance_m;
        if gap <= 0.0 {
            continue;
        }
        if gap < TOO_CLOSE_M && me.speed_mps + 0.15 >= other.speed_mps {
            return true;
        }
        let closing = me.speed_mps - other.speed_mps;
        if closing <= CLOSING_MPS {
            continue;
        }
        let look = (10.0 + f64::from(closing) * 1.4).clamp(12.0, 36.0);
        if gap < look {
            return true;
        }
    }
    false
}

fn find_clear(index: usize, riders: &[Rider], player: &PlayerInput, from_slot: f32) -> Option<f32> {
    let start = from_slot.floor().clamp(SLOT_MIN, SLOT_MAX);
    let mut cursor = start - 1.0;
    while cursor >= SLOT_MIN {
        if !crowded(index, riders, player, cursor, CLEAR_M) {
            return Some(cursor);
        }
        cursor -= 1.0;
    }
    cursor = start + 1.0;
    while cursor <= SLOT_MAX {
        if !crowded(index, riders, player, cursor, CLEAR_M) {
            return Some(cursor);
        }
        cursor += 1.0;
    }
    None
}

fn crowded(index: usize, riders: &[Rider], player: &PlayerInput, slot: f32, window_m: f64) -> bool {
    let me_dist = riders[index].distance_m;
    bodies(index, riders, player)
        .any(|other| same_slot(slot, other.slot) && (other.distance_m - me_dist).abs() < window_m)
}

struct Body {
    distance_m: f64,
    speed_mps: f32,
    slot: f32,
}

fn bodies<'a>(
    index: usize,
    riders: &'a [Rider],
    player: &'a PlayerInput,
) -> impl Iterator<Item = Body> + 'a {
    let me = riders[index].id;
    riders
        .iter()
        .filter(move |rider| rider.id != me)
        .map(|rider| Body {
            distance_m: rider.distance_m,
            speed_mps: rider.speed_mps,
            slot: rider.slot,
        })
        .chain(std::iter::once(Body {
            distance_m: player.total_distance_m,
            speed_mps: player.speed_mps,
            slot: player.slot,
        }))
}

fn same_slot(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

enum Fix {
    /// `rear` is behind a leader and inside two bike lengths in one slot.
    Hold {
        rear: usize,
        leader_dist: f64,
        leader_speed: f32,
        leader_slot: f32,
    },
    /// `index` is level with or ahead of the player in the player's slot.
    LeavePlayer { index: usize },
}

/// Last-resort separation. Leaders keep their place. A boxed-in follower is
/// held two bike lengths back, or nudged into a free neighbouring slot. A
/// bot that is on top of the player steps off the player's slot.
pub(crate) fn separate(riders: &mut [Rider], player: &PlayerInput) {
    for _ in 0..6 {
        let Some(fix) = worst_pair(riders, player) else {
            break;
        };
        match fix {
            Fix::Hold {
                rear,
                leader_dist,
                leader_speed,
                leader_slot,
            } => {
                if let Some(slot) = free_neighbour(riders, player, rear, leader_slot) {
                    riders[rear].slot = slot;
                    riders[rear].target_slot = slot;
                } else {
                    let held = leader_dist - (MIN_SAME_SLOT_M + 0.2);
                    if riders[rear].distance_m > held {
                        riders[rear].distance_m = held;
                        riders[rear].speed_mps = riders[rear].speed_mps.min(leader_speed);
                    }
                }
            }
            Fix::LeavePlayer { index } => {
                let slot = if let Some(slot) = free_neighbour(riders, player, index, player.slot) {
                    slot
                } else {
                    forced_off(player.slot)
                };
                riders[index].slot = slot;
                riders[index].target_slot = slot;
            }
        }
    }
}

fn worst_pair(riders: &[Rider], player: &PlayerInput) -> Option<Fix> {
    let mut best_gap = MIN_SAME_SLOT_M;
    let mut found: Option<Fix> = None;
    for i in 0..riders.len() {
        for j in 0..riders.len() {
            if i == j || !same_slot(riders[i].slot, riders[j].slot) {
                continue;
            }
            let gap = riders[j].distance_m - riders[i].distance_m;
            if gap == 0.0 && i < j {
                best_gap = 0.0;
                found = Some(Fix::Hold {
                    rear: i,
                    leader_dist: riders[j].distance_m,
                    leader_speed: riders[j].speed_mps,
                    leader_slot: riders[j].slot,
                });
                continue;
            }
            if gap <= 0.0 || gap >= best_gap {
                continue;
            }
            best_gap = gap;
            found = Some(Fix::Hold {
                rear: i,
                leader_dist: riders[j].distance_m,
                leader_speed: riders[j].speed_mps,
                leader_slot: riders[j].slot,
            });
        }
        let behind = player.total_distance_m - riders[i].distance_m;
        if same_slot(riders[i].slot, player.slot) && behind > 0.0 && behind < best_gap {
            best_gap = behind;
            found = Some(Fix::Hold {
                rear: i,
                leader_dist: player.total_distance_m,
                leader_speed: player.speed_mps,
                leader_slot: player.slot,
            });
        }
        let ahead = riders[i].distance_m - player.total_distance_m;
        if same_slot(riders[i].slot, player.slot) && ahead >= 0.0 && ahead < best_gap {
            best_gap = ahead;
            found = Some(Fix::LeavePlayer { index: i });
        }
    }
    found
}

fn forced_off(player_slot: f32) -> f32 {
    let inward = (player_slot - 0.8).clamp(SLOT_MIN, SLOT_MAX);
    if (inward - player_slot).abs() >= 0.5 {
        inward
    } else {
        (player_slot + 0.8).clamp(SLOT_MIN, SLOT_MAX)
    }
}

fn free_neighbour(riders: &[Rider], player: &PlayerInput, rear: usize, avoid: f32) -> Option<f32> {
    let me = &riders[rear];
    let inward = (avoid - 1.0).clamp(SLOT_MIN, SLOT_MAX);
    let outward = (avoid + 1.0).clamp(SLOT_MIN, SLOT_MAX);
    for candidate in [inward, outward] {
        if (candidate - avoid).abs() < 0.5 {
            continue;
        }
        let blocked = riders.iter().enumerate().any(|(n, other)| {
            n != rear
                && same_slot(candidate, other.slot)
                && (other.distance_m - me.distance_m).abs() < MIN_SAME_SLOT_M + 1.0
        }) || (same_slot(candidate, player.slot)
            && (player.total_distance_m - me.distance_m).abs() < MIN_SAME_SLOT_M + 1.0);
        if !blocked {
            return Some(candidate);
        }
    }
    None
}
