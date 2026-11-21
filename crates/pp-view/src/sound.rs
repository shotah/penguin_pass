//! Ride audio levels. No Godot types: the player node applies the mix.
//!
//! Each layer has a target loudness and pitch from the ride, then a short
//! slew so nothing clicks when a threshold is crossed. The freewheel is the
//! coasting rule: rolling faster than a walk, with the legs quiet, and it
//! fades in and out over a fifth of a second.

/// How long a layer takes to fade between silent and full, in seconds.
pub const FADE_SECS: f32 = 0.2;
/// Speed, in m/s, above which coasting may begin. A walk is about 1.4.
pub const COAST_SPEED_ON: f32 = 2.0;
/// Speed, in m/s, below which coasting ends.
pub const COAST_SPEED_OFF: f32 = 1.3;
/// Cadence, in rpm, at or above which the legs are clearly turning.
pub const COAST_CADENCE_OFF: f32 = 18.0;
/// Cadence, in rpm, below which the cranks count as stopped.
pub const COAST_CADENCE_ON: f32 = 8.0;
/// Power, in watts, at or above which the rider is pushing.
pub const COAST_POWER_OFF: f32 = 20.0;
/// Power, in watts, below which the rider is not driving the chain.
pub const COAST_POWER_ON: f32 = 8.0;
/// Wind in the ears starts here, in m/s. 30 km/h.
pub const WIND_RUSH_MPS: f32 = 30.0 / 3.6;

/// Which landscape the ambience follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ambience {
    /// Birds, a light breeze, a distant bell, a stream.
    Alpine,
    /// Dry wind, insects, and a hawk when a recording is loaded.
    Desert,
}

/// What the player can start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    /// Alpine birdsong loop.
    AlpineBirds,
    /// Alpine breeze loop.
    AlpineBreeze,
    /// Desert wind loop.
    DesertWind,
    /// Desert insect loop.
    DesertInsects,
    /// Freewheel clicks while coasting.
    Ratchet,
    /// Air in the ears at speed.
    WindRush,
    /// The rider breathing.
    Breath,
    /// One distant cowbell.
    Cowbell,
    /// A short stretch of stream.
    Stream,
    /// A hawk, when the file was found. Playback skips a missing file.
    Hawk,
    /// Fish collected. Pitch rises with the combo.
    Fish,
    /// Golden fish.
    FishGold,
    /// The combo ended.
    ComboBreak,
    /// A powerup was awarded.
    PowerupAward,
    /// A powerup started.
    PowerupOn,
    /// A powerup ran out.
    PowerupOff,
    /// A segment was entered.
    SegmentEnter,
    /// A segment was finished, and it was not a record.
    SegmentFinish,
    /// A segment record.
    Record,
    /// The ghost was beaten.
    Ghost,
    /// A badge was earned.
    Badge,
    /// Lap completed.
    LapBell,
    /// The ride started.
    RideStart,
    /// The ride was paused.
    Pause,
    /// The ride was saved.
    RideSaved,
    /// The menu highlight moved.
    UiMove,
    /// The menu confirmed a row.
    UiSelect,
    /// The menu stepped back.
    UiBack,
}

impl Clip {
    /// Every clip the game tries to load. Order is the index.
    pub const ALL: [Clip; 28] = [
        Clip::AlpineBirds,
        Clip::AlpineBreeze,
        Clip::DesertWind,
        Clip::DesertInsects,
        Clip::Ratchet,
        Clip::WindRush,
        Clip::Breath,
        Clip::Cowbell,
        Clip::Stream,
        Clip::Hawk,
        Clip::Fish,
        Clip::FishGold,
        Clip::ComboBreak,
        Clip::PowerupAward,
        Clip::PowerupOn,
        Clip::PowerupOff,
        Clip::SegmentEnter,
        Clip::SegmentFinish,
        Clip::Record,
        Clip::Ghost,
        Clip::Badge,
        Clip::LapBell,
        Clip::RideStart,
        Clip::Pause,
        Clip::RideSaved,
        Clip::UiMove,
        Clip::UiSelect,
        Clip::UiBack,
    ];

