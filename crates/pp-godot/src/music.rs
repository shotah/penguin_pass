//! Music player. Two stream players crossfade on the `Music` bus.
//!
//! Bundled tracks load from `res://assets/music/` with `load_from_file`, the
//! same way models load without an import step, so an exported pack and the
//! editor run share one path. The rider's folder is an absolute path.

use std::path::Path;
use std::time::Instant;

use godot::classes::audio_stream_wav::LoopMode;
use godot::classes::file_access::ModeFlags;
use godot::classes::{
    AudioServer, AudioStream, AudioStreamMp3, AudioStreamOggVorbis, AudioStreamPlayer,
    AudioStreamWav, FileAccess, INode, Node, ProjectSettings, Time,
};
use godot::obj::{Base, Gd, WithBaseField};
use godot::prelude::*;
use pp_view::music::{self, Decision, Director, PlayRequest, TrackInfo};
pub use pp_view::music::{FolderReport, MusicScene};

/// Crossfade state. Plain numbers, so a frame does not allocate.
struct Fade {
    incoming: usize,
    outgoing: Option<usize>,
    elapsed: f32,
    duration: f32,
}

/// Disk read for the next stream. Godot objects stay on the main thread.
struct Incoming {
    request: PlayRequest,
    worker: std::thread::JoinHandle<Option<Vec<u8>>>,
}

/// Plays the title track and the ride rotation.
///
/// The game node calls [`Self::set_scene`], [`Self::set_ducked`],
/// [`Self::set_volume_percent`], and [`Self::set_user_folder`]. GDScript
/// tests call the same names with strings and ints.
#[derive(GodotClass)]
#[class(base = Node)]
pub struct MusicPlayer {
    base: Base<Node>,
    director: Director,
    players: [Option<Gd<AudioStreamPlayer>>; 2],
    live: Option<usize>,
    live_loops: bool,
    live_length: f32,
    heard: bool,
    handoff: bool,
    fade: Option<Fade>,
    bus_name: StringName,
    bus_index: i32,
    bus_created: bool,
    outputs_paused: bool,
    started: bool,
    pending: Option<MusicScene>,
    /// A track chosen while the bus is silent. Nothing is read until the volume is up.
    queued: Option<PlayRequest>,
    incoming: Option<Incoming>,
    /// Reads superseded by a newer track. Joined when they finish, never detached.
    retired: Vec<std::thread::JoinHandle<Option<Vec<u8>>>>,
    load_attempts: u8,
    slowest_load_us: u64,
    slowest_load_bytes: u64,
}

#[godot_api]
impl INode for MusicPlayer {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            director: Director::new(music::Library::default(), 1),
            players: [None, None],
            live: None,
            live_loops: false,
            live_length: 0.0,
            heard: false,
            handoff: false,
            fade: None,
            bus_name: StringName::from("Music"),
            bus_index: -1,
            bus_created: false,
            outputs_paused: false,
            started: false,
            pending: None,
            queued: None,
            incoming: None,
            retired: Vec::new(),
            load_attempts: 0,
            slowest_load_us: 0,
            slowest_load_bytes: 0,
        }
    }

    fn ready(&mut self) {
        if self.started {
            return;
        }
        self.ensure_bus();
        self.players[0] = Some(self.make_player("MusicA"));
        self.players[1] = Some(self.make_player("MusicB"));
        let seed = Time::singleton().get_ticks_msec();
        self.director.install_bundled(load_bundled_library(), seed);
        self.started = true;
        self.apply_bus();
        self.set_outputs_paused(!self.director.audible());
        if let Some(scene) = self.pending.take() {
            let decision = self.director.set_scene(scene);
            self.apply(decision);
        }
    }

    fn process(&mut self, delta: f64) {
        if !self.started {
            return;
        }
        // The file read finished on a worker. Decoding the buffer is the
        // only work left on this frame.
        self.reap_retired();
        self.poll_incoming();
        // Muted output is paused, so this returns before the fade or the
        // handoff. Nothing here builds a string or a buffer.
        if !self.director.audible() {
            self.set_outputs_paused(true);
            return;
        }
        self.set_outputs_paused(false);
        self.pump_fade(delta as f32);
        self.watch_track();
    }

    fn exit_tree(&mut self) {
        self.join_workers();
        self.fade = None;
        self.live = None;
        // Free the players here, while the audio server is still up. Leaving
        // the Ogg playback for the tree teardown leaks the packet sequence.
        for slot in 0..2 {
            self.silence_slot(slot);
        }
        self.remove_bus();
    }
}

