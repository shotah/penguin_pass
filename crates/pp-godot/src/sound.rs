//! Ambience and effects. Levels come from [`pp_view::sound`]; this node only
//! loads files, keeps the buses, and starts the one-shots.

use godot::classes::{
    AudioEffect, AudioEffectCapture, AudioServer, AudioStream, AudioStreamOggVorbis,
    AudioStreamPlayer, INode, Node, ProjectSettings,
};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_view::sound::{Clip, Cue, DEMO_SECS, Demo, Director, Layer, Mix, RideInput, SoundEvent};

const POOL: usize = 6;

/// Looping layers and a small pool of one-shots.
#[derive(GodotClass)]
#[class(base = Node)]
pub struct SoundPlayer {
    base: Base<Node>,
    director: Director,
    streams: Vec<Option<Gd<AudioStream>>>,
    layers: Vec<Option<Gd<AudioStreamPlayer>>>,
    pool: Vec<Gd<AudioStreamPlayer>>,
    pool_at: usize,
    ambient_bus: StringName,
    effects_bus: StringName,
    ambient_index: i32,
    effects_index: i32,
    ambient_created: bool,
    effects_created: bool,
    ambient_percent: u8,
    effects_percent: u8,
    files_ok: i32,
    files_missing: i32,
    logged: [f32; 9],
    coast_logged: bool,
    demo: Option<Demo>,
    capture_path: Option<String>,
    capture_fx: Option<Gd<AudioEffectCapture>>,
    pcm: Vec<f32>,
    capture_done: bool,
    next_capture_log: f32,
    started: bool,
}

#[godot_api]
impl INode for SoundPlayer {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            director: Director::new(1),
            streams: Vec::new(),
            layers: Vec::new(),
            pool: Vec::new(),
            pool_at: 0,
            ambient_bus: StringName::from("Ambient"),
            effects_bus: StringName::from("Effects"),
            ambient_index: -1,
            effects_index: -1,
            ambient_created: false,
            effects_created: false,
            ambient_percent: 0,
            effects_percent: 0,
            files_ok: 0,
            files_missing: 0,
            logged: [0.0; 9],
            coast_logged: false,
            demo: None,
            capture_path: None,
            capture_fx: None,
            pcm: Vec::new(),
            capture_done: false,
            next_capture_log: 0.0,
            started: false,
        }
    }

    fn ready(&mut self) {
        if self.started {
            return;
        }
        self.ensure_buses();
        self.load_clips();
        self.make_layers();
        self.make_pool();
        self.started = true;
        let line = self.report_line();
        eprintln!("{line}");
    }

    fn exit_tree(&mut self) {
        self.finish_capture();
        for slot in &mut self.layers {
            free_player(slot.take());
        }
        let pool = std::mem::take(&mut self.pool);
        for player in pool {
            free_player(Some(player));
        }
        self.remove_buses();
    }
}

impl SoundPlayer {
    /// Slider positions, 0 to 100. Zero pauses that bus's streams.
    pub fn set_volumes(&mut self, ambient: u8, effects: u8) {
        self.ambient_percent = ambient.min(100);
        self.effects_percent = effects.min(100);
        if self.started {
            self.apply_buses();
        }
    }

    /// Record a scripted ride to `path`. The mix ignores the live ride.
    pub fn arm_capture(&mut self, path: &str) {
        self.capture_path = Some(path.to_owned());
        self.demo = Some(Demo::new());
        self.set_volumes(80, 80);
        if self.started {
            self.start_record();
        }
    }

    /// Queue a stinger for the next frame. Ignored while a capture is running.
    pub fn push_event(&mut self, event: SoundEvent) {
        if self.demo.is_some() {
            return;
        }
        self.director.push(event);
    }