    /// File name under `res://assets/sfx/`.
    #[must_use]
    pub fn file(self) -> &'static str {
        match self {
            Clip::AlpineBirds => "alpine-birds.ogg",
            Clip::AlpineBreeze => "alpine-breeze.ogg",
            Clip::DesertWind => "desert-wind.ogg",
            Clip::DesertInsects => "desert-insects.ogg",
            Clip::Ratchet => "ratchet.ogg",
            Clip::WindRush => "wind-rush.ogg",
            Clip::Breath => "breath.ogg",
            Clip::Cowbell => "cowbell.ogg",
            Clip::Stream => "stream.ogg",
            Clip::Hawk => "hawk.ogg",
            Clip::Fish => "fish.ogg",
            Clip::FishGold => "fish-gold.ogg",
            Clip::ComboBreak => "combo-break.ogg",
            Clip::PowerupAward => "powerup-award.ogg",
            Clip::PowerupOn => "powerup-on.ogg",
            Clip::PowerupOff => "powerup-off.ogg",
            Clip::SegmentEnter => "segment-enter.ogg",
            Clip::SegmentFinish => "segment-finish.ogg",
            Clip::Record => "record.ogg",
            Clip::Ghost => "ghost.ogg",
            Clip::Badge => "badge.ogg",
            Clip::LapBell => "lap-bell.ogg",
            Clip::RideStart => "ride-start.ogg",
            Clip::Pause => "pause.ogg",
            Clip::RideSaved => "ride-saved.ogg",
            Clip::UiMove => "ui-move.ogg",
            Clip::UiSelect => "ui-select.ogg",
            Clip::UiBack => "ui-back.ogg",
        }
    }

    /// Loops stay up for the whole ride. One-shots play once.
    #[must_use]
    pub fn loops(self) -> bool {
        matches!(
            self,
            Clip::AlpineBirds
                | Clip::AlpineBreeze
                | Clip::DesertWind
                | Clip::DesertInsects
                | Clip::Ratchet
                | Clip::WindRush
                | Clip::Breath
        )
    }

    /// Ambience bus, or the effects bus.
    #[must_use]
    pub fn ambient(self) -> bool {
        matches!(
            self,
            Clip::AlpineBirds
                | Clip::AlpineBreeze
                | Clip::DesertWind
                | Clip::DesertInsects
                | Clip::Cowbell
                | Clip::Stream
                | Clip::Hawk
        )
    }

    /// A missing hawk or stream is reported and skipped. No CC0 recording of
    /// either was fetched. Every other clip is required.
    #[must_use]
    pub fn required(self) -> bool {
        !matches!(self, Clip::Hawk | Clip::Stream)
    }
}

/// A continuous layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Alpine birds.
    Birds,
    /// Alpine breeze. Louder with height and grade.
    Breeze,
    /// Desert wind.
    DesertWind,
    /// Crickets or cicadas.
    Insects,
    /// Tyre and road. The recording may be absent; the level is still computed.
    Tyre,
    /// Drivetrain while pedalling. Same caveat as [`Layer::Tyre`].
    Drivetrain,
    /// Freewheel.
    Ratchet,
    /// Wind rush above 30 km/h.
    WindRush,
    /// Breathing.
    Breath,
}

impl Layer {
    /// Draw order, and the index into a mix.
    pub const ALL: [Layer; 9] = [
        Layer::Birds,
        Layer::Breeze,
        Layer::DesertWind,
        Layer::Insects,
        Layer::Tyre,
        Layer::Drivetrain,
        Layer::Ratchet,
        Layer::WindRush,
        Layer::Breath,
    ];