impl MusicPlayer {
    /// Crossfade to the title track or the ride rotation.
    ///
    /// Setting the scene that is already playing does nothing, so a second
    /// call does not click or restart the stream.
    pub fn set_scene(&mut self, scene: MusicScene) {
        if !self.started {
            self.pending = Some(scene);
            return;
        }
        let decision = self.director.set_scene(scene);
        self.apply(decision);
    }

    /// Pull the bus down while the pause menu is open. The stream keeps going.
    pub fn set_ducked(&mut self, ducked: bool) {
        self.director.set_ducked(ducked);
        if self.started {
            self.apply_bus();
        }
    }

    /// `percent` is clamped to 0–100 and applied at once. Zero pauses the
    /// streams so they stop decoding.
    pub fn set_volume_percent(&mut self, percent: u8) {
        self.director.set_volume(percent);
        if !self.started {
            return;
        }
        self.apply_bus();
        self.set_outputs_paused(!self.director.audible());
        self.start_queued();
    }

    /// Use `folder` for the ride when it contains a playable file.
    ///
    /// `None`, an empty folder, or a folder that cannot be read falls back
    /// to the bundled ride tracks. The title track stays the bundled one.
    pub fn set_user_folder(&mut self, folder: Option<&Path>) -> FolderReport {
        let (tracks, report) = match folder {
            None => (Vec::new(), empty_report(None)),
            Some(folder) => match read_file_names(folder) {
                Err(()) => (Vec::new(), empty_report(Some("folder could not be read"))),
                Ok(names) => {
                    let (playable, report) = music::inspect_names(&names);
                    let tracks = playable
                        .into_iter()
                        .map(|name| {
                            let path = folder.join(&name);
                            let text = path.to_string_lossy().into_owned();
                            TrackInfo {
                                id: text.clone(),
                                path: text,
                                display_name: music::display_name_from_file(&name),
                            }
                        })
                        .collect();
                    (tracks, report)
                }
            },
        };
        let decision = self.director.set_user_tracks(tracks);
        if self.started {
            self.apply(decision);
        }
        report
    }

    /// Skip to the next ride track. On the title this does nothing.
    pub fn next_track(&mut self) {
        if !self.started {
            return;
        }
        let decision = self.director.next_track();
        self.apply(decision);
    }

    /// Display name of the current track.
    #[must_use]
    pub fn now_playing(&self) -> Option<String> {
        self.director.now_playing().map(str::to_owned)
    }