    /// One frame. `true` when a capture has been written and the game should quit.
    pub fn sync(&mut self, input: RideInput, dt: f32, event: Option<SoundEvent>) -> bool {
        if !self.started {
            return false;
        }
        if self.capture_done {
            return true;
        }
        if self.demo.is_some() && self.capture_fx.is_none() {
            self.start_record();
        }
        self.drain_capture();
        let (input, event) = if let Some(demo) = self.demo.as_mut() {
            let (input, event) = demo.advance(dt);
            (input, event)
        } else {
            (input, event)
        };
        if let Some(event) = event {
            self.director.push(event);
        }
        let mix = self.director.step(&input, dt);
        if self.audible() {
            self.apply(&mix);
            self.log_changes(&mix);
        }
        if self.demo.is_some() {
            let t = self.demo.as_ref().map(|demo| demo.t).unwrap_or(0.0);
            if t >= self.next_capture_log {
                self.log_capture(t, &mix);
                self.next_capture_log = t + 0.5;
            }
            if t >= DEMO_SECS {
                self.finish_capture();
                self.capture_done = true;
                return true;
            }
        }
        false
    }

    /// Power the scripted ride wants, while a capture is running.
    #[must_use]
    pub fn demo_power_w(&self) -> Option<f32> {
        self.demo
            .as_ref()
            .map(|demo| pp_view::sound::demo_input(demo.t).power_w)
    }

    /// `PENGUIN_PASS_SOUND files=<n> missing=<n>`.
    #[must_use]
    pub fn report_line(&self) -> String {
        format!(
            "PENGUIN_PASS_SOUND files={} missing={}",
            self.files_ok, self.files_missing
        )
    }

    fn audible(&self) -> bool {
        self.ambient_percent > 0 || self.effects_percent > 0
    }

    fn load_clips(&mut self) {
        self.streams.clear();
        self.files_ok = 0;
        self.files_missing = 0;
        for clip in Clip::ALL {
            match load_ogg(clip.file(), clip.loops()) {
                Some(stream) => {
                    self.streams.push(Some(stream));
                    self.files_ok += 1;
                }
                None => {
                    self.streams.push(None);
                    if clip.required() {
                        self.files_missing += 1;
                        eprintln!("PENGUIN_PASS_SOUND missing {}", clip.file());
                    } else {
                        eprintln!("PENGUIN_PASS_SOUND optional-missing {}", clip.file());
                    }
                }
            }
        }
    }

    fn make_layers(&mut self) {
        for layer in Layer::ALL {
            let player = layer.clip().and_then(|clip| {
                let stream = self.stream(clip)?;
                let bus = if clip.ambient() {
                    self.ambient_bus.clone()
                } else {
                    self.effects_bus.clone()
                };
                let mut player = self.spawn(&format!("Layer{}", layer.name()), &bus);
                player.set_stream(&stream);
                Some(player)
            });
            self.layers.push(player);
        }
    }

    fn make_pool(&mut self) {
        for index in 0..POOL {
            let name = format!("Shot{index}");
            let bus = self.effects_bus.clone();
            let player = self.spawn(&name, &bus);
            self.pool.push(player);
        }
    }

    fn spawn(&mut self, name: &str, bus: &StringName) -> Gd<AudioStreamPlayer> {
        let mut player = AudioStreamPlayer::new_alloc();
        player.set_name(name);
        player.set_bus(bus);
        self.base_mut().add_child(&player);
        player
    }

    fn apply(&mut self, mix: &Mix) {
        for (index, layer) in Layer::ALL.iter().enumerate() {
            let out = mix.layer(*layer);
            let Some(player) = self.layers.get_mut(index).and_then(Option::as_mut) else {
                continue;
            };
            player.set_volume_db(gain_db(out.level));
            player.set_pitch_scale(out.pitch.max(0.05));
            if out.level < 0.02 {
                if player.is_playing() {
                    player.set_stream_paused(true);
                }
            } else if player.is_playing() {
                player.set_stream_paused(false);
            } else {
                player.play();
            }
        }
        let cues: Vec<Cue> = mix.cues().iter().copied().flatten().collect();
        for cue in cues {
            self.play_cue(cue);
        }
    }