    /// Short name for the log line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Layer::Birds => "birds",
            Layer::Breeze => "breeze",
            Layer::DesertWind => "desert-wind",
            Layer::Insects => "insects",
            Layer::Tyre => "tyre",
            Layer::Drivetrain => "drivetrain",
            Layer::Ratchet => "ratchet",
            Layer::WindRush => "wind-rush",
            Layer::Breath => "breath",
        }
    }

    /// Clip for this layer, when a recording ships with the game.
    #[must_use]
    pub fn clip(self) -> Option<Clip> {
        match self {
            Layer::Birds => Some(Clip::AlpineBirds),
            Layer::Breeze => Some(Clip::AlpineBreeze),
            Layer::DesertWind => Some(Clip::DesertWind),
            Layer::Insects => Some(Clip::DesertInsects),
            Layer::Tyre => None,
            Layer::Drivetrain => None,
            Layer::Ratchet => Some(Clip::Ratchet),
            Layer::WindRush => Some(Clip::WindRush),
            Layer::Breath => Some(Clip::Breath),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Something that just happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundEvent {
    /// A fish was collected.
    Fish {
        /// Golden fish use a different clip.
        golden: bool,
        /// Combo length, including this fish.
        combo: u32,
    },
    /// The combo reset.
    ComboBroken,
    /// A powerup landed in the slot.
    PowerupAwarded,
    /// The rider started the held powerup.
    PowerupActivated,
    /// A powerup's time ran out.
    PowerupExpired,
    /// A segment start line.
    SegmentEntered,
    /// A segment finish. `record` picks the record clip.
    SegmentFinished {
        /// This finish beat the stored best.
        record: bool,
    },
    /// The first lap beat the ghost.
    GhostBeaten,
    /// A badge was earned.
    BadgeEarned,
    /// A lap was completed.
    LapCompleted,
    /// The ride file was written.
    RideSaved,
    /// The highlight moved.
    MenuMove,
    /// A row was confirmed.
    MenuSelect,
    /// The menu stepped back.
    MenuBack,
}

/// One frame of the ride, already gathered by the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RideInput {
    /// Game speed, in meters per second.
    pub speed_mps: f32,
    /// Cadence. `None` when the sensor is not reporting.
    pub cadence_rpm: Option<f32>,
    /// Power, in watts.
    pub power_w: f32,
    /// Coggan zone, 1 to 7, as the HUD already shows it.
    pub zone: u8,
    /// Heart rate, when a strap is connected.
    pub heart_bpm: Option<u8>,
    /// Grade, in percent. Positive is uphill.
    pub grade_pct: f32,
    /// Elevation above the course datum, in meters.
    pub elevation_m: f32,
    /// Theme name, reduced to the two ambiences.
    pub theme: Ambience,
    /// On the road, including the pause menu over a ride.
    pub riding: bool,
    /// Pause, the end-ride confirm, or the settings page.
    pub paused: bool,
    /// The rider-breathing setting.
    pub breathing: bool,
}

/// Target the player should be at, after smoothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerOut {
    /// Which layer.
    pub layer: Layer,
    /// Linear loudness, 0 to 1, before the bus slider.
    pub level: f32,
    /// Playback rate.
    pub pitch: f32,
}

/// One sound to start this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    /// Which file.
    pub clip: Clip,
    /// Linear loudness, 0 to 1.
    pub gain: f32,
    /// Playback rate. Fish rises with the combo.
    pub pitch: f32,
}

/// The frame's mix. Cues are the ones that should start now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    layers: [LayerOut; 9],
    cues: [Option<Cue>; 4],
    /// How many of [`Self::cues`] are filled.
    cue_count: usize,
    /// The coasting gate, after hysteresis. The ratchet level lags this.
    coasting: bool,
}

impl Mix {
    /// One layer.
    #[must_use]
    pub fn layer(&self, layer: Layer) -> LayerOut {
        self.layers[layer.index()]
    }

    /// One-shots and stingers for this frame.
    #[must_use]
    pub fn cues(&self) -> &[Option<Cue>] {
        &self.cues[..self.cue_count]
    }

    /// The freewheel gate. The audible level fades toward this.
    #[must_use]
    pub fn coasting(&self) -> bool {
        self.coasting
    }
}

/// Maps the ride to a mix. The same seed schedules the same one-shots.
#[derive(Clone, Debug)]
pub struct Director {
    levels: [f32; 9],
    pitches: [f32; 9],
    coasting: bool,
    was_riding: bool,
    was_paused: bool,
    rng: u64,
    /// Cowbell, stream, hawk. Seconds until the next try.
    wait: [f32; 3],
    pending: [Option<Cue>; 8],
    pending_len: usize,
}

