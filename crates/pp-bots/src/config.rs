//! How many riders, and whether the steady pace group is out.

/// Default number of riders on the road.
pub const DEFAULT_COUNT: u8 = 8;

/// Default smoothing time for the player's power, in seconds.
pub const DEFAULT_SMOOTH_S: f32 = 15.0;

/// A company rider farther than this may be replaced, in metres.
pub const DEFAULT_RETIRE_M: f64 = 400.0;

/// A new company rider appears at least this far away, in metres.
pub const DEFAULT_SPAWN_M: f64 = 320.0;

/// How many riders to put on the road, and the distances that keep them there.
///
/// `count == 0` turns the field off: no riders, and the draft factor is 1.
/// The default is eight riders, four of them pace riders when
/// [`Self::pace_riders`] is set. Counts are honoured up to 24. A request
/// above that is treated as 24 so a bad setting cannot fill the road.
///
/// Spawn and retire distances are pulled back into a band where a new rider
/// is at least 300 m away and at least 40 m inside the retire distance.
/// Somebody who appears does so out of sight, and is not removed on the
/// next tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BotConfig {
    /// Riders on the road. `0` turns the field off.
    pub count: u8,
    /// Steady watts-per-kilogram riders who lap the whole course.
    pub pace_riders: bool,
    /// Time constant of the player's smoothed power, in seconds.
    pub smooth_s: f32,
    /// Company riders farther than this, in metres, may be replaced.
    pub retire_m: f64,
    /// New company riders appear at least this far from the player, in metres.
    pub spawn_m: f64,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            count: DEFAULT_COUNT,
            pace_riders: true,
            smooth_s: DEFAULT_SMOOTH_S,
            retire_m: DEFAULT_RETIRE_M,
            spawn_m: DEFAULT_SPAWN_M,
        }
    }
}

/// Distances and the count after they have been pulled into the safe band.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub count: u8,
    pub pace_riders: bool,
    pub smooth_s: f32,
    pub retire_m: f64,
    pub spawn_m: f64,
}

impl Limits {
    pub fn from_config(config: BotConfig) -> Self {
        let count = config.count.min(24);
        let smooth_s = if config.smooth_s.is_finite() {
            config.smooth_s.clamp(0.5, 120.0)
        } else {
            DEFAULT_SMOOTH_S
        };
        let retire_m = if config.retire_m.is_finite() {
            config.retire_m.clamp(360.0, 2_000.0)
        } else {
            DEFAULT_RETIRE_M
        };
        let spawn_raw = if config.spawn_m.is_finite() {
            config.spawn_m
        } else {
            DEFAULT_SPAWN_M
        };
        let spawn_m = spawn_raw.clamp(300.0, retire_m - 40.0);
        Self {
            count,
            pace_riders: config.pace_riders,
            smooth_s,
            retire_m,
            spawn_m,
        }
    }
}
