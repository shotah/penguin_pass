//! Public rider snapshot and the player sample the session passes in.

/// Where the player is at the end of this tick.
///
/// Distance uses the same odometer as the bots: metres from the start line,
/// still increasing after each lap. `slot` is the eased lane fraction, `0.0`
/// nearest the centre line and `4.0` at the edge. `mass_kg` is body mass,
/// not including the bicycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerInput {
    /// Odometer, in metres. Laps are included.
    pub total_distance_m: f64,
    /// Game speed, in metres per second. Negative is treated as stopped.
    pub speed_mps: f32,
    /// Trainer power, in watts. Negative is treated as coasting.
    pub power_w: f32,
    /// Eased lane fraction, `0.0` at the centre side and `4.0` at the edge.
    pub slot: f32,
    /// Rider body mass, in kilograms. The bicycle is not included.
    pub mass_kg: f32,
}

/// One bot, ready to draw.
///
/// `total_distance_m` is on the player's odometer. Wrap it with the lap
/// length for a point on the course. `slot` is the eased fraction, not a
/// destination: it moves for about a second when the rider changes lanes.
#[derive(Debug, Clone, PartialEq)]
pub struct BotState {
    /// Stable id for this appearance. A replacement rider gets a new id.
    pub id: u32,
    /// Given name from the built-in list.
    pub name: String,
    /// Rider preset id, `nils` or `sigrid`.
    pub preset: String,
    /// Kit id: `default`, `kit-sunrise`, `kit-glacier`, or `kit-kom-dots`.
    pub kit: String,
    /// Odometer, in metres, on the same scale as the player.
    pub total_distance_m: f64,
    /// Speed from the bike model, in metres per second.
    pub speed_mps: f32,
    /// Power the bot is putting into the pedals, in watts.
    pub power_w: f32,
    /// Cadence, in revolutions per minute.
    pub cadence_rpm: f32,
    /// Eased lane fraction, `0.0` at the centre side and `4.0` at the edge.
    pub slot: f32,
    /// Short out-of-the-saddle flag. True for a few seconds, then false.
    pub standing: bool,
    /// Watts per kilogram of body mass for a pace rider. Company riders are
    /// `None`.
    pub pace_wkg: Option<f32>,
}

/// Something the ride can show, or that the session can count.
#[derive(Debug, Clone, PartialEq)]
pub enum BotEvent {
    /// The player drew level with this bot and moved ahead.
    Passed {
        /// Bot id.
        id: u32,
        /// Bot name.
        name: String,
    },
    /// This bot drew level with the player and moved ahead.
    PassedBy {
        /// Bot id.
        id: u32,
        /// Bot name.
        name: String,
    },
    /// A company rider was introduced, already out of sight.
    Joined {
        /// Bot id.
        id: u32,
    },
    /// A company rider left, already out of sight.
    Left {
        /// Bot id.
        id: u32,
    },
}