impl Director {
    /// Start silent. `seed` picks the ambience one-shots.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut rng = seed | 1;
        let wait = [
            interval(&mut rng, 12.0, 28.0),
            interval(&mut rng, 18.0, 36.0),
            interval(&mut rng, 16.0, 32.0),
        ];
        Self {
            levels: [0.0; 9],
            pitches: [1.0; 9],
            coasting: false,
            was_riding: false,
            was_paused: false,
            rng,
            wait,
            pending: [None; 8],
            pending_len: 0,
        }
    }

    /// Queue a stinger. A full queue drops the oldest.
    pub fn push(&mut self, event: SoundEvent) {
        self.enqueue(cue_for(event));
    }

    /// Advance one frame.
    pub fn step(&mut self, input: &RideInput, dt: f32) -> Mix {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        self.update_coast(input);
        self.edges(input);
        self.schedule(input, dt);
        let targets = targets(input, self.coasting);
        for (index, (level, pitch)) in targets.iter().enumerate() {
            self.levels[index] = slew(self.levels[index], *level, dt, FADE_SECS);
            self.pitches[index] = slew(self.pitches[index], *pitch, dt, FADE_SECS);
        }
        let mut layers = [LayerOut {
            layer: Layer::Birds,
            level: 0.0,
            pitch: 1.0,
        }; 9];
        for (index, layer) in Layer::ALL.iter().enumerate() {
            layers[index] = LayerOut {
                layer: *layer,
                level: self.levels[index],
                pitch: self.pitches[index],
            };
        }
        let mut cues = [None; 4];
        let mut cue_count = 0;
        while cue_count < cues.len() {
            let Some(cue) = self.pop_cue() else {
                break;
            };
            cues[cue_count] = Some(cue);
            cue_count += 1;
        }
        Mix {
            layers,
            cues,
            cue_count,
            coasting: self.coasting,
        }
    }

    /// The gate, for tests and the log.
    #[must_use]
    pub fn coasting(&self) -> bool {
        self.coasting
    }

    fn update_coast(&mut self, input: &RideInput) {
        let speed = finite(input.speed_mps).max(0.0);
        let power = finite(input.power_w).max(0.0);
        let cadence = input.cadence_rpm.filter(|rpm| rpm.is_finite());
        let rolling = if self.coasting {
            speed >= COAST_SPEED_OFF
        } else {
            speed > COAST_SPEED_ON
        };
        // Unknown cadence is not "near zero": only the watts decide then.
        let quiet = match cadence {
            Some(rpm) if self.coasting => rpm < COAST_CADENCE_OFF || power < COAST_POWER_OFF,
            Some(rpm) => rpm < COAST_CADENCE_ON || power < COAST_POWER_ON,
            None if self.coasting => power < COAST_POWER_OFF,
            None => power < COAST_POWER_ON,
        };
        // Leaving needs both the cranks and the power back, and the speed down
        // uses its own threshold. Entering needs either quiet legs or a soft
        // pedal, while already rolling.
        self.coasting = rolling && quiet;
    }

    fn edges(&mut self, input: &RideInput) {
        if input.riding && !self.was_riding {
            self.enqueue(Cue {
                clip: Clip::RideStart,
                gain: 0.7,
                pitch: 1.0,
            });
        }
        if input.paused && !self.was_paused && input.riding {
            self.enqueue(Cue {
                clip: Clip::Pause,
                gain: 0.55,
                pitch: 1.0,
            });
        }
        self.was_riding = input.riding;
        self.was_paused = input.paused;
    }

    fn schedule(&mut self, input: &RideInput, dt: f32) {
        let slots: [(Clip, Ambience); 3] = [
            (Clip::Cowbell, Ambience::Alpine),
            (Clip::Stream, Ambience::Alpine),
            (Clip::Hawk, Ambience::Desert),
        ];
        for (index, (clip, theme)) in slots.iter().enumerate() {
            self.wait[index] -= dt;
            if self.wait[index] > 0.0 {
                continue;
            }
            let (min, span) = match clip {
                Clip::Cowbell => (22.0, 36.0),
                Clip::Stream => (30.0, 40.0),
                _ => (24.0, 34.0),
            };
            self.wait[index] = interval(&mut self.rng, min, span);
            if input.theme == *theme {
                self.enqueue(Cue {
                    clip: *clip,
                    gain: 0.45,
                    pitch: 1.0,
                });
            }
        }
    }

    fn enqueue(&mut self, cue: Cue) {
        if self.pending_len == self.pending.len() {
            self.pending.copy_within(1.., 0);
            self.pending_len -= 1;
        }
        self.pending[self.pending_len] = Some(cue);
        self.pending_len += 1;
    }

    fn pop_cue(&mut self) -> Option<Cue> {
        if self.pending_len == 0 {
            return None;
        }
        let cue = self.pending[0].take();
        self.pending.copy_within(1.., 0);
        self.pending_len -= 1;
        cue
    }
}