    /// Tracks in the ride rotation that is active right now.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.director.track_count()
    }

    fn start_queued(&mut self) {
        if !self.director.audible() {
            return;
        }
        if let Some(request) = self.queued.take() {
            self.begin(request);
        }
    }

    fn apply(&mut self, decision: Decision) {
        match decision {
            Decision::Hold => {}
            Decision::Stop => self.stop_all(),
            Decision::Play(request) => self.begin(request),
        }
    }

    fn begin(&mut self, request: PlayRequest) {
        // Muted startup must not decode. `--no-music` and a zero volume
        // leave the request here until the rider turns the volume up.
        if !self.director.audible() {
            self.queued = Some(request);
            return;
        }
        self.queued = None;
        // A 6 MB file took about 10 ms in `load_from_file`, most of it the
        // disk read. Read off the frame and only decode on the main thread.
        // Inside an exported pack the file is not on disk, so Godot reads it.
        if let Some(path) = filesystem_path(&request.track.path) {
            if let Some(previous) = self.incoming.take() {
                self.retired.push(previous.worker);
            }
            self.incoming = Some(Incoming {
                request,
                worker: std::thread::spawn(move || std::fs::read(path).ok()),
            });
            return;
        }
        self.begin_from_godot(request);
    }

    fn join_workers(&mut self) {
        if let Some(incoming) = self.incoming.take() {
            let _bytes = incoming.worker.join();
        }
        for worker in self.retired.drain(..) {
            let _bytes = worker.join();
        }
    }

    fn reap_retired(&mut self) {
        if self.retired.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.retired);
        for worker in pending {
            if worker.is_finished() {
                let _bytes = worker.join();
            } else {
                self.retired.push(worker);
            }
        }
    }

    fn poll_incoming(&mut self) {
        let ready = self
            .incoming
            .as_ref()
            .is_some_and(|incoming| incoming.worker.is_finished());
        if !ready {
            return;
        }
        let Some(incoming) = self.incoming.take() else {
            return;
        };
        let request = incoming.request;
        let bytes = incoming.worker.join().ok().flatten();
        let Some(bytes) = bytes else {
            self.fail_track(&request);
            return;
        };
        let started = Instant::now();
        match decode_bytes(&request.track.path, &bytes, request.looping) {
            Some((stream, length)) => {
                self.note_load(started, &request.track.path);
                self.arm(request, stream, length);
            }
            None => self.fail_track(&request),
        }
    }

    fn begin_from_godot(&mut self, request: PlayRequest) {
        let started = Instant::now();
        match load_clip(&request.track.path, request.looping) {
            Some((stream, length)) => {
                self.note_load(started, &request.track.path);
                self.arm(request, stream, length);
            }
            None => self.fail_track(&request),
        }
    }

    fn fail_track(&mut self, request: &PlayRequest) {
        self.load_attempts = self.load_attempts.saturating_add(1);
        eprintln!(
            "Penguin Pass: skipping unreadable music file {}",
            request.track.path
        );
        if self.load_attempts > 24 {
            self.stop_all();
            return;
        }
        match self.director.skip_failed(&request.track.id) {
            Decision::Play(next) => self.begin(next),
            Decision::Stop => self.stop_all(),
            Decision::Hold => {}
        }
    }

    fn note_load(&mut self, started: Instant, path: &str) {
        let us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        if us > self.slowest_load_us {
            self.slowest_load_us = us;
            self.slowest_load_bytes = file_len(path);
        }
    }

    fn arm(&mut self, request: PlayRequest, stream: Gd<AudioStream>, length: f32) {
        let slot = match self.live {
            Some(current) => 1 - current,
            None => 0,
        };
        let fade = request.fade_secs;
        self.ensure_player(slot);
        if let Some(mut player) = self.players[slot].clone() {
            player.set_stream(&stream);
            player.set_volume_db(if fade <= 0.0 { 0.0 } else { -80.0 });
            player.play();
            if self.outputs_paused {
                player.set_stream_paused(true);
            }
        }
        self.live_loops = request.looping;
        self.live_length = length;
        self.heard = false;
        self.handoff = false;
        self.load_attempts = 0;
        if fade <= 0.0 {
            if let Some(previous) = self.live
                && previous != slot
            {
                self.silence_slot(previous);
            }
            self.live = Some(slot);
            self.fade = None;
            return;
        }
        self.fade = Some(Fade {
            incoming: slot,
            outgoing: self.live,
            elapsed: 0.0,
            duration: fade,
        });
        self.live = Some(slot);
    }

    fn pump_fade(&mut self, delta: f32) {
        let Some(fade) = &self.fade else {
            return;
        };
        let incoming = fade.incoming;
        let outgoing = fade.outgoing;
        let elapsed = fade.elapsed + delta;
        let duration = fade.duration.max(0.001);
        let t = (elapsed / duration).clamp(0.0, 1.0);
        let done = elapsed >= duration;
        if let Some(fade) = self.fade.as_mut() {
            fade.elapsed = elapsed;
        }
        let angle = t * std::f32::consts::FRAC_PI_2;
        self.set_slot_db(incoming, gain_db(angle.sin()));
        if let Some(outgoing) = outgoing {
            self.set_slot_db(outgoing, gain_db(angle.cos()));
        }
        if done {
            if let Some(outgoing) = outgoing {
                self.silence_slot(outgoing);
            }
            self.fade = None;
        }
    }

    fn watch_track(&mut self) {
        if self.fade.is_some() || self.handoff {
            return;
        }
        let Some(slot) = self.live else {
            return;
        };
        let (playing, pos) = {
            let Some(player) = self.players[slot].as_ref() else {
                return;
            };
            (player.is_playing(), player.get_playback_position())
        };
        if playing {
            self.heard = true;
        }
        let ended = !playing && (self.heard || pos > 0.05);
        let near_end = !self.live_loops
            && self.live_length > music::TRACK_CROSSFADE_SECS + 0.2
            && pos >= self.live_length - music::TRACK_CROSSFADE_SECS;
        let restart_loop = self.live_loops && ended && self.heard;
        if restart_loop || (!self.live_loops && (near_end || (ended && self.heard))) {
            self.handoff = true;
            let decision = self.director.on_finished();
            self.apply(decision);
        }
    }

    fn stop_all(&mut self) {
        if let Some(incoming) = self.incoming.take() {
            self.retired.push(incoming.worker);
        }
        self.reap_retired();
        self.fade = None;
        self.live = None;
        self.handoff = false;
        self.heard = false;
        self.silence_slot(0);
        self.silence_slot(1);
    }

    /// Drop the player node. Clearing the stream in place leaks the Ogg
    /// playback; freeing the node releases it while the audio server is up.
    fn silence_slot(&mut self, slot: usize) {
        let Some(mut player) = self.players[slot].take() else {
            return;
        };
        if player.is_instance_valid() {
            // A paused stream keeps its playback unless it is unpaused, and
            // freeing the player while the stream property is set leaves the
            // playback and the packet sequence behind.
            player.set_stream_paused(false);
            player.stop();
            player.free();
        }
    }

    fn ensure_player(&mut self, slot: usize) {
        if self.players[slot].is_some() {
            return;
        }
        let name = if slot == 0 { "MusicA" } else { "MusicB" };
        self.players[slot] = Some(self.make_player(name));
    }

    fn set_slot_db(&mut self, slot: usize, db: f32) {
        if let Some(player) = self.players[slot].as_mut() {
            player.set_volume_db(db);
        }
    }

    fn set_outputs_paused(&mut self, paused: bool) {
        if self.outputs_paused == paused {
            return;
        }
        self.outputs_paused = paused;
        for slot in 0..2 {
            if let Some(player) = self.players[slot].as_mut() {
                player.set_stream_paused(paused);
            }
        }
    }

    fn make_player(&mut self, name: &str) -> Gd<AudioStreamPlayer> {
        let mut player = AudioStreamPlayer::new_alloc();
        player.set_name(name);
        player.set_bus(&self.bus_name);
        self.base_mut().add_child(&player);
        player
    }

    fn ensure_bus(&mut self) {
        let mut server = AudioServer::singleton();
        let existing = server.get_bus_index(&self.bus_name);
        if existing >= 0 {
            self.bus_index = existing;
            self.bus_created = false;
            return;
        }
        server.add_bus();
        let index = server.get_bus_count() - 1;
        server.set_bus_name(index, "Music");
        server.set_bus_send(index, &StringName::from("Master"));
        self.bus_index = index;
        self.bus_created = true;
    }

    fn remove_bus(&mut self) {
        if !self.bus_created || self.bus_index <= 0 {
            return;
        }
        let mut server = AudioServer::singleton();
        if self.bus_index < server.get_bus_count() {
            server.remove_bus(self.bus_index);
        }
        self.bus_created = false;
        self.bus_index = -1;
    }

    fn apply_bus(&mut self) {
        if self.bus_index < 0 {
            return;
        }
        let mut server = AudioServer::singleton();
        match self.director.bus_db() {
            Some(db) => {
                server.set_bus_mute(self.bus_index, false);
                server.set_bus_volume_db(self.bus_index, db);
            }
            None => server.set_bus_mute(self.bus_index, true),
        }
    }
}