    fn play_cue(&mut self, cue: Cue) {
        let Some(stream) = self.stream(cue.clip) else {
            return;
        };
        if self.pool.is_empty() {
            return;
        }
        let index = self.pool_at % self.pool.len();
        self.pool_at = index + 1;
        let bus = if cue.clip.ambient() {
            self.ambient_bus.clone()
        } else {
            self.effects_bus.clone()
        };
        let mut player = self.pool[index].clone();
        player.set_bus(&bus);
        player.set_stream(&stream);
        player.set_pitch_scale(cue.pitch.max(0.05));
        player.set_volume_db(gain_db(cue.gain));
        player.set_stream_paused(false);
        player.play();
    }

    fn stream(&self, clip: Clip) -> Option<Gd<AudioStream>> {
        let index = Clip::ALL.iter().position(|item| *item == clip)?;
        self.streams.get(index).and_then(Clone::clone)
    }

    fn log_changes(&mut self, mix: &Mix) {
        if mix.coasting() != self.coast_logged {
            self.coast_logged = mix.coasting();
            let bit = if self.coast_logged { 1 } else { 0 };
            eprintln!("PENGUIN_PASS_SOUND coasting={bit}");
        }
        for (index, layer) in Layer::ALL.iter().enumerate() {
            let level = mix.layer(*layer).level;
            let previous = self.logged[index];
            let crossed = (previous < 0.05 && level >= 0.05) || (previous >= 0.05 && level < 0.05);
            if crossed || (level - previous).abs() >= 0.1 {
                self.logged[index] = level;
                eprintln!(
                    "PENGUIN_PASS_SOUND {} level={:.2} pitch={:.2}",
                    layer.name(),
                    level,
                    mix.layer(*layer).pitch
                );
            }
        }
    }

    fn log_capture(&self, t: f32, mix: &Mix) {
        let mut ambient = 0.0;
        let mut effects = 0.0;
        for layer in Layer::ALL {
            let level = mix.layer(layer).level;
            match layer.clip() {
                Some(clip) if clip.ambient() => ambient += level,
                _ => effects += level,
            }
        }
        eprintln!(
            "PENGUIN_PASS_SOUND t={t:.1} ambient={ambient:.2} effects={effects:.2} ratchet={:.2} tyre={:.2} drivetrain={:.2} wind={:.2} breath={:.2}",
            mix.layer(Layer::Ratchet).level,
            mix.layer(Layer::Tyre).level,
            mix.layer(Layer::Drivetrain).level,
            mix.layer(Layer::WindRush).level,
            mix.layer(Layer::Breath).level,
        );
    }

    fn start_record(&mut self) {
        if self.capture_fx.is_some() {
            return;
        }
        let mut capture = AudioEffectCapture::new_gd();
        capture.set_buffer_length(12.0);
        let effect = capture.clone().upcast::<AudioEffect>();
        AudioServer::singleton().add_bus_effect(0, &effect);
        self.capture_fx = Some(capture);
    }

    fn drain_capture(&mut self) {
        let available = self
            .capture_fx
            .as_ref()
            .map(|capture| capture.get_frames_available())
            .unwrap_or(0);
        if available <= 0 {
            return;
        }
        let Some(capture) = self.capture_fx.clone() else {
            return;
        };
        let frames = capture.get_buffer(available);
        for index in 0..frames.len() {
            let Some(frame) = frames.get(index) else {
                continue;
            };
            self.pcm.push(frame.x);
            self.pcm.push(frame.y);
        }
    }

    fn finish_capture(&mut self) {
        let Some(path) = self.capture_path.take() else {
            return;
        };
        if self.capture_fx.is_none() && self.pcm.is_empty() {
            return;
        }
        self.drain_capture();
        self.capture_fx.take();
        let rate = AudioServer::singleton().get_mix_rate();
        match write_wav(&path, rate, &self.pcm) {
            Ok(peak) => eprintln!(
                "PENGUIN_PASS_SOUND capture={path} frames={} rate={rate:.0} peak={peak:.3}",
                self.pcm.len() / 2
            ),
            Err(err) => eprintln!("PENGUIN_PASS_SOUND capture={path} save={err}"),
        }
    }

    fn ensure_buses(&mut self) {
        self.ambient_index = ensure_bus("Ambient", &mut self.ambient_created);
        self.effects_index = ensure_bus("Effects", &mut self.effects_created);
        self.apply_buses();
    }