/// Clip, gain, and pitch for one event. Fish pitch rises with the combo.
#[must_use]
pub fn cue_for(event: SoundEvent) -> Cue {
    match event {
        SoundEvent::Fish { golden, combo } => {
            let steps = combo.min(8) as f32;
            if golden {
                Cue {
                    clip: Clip::FishGold,
                    gain: 0.7,
                    pitch: 1.0 + steps * 0.04,
                }
            } else {
                Cue {
                    clip: Clip::Fish,
                    gain: 0.62,
                    pitch: 1.0 + steps * 0.06,
                }
            }
        }
        SoundEvent::ComboBroken => Cue {
            clip: Clip::ComboBreak,
            gain: 0.5,
            pitch: 1.0,
        },
        SoundEvent::PowerupAwarded => Cue {
            clip: Clip::PowerupAward,
            gain: 0.6,
            pitch: 1.0,
        },
        SoundEvent::PowerupActivated => Cue {
            clip: Clip::PowerupOn,
            gain: 0.62,
            pitch: 1.0,
        },
        SoundEvent::PowerupExpired => Cue {
            clip: Clip::PowerupOff,
            gain: 0.45,
            pitch: 1.0,
        },
        SoundEvent::SegmentEntered => Cue {
            clip: Clip::SegmentEnter,
            gain: 0.55,
            pitch: 1.0,
        },
        SoundEvent::SegmentFinished { record: true } => Cue {
            clip: Clip::Record,
            gain: 0.7,
            pitch: 1.0,
        },
        SoundEvent::SegmentFinished { record: false } => Cue {
            clip: Clip::SegmentFinish,
            gain: 0.6,
            pitch: 1.0,
        },
        SoundEvent::GhostBeaten => Cue {
            clip: Clip::Ghost,
            gain: 0.65,
            pitch: 1.0,
        },
        SoundEvent::BadgeEarned => Cue {
            clip: Clip::Badge,
            gain: 0.65,
            pitch: 1.0,
        },
        SoundEvent::LapCompleted => Cue {
            clip: Clip::LapBell,
            gain: 0.7,
            pitch: 1.0,
        },
        SoundEvent::RideSaved => Cue {
            clip: Clip::RideSaved,
            gain: 0.55,
            pitch: 1.0,
        },
        SoundEvent::MenuMove => Cue {
            clip: Clip::UiMove,
            gain: 0.35,
            pitch: 1.0,
        },
        SoundEvent::MenuSelect => Cue {
            clip: Clip::UiSelect,
            gain: 0.5,
            pitch: 1.0,
        },
        SoundEvent::MenuBack => Cue {
            clip: Clip::UiBack,
            gain: 0.4,
            pitch: 0.96,
        },
    }
}

/// How open the slope and the height are, from 0 to 1. Cheap on purpose.
#[must_use]
pub fn exposure(elevation_m: f32, grade_pct: f32) -> f32 {
    let height = (finite(elevation_m) / 180.0).clamp(0.0, 1.0);
    let slope = (finite(grade_pct).abs() / 10.0).clamp(0.0, 1.0);
    height * 0.65 + slope * 0.35
}

/// Scripted ride used by the capture: pedal, sprint, coast, pause.
#[derive(Clone, Debug)]
pub struct Demo {
    /// Seconds since the script started.
    pub t: f32,
    fish_sent: bool,
}

impl Demo {
    /// Start at the pedal.
    #[must_use]
    pub fn new() -> Self {
        Self {
            t: 0.0,
            fish_sent: false,
        }
    }

    /// The scripted state, and a fish once the coast has begun.
    pub fn advance(&mut self, dt: f32) -> (RideInput, Option<SoundEvent>) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let previous = self.t;
        self.t += dt;
        let event = if !self.fish_sent && previous < 5.2 && self.t >= 5.2 {
            self.fish_sent = true;
            Some(SoundEvent::Fish {
                golden: false,
                combo: 3,
            })
        } else {
            None
        };
        (demo_input(self.t), event)
    }
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

/// The ride state at `t` seconds into [`Demo`].
#[must_use]
pub fn demo_input(t: f32) -> RideInput {
    if t < 2.5 {
        ride(6.0, Some(80.0), 160.0, 3, Some(130), 3.0, 40.0, false)
    } else if t < 4.5 {
        ride(11.0, Some(105.0), 420.0, 6, Some(168), 6.0, 90.0, false)
    } else if t < 7.5 {
        ride(14.0, Some(0.0), 0.0, 1, Some(150), -8.0, 140.0, false)
    } else {
        ride(0.0, Some(0.0), 0.0, 1, None, 0.0, 140.0, true)
    }
}

/// Length of [`Demo`], in seconds.
pub const DEMO_SECS: f32 = 9.0;

#[allow(clippy::too_many_arguments)]
fn ride(
    speed_mps: f32,
    cadence_rpm: Option<f32>,
    power_w: f32,
    zone: u8,
    heart_bpm: Option<u8>,
    grade_pct: f32,
    elevation_m: f32,
    paused: bool,
) -> RideInput {
    RideInput {
        speed_mps,
        cadence_rpm,
        power_w,
        zone,
        heart_bpm,
        grade_pct,
        elevation_m,
        theme: Ambience::Alpine,
        riding: true,
        paused,
        breathing: true,
    }
}