#[godot_api]
impl MusicPlayer {
    /// `"title"` or `"ride"`.
    #[func(rename = set_scene)]
    fn gd_set_scene(&mut self, scene: GString) {
        match scene.to_string().to_ascii_lowercase().as_str() {
            "title" => self.set_scene(MusicScene::Title),
            "ride" => self.set_scene(MusicScene::Ride),
            _ => {}
        }
    }

    /// Pause-menu duck.
    #[func(rename = set_ducked)]
    fn gd_set_ducked(&mut self, ducked: bool) {
        self.set_ducked(ducked);
    }

    /// Slider, 0 to 100.
    #[func(rename = set_volume_percent)]
    fn gd_set_volume_percent(&mut self, percent: i64) {
        let clamped = percent.clamp(0, 100);
        self.set_volume_percent(u8::try_from(clamped).unwrap_or(0));
    }

    /// Absolute folder, or empty to clear it. Returns the found/skipped line.
    #[func(rename = set_user_folder)]
    fn gd_set_user_folder(&mut self, folder: GString) -> GString {
        let text = folder.to_string();
        let report = if text.is_empty() {
            self.set_user_folder(None)
        } else {
            self.set_user_folder(Some(Path::new(&text)))
        };
        GString::from(report.summary().as_str())
    }