    fn apply_buses(&mut self) {
        apply_bus(self.ambient_index, self.ambient_percent);
        apply_bus(self.effects_index, self.effects_percent);
    }

    fn remove_buses(&mut self) {
        remove_bus(self.effects_index, self.effects_created);
        remove_bus(self.ambient_index, self.ambient_created);
        self.ambient_created = false;
        self.effects_created = false;
    }
}

fn ensure_bus(label: &str, created: &mut bool) -> i32 {
    let mut server = AudioServer::singleton();
    let name = StringName::from(label);
    let existing = server.get_bus_index(&name);
    if existing >= 0 {
        *created = false;
        return existing;
    }
    server.add_bus();
    let index = server.get_bus_count() - 1;
    server.set_bus_name(index, label);
    server.set_bus_send(index, &StringName::from("Master"));
    *created = true;
    index
}

fn apply_bus(index: i32, percent: u8) {
    if index < 0 {
        return;
    }
    let mut server = AudioServer::singleton();
    match pp_view::music::volume_db(percent) {
        Some(db) => {
            server.set_bus_mute(index, false);
            server.set_bus_volume_db(index, db);
        }
        None => server.set_bus_mute(index, true),
    }
}

fn remove_bus(index: i32, created: bool) {
    if !created || index <= 0 {
        return;
    }
    let mut server = AudioServer::singleton();
    if index < server.get_bus_count() {
        let name = server.get_bus_name(index);
        if name == "Ambient" || name == "Effects" {
            server.remove_bus(index);
        }
    }
}

fn free_player(player: Option<Gd<AudioStreamPlayer>>) {
    let Some(mut player) = player else {
        return;
    };
    if player.is_instance_valid() {
        player.set_stream_paused(false);
        player.stop();
        player.free();
    }
}

fn load_ogg(file: &str, looping: bool) -> Option<Gd<AudioStream>> {
    let path = format!("res://assets/sfx/{file}");
    let mut clip = AudioStreamOggVorbis::load_from_file(&GString::from(path.as_str()))
        .or_else(|| load_ogg_buffer(&path))?;
    clip.set_loop(looping);
    Some(clip.upcast())
}

fn load_ogg_buffer(path: &str) -> Option<Gd<AudioStreamOggVorbis>> {
    let global = ProjectSettings::singleton().globalize_path(path);
    let bytes = std::fs::read(global.to_string()).ok()?;
    AudioStreamOggVorbis::load_from_buffer(&PackedByteArray::from(bytes.as_slice()))
}

fn write_wav(path: &str, rate: f32, interleaved: &[f32]) -> Result<f32, String> {
    if interleaved.is_empty() {
        return Err("no frames from the capture effect".to_owned());
    }
    let mut peak = 0.0_f32;
    let mut body = Vec::with_capacity(interleaved.len() * 2);
    for sample in interleaved {
        peak = peak.max(sample.abs());
        let clamped = sample.clamp(-1.0, 1.0);
        let value = (clamped * 32767.0).round() as i16;
        body.extend_from_slice(&value.to_le_bytes());
    }
    let rate_u = u32::try_from(rate.round().max(1.0) as i64).unwrap_or(44100);
    let channels = 2_u16;
    let bits = 16_u16;
    let block = channels * bits / 8;
    let byte_rate = rate_u * u32::from(block);
    let data_len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    let mut file = Vec::with_capacity(44 + body.len());
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + data_len).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16_u32.to_le_bytes());
    file.extend_from_slice(&1_u16.to_le_bytes());
    file.extend_from_slice(&channels.to_le_bytes());
    file.extend_from_slice(&rate_u.to_le_bytes());
    file.extend_from_slice(&byte_rate.to_le_bytes());
    file.extend_from_slice(&block.to_le_bytes());
    file.extend_from_slice(&bits.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&data_len.to_le_bytes());
    file.extend_from_slice(&body);
    std::fs::write(path, file).map_err(|err| err.to_string())?;
    Ok(peak)
}

fn gain_db(amplitude: f32) -> f32 {
    if amplitude <= 0.000_1 {
        -80.0
    } else {
        20.0 * amplitude.clamp(0.0, 1.0).log10()
    }
}