fn targets(input: &RideInput, coasting: bool) -> [(f32, f32); 9] {
    let speed = finite(input.speed_mps).max(0.0);
    let power = finite(input.power_w).max(0.0);
    let cadence = input
        .cadence_rpm
        .filter(|rpm| rpm.is_finite())
        .unwrap_or(0.0)
        .max(0.0);
    let open = exposure(input.elevation_m, input.grade_pct);
    let place = if !input.riding {
        0.42
    } else if input.paused {
        0.38
    } else {
        1.0
    };
    let on_bike = input.riding && !input.paused;
    let mut out = [(0.0, 1.0); 9];
    if input.theme == Ambience::Alpine {
        out[Layer::Birds.index()] = ((0.58 * (1.0 - 0.22 * open) * place), 1.0);
        out[Layer::Breeze.index()] = ((0.2 + 0.5 * open) * place, 0.96 + 0.08 * open);
    } else {
        out[Layer::DesertWind.index()] = ((0.28 + 0.55 * open) * place, 0.94 + 0.1 * open);
        out[Layer::Insects.index()] = (0.46 * place, 1.0);
    }
    if on_bike {
        let tyre = ((speed - 1.2) / 10.0).clamp(0.0, 1.0);
        out[Layer::Tyre.index()] = (tyre * 0.7, (0.8 + speed * 0.035).clamp(0.75, 1.7));
        if !coasting && cadence > 20.0 && power > 15.0 {
            let cad = (cadence / 95.0).clamp(0.0, 1.3);
            let effort = (power / 280.0).clamp(0.0, 1.0);
            out[Layer::Drivetrain.index()] = (
                cad * 0.5 * (0.35 + 0.65 * effort),
                (0.75 + cadence / 150.0).clamp(0.7, 1.45),
            );
        }
        if coasting {
            out[Layer::Ratchet.index()] = (0.62, (0.55 + speed * 0.07).clamp(0.5, 2.0));
        }
        let rush = wind_rush(speed, input.grade_pct);
        out[Layer::WindRush.index()] = (
            rush,
            (0.9 + (speed - WIND_RUSH_MPS).max(0.0) * 0.04).clamp(0.85, 1.6),
        );
        if input.breathing {
            let effort = breath_effort(input.zone, input.heart_bpm);
            out[Layer::Breath.index()] = (effort * 0.42, 0.92 + effort * 0.2);
        }
    }
    out
}

fn wind_rush(speed_mps: f32, grade_pct: f32) -> f32 {
    if speed_mps <= WIND_RUSH_MPS {
        return 0.0;
    }
    let amount = ((speed_mps - WIND_RUSH_MPS) / 7.0).clamp(0.0, 1.0);
    let descent = if grade_pct < -1.0 { 1.3 } else { 1.0 };
    (amount * descent).min(1.0)
}

fn breath_effort(zone: u8, heart_bpm: Option<u8>) -> f32 {
    let from_zone = match zone {
        0 | 1 => 0.12,
        2 => 0.22,
        3 => 0.36,
        4 => 0.5,
        5 => 0.66,
        6 => 0.82,
        _ => 1.0,
    };
    match heart_bpm {
        Some(bpm) => {
            let from_heart = ((f32::from(bpm) - 85.0) / 85.0).clamp(0.0, 1.0);
            from_zone * 0.5 + from_heart * 0.5
        }
        None => from_zone,
    }
}