    /// Next ride track.
    #[func(rename = next_track)]
    fn gd_next_track(&mut self) {
        self.next_track();
    }

    /// Current display name, or empty when silent.
    #[func(rename = now_playing)]
    fn gd_now_playing(&self) -> GString {
        GString::from(self.now_playing().unwrap_or_default().as_str())
    }

    /// Ride tracks in the active source.
    #[func(rename = track_count)]
    fn gd_track_count(&self) -> i64 {
        i64::try_from(self.track_count()).unwrap_or(0)
    }

    /// Length of the stream that last started, in seconds.
    #[func]
    fn live_length_secs(&self) -> f64 {
        f64::from(self.live_length)
    }

    /// Slowest main-thread decode this process has done, in microseconds.
    #[func]
    fn slowest_load_us(&self) -> i64 {
        i64::try_from(self.slowest_load_us).unwrap_or(i64::MAX)
    }

    /// Size in bytes of the file that set [`Self::slowest_load_us`].
    #[func]
    fn slowest_load_bytes(&self) -> i64 {
        i64::try_from(self.slowest_load_bytes).unwrap_or(0)
    }
}

fn empty_report(error: Option<&str>) -> FolderReport {
    FolderReport {
        found: 0,
        skipped: Vec::new(),
        error: error.map(str::to_owned),
    }
}

fn read_file_names(folder: &Path) -> Result<Vec<String>, ()> {
    let entries = std::fs::read_dir(folder).map_err(|_| ())?;
    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_file() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn load_bundled_library() -> music::Library {
    let Some(file) = FileAccess::open("res://assets/music/music.json", ModeFlags::READ) else {
        eprintln!("Penguin Pass: music manifest missing (res://assets/music/music.json)");
        return music::Library::default();
    };
    let text = file.get_as_text().to_string();
    match music::library_from_manifest(&text, "res://assets/music/") {
        Ok(library) => library,
        Err(error) => {
            eprintln!("Penguin Pass: music manifest rejected ({error})");
            music::Library::default()
        }
    }
}

fn filesystem_path(path: &str) -> Option<std::path::PathBuf> {
    let direct = Path::new(path);
    if direct.is_file() {
        return Some(direct.to_path_buf());
    }
    if !path.starts_with("res://") {
        return None;
    }
    let global = ProjectSettings::singleton()
        .globalize_path(path)
        .to_string();
    let global_path = std::path::PathBuf::from(global);
    global_path.is_file().then_some(global_path)
}

fn decode_bytes(path: &str, bytes: &[u8], looping: bool) -> Option<(Gd<AudioStream>, f32)> {
    let packed = PackedByteArray::from(bytes);
    let extension = path.rsplit('.').next().unwrap_or("");
    if extension.eq_ignore_ascii_case("mp3") {
        let mut clip = AudioStreamMp3::load_from_buffer(&packed)?;
        clip.set_loop(looping);
        let stream = clip.upcast::<AudioStream>();
        let length = stream.get_length() as f32;
        return Some((stream, length));
    }
    if extension.eq_ignore_ascii_case("ogg") {
        let mut clip = AudioStreamOggVorbis::load_from_buffer(&packed)?;
        clip.set_loop(looping);
        let stream = clip.upcast::<AudioStream>();
        let length = stream.get_length() as f32;
        return Some((stream, length));
    }
    if extension.eq_ignore_ascii_case("wav") {
        let mut clip = AudioStreamWav::load_from_buffer(&packed)?;
        clip.set_loop_mode(if looping {
            LoopMode::FORWARD
        } else {
            LoopMode::DISABLED
        });
        let stream = clip.upcast::<AudioStream>();
        let length = stream.get_length() as f32;
        return Some((stream, length));
    }
    None
}

fn load_clip(path: &str, looping: bool) -> Option<(Gd<AudioStream>, f32)> {
    let extension = path.rsplit('.').next().unwrap_or("");
    if extension.eq_ignore_ascii_case("mp3") {
        let mut clip = AudioStreamMp3::load_from_file(path).or_else(|| load_mp3_bytes(path))?;
        clip.set_loop(looping);
        let stream = clip.upcast::<AudioStream>();
        let length = stream.get_length() as f32;
        return Some((stream, length));
    }
    if extension.eq_ignore_ascii_case("ogg") {
        let mut clip =
            AudioStreamOggVorbis::load_from_file(path).or_else(|| load_ogg_bytes(path))?;
        clip.set_loop(looping);
        let stream = clip.upcast::<AudioStream>();
        let length = stream.get_length() as f32;
        return Some((stream, length));
    }
    if extension.eq_ignore_ascii_case("wav") {
        let mut clip = AudioStreamWav::load_from_file(path).or_else(|| load_wav_bytes(path))?;
        clip.set_loop_mode(if looping {
            LoopMode::FORWARD
        } else {
            LoopMode::DISABLED
        });
        let stream = clip.upcast::<AudioStream>();
        let length = stream.get_length() as f32;
        return Some((stream, length));
    }
    None
}

fn load_mp3_bytes(path: &str) -> Option<Gd<AudioStreamMp3>> {
    let bytes = read_bytes(path)?;
    AudioStreamMp3::load_from_buffer(&bytes)
}

fn load_ogg_bytes(path: &str) -> Option<Gd<AudioStreamOggVorbis>> {
    let bytes = read_bytes(path)?;
    AudioStreamOggVorbis::load_from_buffer(&bytes)
}

fn load_wav_bytes(path: &str) -> Option<Gd<AudioStreamWav>> {
    let bytes = read_bytes(path)?;
    AudioStreamWav::load_from_buffer(&bytes)
}

fn read_bytes(path: &str) -> Option<PackedByteArray> {
    match std::fs::read(path) {
        Ok(bytes) => Some(PackedByteArray::from(bytes.as_slice())),
        Err(_) => {
            let global = ProjectSettings::singleton().globalize_path(path);
            let bytes = std::fs::read(global.to_string()).ok()?;
            Some(PackedByteArray::from(bytes.as_slice()))
        }
    }
}

fn file_len(path: &str) -> u64 {
    if let Ok(meta) = std::fs::metadata(path) {
        return meta.len();
    }
    let global = ProjectSettings::singleton().globalize_path(path);
    std::fs::metadata(global.to_string())
        .map(|meta| meta.len())
        .unwrap_or(0)
}

fn gain_db(amplitude: f32) -> f32 {
    if amplitude <= 0.000_1 {
        -80.0
    } else {
        20.0 * amplitude.log10()
    }
}