fn slew(current: f32, target: f32, dt: f32, seconds: f32) -> f32 {
    let step = dt / seconds.max(0.001);
    let delta = (target - current).clamp(-step, step);
    current + delta
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn interval(rng: &mut u64, min: f32, span: f32) -> f32 {
    let unit = (xorshift(rng) % 10_000) as f32 / 10_000.0;
    min + unit * span
}

fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpine() -> RideInput {
        ride(0.0, Some(0.0), 0.0, 1, None, 0.0, 20.0, false)
    }

    fn settled(input: &RideInput, coasting: bool) -> Mix {
        let mut director = Director::new(1);
        director.coasting = coasting;
        let mut mix = director.step(input, 0.0);
        // One long step would clamp dt to 0.1. Walk the fade in.
        for _ in 0..30 {
            mix = director.step(input, 0.1);
        }
        mix
    }

    #[test]
    fn coasting_has_hysteresis_and_the_ratchet_fades() {
        let mut director = Director::new(3);
        let mut rolling = alpine();
        rolling.speed_mps = 1.6;
        rolling.cadence_rpm = Some(0.0);
        rolling.power_w = 0.0;
        director.step(&rolling, 0.05);
        assert!(!director.coasting());
        rolling.speed_mps = 2.4;
        director.step(&rolling, 0.05);
        assert!(director.coasting());
        // Still rolling, cadence in the gap, power in the gap: stay on.
        rolling.speed_mps = 1.5;
        rolling.cadence_rpm = Some(12.0);
        rolling.power_w = 12.0;
        director.step(&rolling, 0.05);
        assert!(director.coasting());
        rolling.cadence_rpm = Some(30.0);
        rolling.power_w = 40.0;
        director.step(&rolling, 0.05);
        assert!(!director.coasting());
        // Power alone is enough to enter, once the speed is back up.
        rolling.speed_mps = 3.0;
        rolling.cadence_rpm = Some(70.0);
        rolling.power_w = 4.0;
        director.step(&rolling, 0.05);
        assert!(director.coasting());

        let mut director = Director::new(3);
        let mut coast = alpine();
        coast.speed_mps = 8.0;
        coast.cadence_rpm = Some(0.0);
        coast.power_w = 0.0;
        let mid = director.step(&coast, 0.1);
        let level = mid.layer(Layer::Ratchet).level;
        assert!(level > 0.35 && level < 0.62, "{level}");
        let full = director.step(&coast, 0.1);
        assert!((full.layer(Layer::Ratchet).level - 0.62).abs() < 0.02);
        coast.speed_mps = 0.2;
        let falling = director.step(&coast, 0.1);
        assert!(falling.layer(Layer::Ratchet).level > 0.05);
        assert!(falling.layer(Layer::Ratchet).level < 0.3);
    }

    #[test]
    fn pitch_follows_speed_and_cadence_and_wind_waits_for_thirty() {
        let pedal = ride(6.0, Some(85.0), 180.0, 3, None, 2.0, 30.0, false);
        let mix = settled(&pedal, false);
        assert!(mix.layer(Layer::Tyre).level > 0.2);
        assert_eq!(mix.layer(Layer::WindRush).level, 0.0);
        assert!(mix.layer(Layer::Drivetrain).level > 0.2);
        assert!(mix.layer(Layer::Ratchet).level < 0.05);
        let fast = mix.layer(Layer::Drivetrain).pitch;
        let easy = settled(
            &ride(6.0, Some(50.0), 80.0, 2, None, 2.0, 30.0, false),
            false,
        );
        assert!(fast > easy.layer(Layer::Drivetrain).pitch);

        let descent = ride(14.0, Some(0.0), 0.0, 1, None, -8.0, 100.0, false);
        let mix = settled(&descent, true);
        assert!(mix.layer(Layer::WindRush).level > 0.7);
        assert!(mix.layer(Layer::Ratchet).pitch > 1.2);
        let flat = settled(
            &ride(14.0, Some(90.0), 200.0, 4, None, 1.0, 100.0, false),
            false,
        );
        assert!(mix.layer(Layer::WindRush).level > flat.layer(Layer::WindRush).level);
    }

    #[test]
    fn breeze_rises_with_height_and_the_desert_uses_its_own_bed() {
        let low = settled(&ride(0.0, Some(0.0), 0.0, 1, None, 0.0, 0.0, false), false);
        let high = settled(
            &ride(0.0, Some(0.0), 0.0, 1, None, 8.0, 200.0, false),
            false,
        );
        assert!(high.layer(Layer::Breeze).level > low.layer(Layer::Breeze).level + 0.2);
        assert!(low.layer(Layer::Birds).level > 0.3);
        assert_eq!(low.layer(Layer::Insects).level, 0.0);

        let mut desert = alpine();
        desert.theme = Ambience::Desert;
        desert.elevation_m = 40.0;
        let mix = settled(&desert, false);
        assert!(mix.layer(Layer::DesertWind).level > 0.2);
        assert!(mix.layer(Layer::Insects).level > 0.3);
        assert!(mix.layer(Layer::Birds).level < 0.05);
    }

    #[test]
    fn breathing_follows_effort_and_can_be_switched_off() {
        let easy = settled(
            &ride(5.0, Some(70.0), 100.0, 1, None, 1.0, 10.0, false),
            false,
        );
        let hard = settled(
            &ride(5.0, Some(95.0), 400.0, 7, Some(175), 4.0, 10.0, false),
            false,
        );
        assert!(hard.layer(Layer::Breath).level > easy.layer(Layer::Breath).level + 0.15);
        let mut quiet = ride(5.0, Some(95.0), 400.0, 7, Some(175), 4.0, 10.0, false);
        quiet.breathing = false;
        let mix = settled(&quiet, false);
        assert!(mix.layer(Layer::Breath).level < 0.02);
        let paused = settled(
            &ride(5.0, Some(95.0), 400.0, 7, Some(175), 4.0, 10.0, true),
            false,
        );
        assert!(paused.layer(Layer::Breath).level < 0.02);
        assert!(paused.layer(Layer::Tyre).level < 0.05);
        assert!(paused.layer(Layer::Birds).level > 0.1);
    }

    #[test]
    fn title_keeps_a_quiet_bed_and_drops_the_bike() {
        let mut title = alpine();
        title.riding = false;
        title.speed_mps = 8.0;
        title.power_w = 200.0;
        title.cadence_rpm = Some(90.0);
        let mix = settled(&title, false);
        assert!(mix.layer(Layer::Birds).level > 0.1);
        assert!(mix.layer(Layer::Birds).level < 0.4);
        assert!(mix.layer(Layer::Tyre).level < 0.05);
    }

    #[test]
    fn events_pick_a_clip_and_the_fish_rises() {
        let one = cue_for(SoundEvent::Fish {
            golden: false,
            combo: 1,
        });
        let four = cue_for(SoundEvent::Fish {
            golden: false,
            combo: 4,
        });
        assert_eq!(one.clip, Clip::Fish);
        assert!(four.pitch > one.pitch);
        assert_eq!(
            cue_for(SoundEvent::Fish {
                golden: true,
                combo: 2,
            })
            .clip,
            Clip::FishGold
        );
        assert_eq!(
            cue_for(SoundEvent::SegmentFinished { record: true }).clip,
            Clip::Record
        );
        assert_eq!(
            cue_for(SoundEvent::SegmentFinished { record: false }).clip,
            Clip::SegmentFinish
        );
        assert_eq!(cue_for(SoundEvent::LapCompleted).clip, Clip::LapBell);
        assert_eq!(cue_for(SoundEvent::MenuBack).clip, Clip::UiBack);
        assert!(Clip::ALL.iter().any(|clip| clip.file() == "fish.ogg"));
        assert!(!Clip::Hawk.required());
        assert!(Clip::Fish.required());
    }

    #[test]
    fn one_shots_follow_the_theme_and_the_seed() {
        let run = |seed, theme| {
            let mut director = Director::new(seed);
            let mut input = alpine();
            input.theme = theme;
            let mut heard = Vec::new();
            // `step` clamps a frame to a tenth of a second, so this is
            // eighty seconds of ride time.
            let mut t = 0.0;
            while t < 80.0 {
                for cue in director.step(&input, 0.1).cues().iter().flatten() {
                    if matches!(cue.clip, Clip::Cowbell | Clip::Stream | Clip::Hawk) {
                        heard.push(cue.clip);
                    }
                }
                t += 0.1;
            }
            heard
        };
        assert_eq!(run(9, Ambience::Alpine), run(9, Ambience::Alpine));
        let alpine_heard = run(9, Ambience::Alpine);
        assert!(alpine_heard.contains(&Clip::Cowbell));
        assert!(alpine_heard.contains(&Clip::Stream));
        assert!(!alpine_heard.contains(&Clip::Hawk));
        let desert = run(9, Ambience::Desert);
        assert!(desert.contains(&Clip::Hawk));
        assert!(!desert.contains(&Clip::Cowbell));
        assert!(!desert.contains(&Clip::Stream));
    }

    #[test]
    fn the_demo_coasts_sprints_and_collects_one_fish() {
        let mut demo = Demo::new();
        let mut director = Director::new(2);
        let mut saw_fish = false;
        let mut coast_ratchet: f32 = 0.0;
        let mut pedal_ratchet: f32 = 1.0;
        let mut sprint_wind: f32 = 0.0;
        while demo.t < DEMO_SECS {
            let (input, event) = demo.advance(0.05);
            if let Some(event) = event {
                assert!(matches!(
                    event,
                    SoundEvent::Fish {
                        golden: false,
                        combo: 3
                    }
                ));
                director.push(event);
                saw_fish = true;
            }
            let mix = director.step(&input, 0.05);
            if (2.8..3.2).contains(&demo.t) {
                pedal_ratchet = pedal_ratchet.min(mix.layer(Layer::Ratchet).level);
                sprint_wind = sprint_wind.max(mix.layer(Layer::WindRush).level);
            }
            if (6.0..6.4).contains(&demo.t) {
                coast_ratchet = coast_ratchet.max(mix.layer(Layer::Ratchet).level);
            }
            if mix
                .cues()
                .iter()
                .flatten()
                .any(|cue| cue.clip == Clip::Fish)
            {
                saw_fish = true;
            }
        }
        assert!(saw_fish);
        assert!(pedal_ratchet < 0.08, "{pedal_ratchet}");
        assert!(coast_ratchet > 0.5, "{coast_ratchet}");
        assert!(sprint_wind > 0.2, "{sprint_wind}");
    }
}
