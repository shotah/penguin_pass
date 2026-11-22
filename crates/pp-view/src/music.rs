//! Music playlist rules. No Godot types: the player node executes decisions.
//!
//! Playable files are mp3, ogg, and wav. A seeded rotation plays every ride
//! track once before any repeat, and the next cycle does not open on the
//! track that just finished. The title track is always the bundled one.
//! Volume is a 0–100 slider mapped to decibels.

use serde::Deserialize;

/// Crossfade when the scene changes, in seconds.
pub const SCENE_CROSSFADE_SECS: f32 = 1.5;
/// Crossfade when a ride track hands off to the next one, in seconds.
pub const TRACK_CROSSFADE_SECS: f32 = 0.5;
/// Slider position used when the rider has not chosen one.
pub const DEFAULT_VOLUME_PERCENT: u8 = 60;
/// How far the pause menu pulls the music back, in decibels.
pub const DUCK_DB: f32 = -12.0;

/// Ride music flavor. Filters bundled tracks when the rider has no custom folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CourseMood {
    /// High-energy climbs and sprints.
    #[default]
    Energetic,
    /// Steady tempo and rolling roads.
    Cruising,
    /// Open roads and sparse arrangements.
    Solitary,
}

impl CourseMood {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "energetic" => Some(Self::Energetic),
            "cruising" => Some(Self::Cruising),
            "solitary" => Some(Self::Solitary),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Energetic => "energetic",
            Self::Cruising => "cruising",
            Self::Solitary => "solitary",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Energetic => "Energetic",
            Self::Cruising => "Cruising",
            Self::Solitary => "Solitary",
        }
    }
}

/// Where the music should sit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicScene {
    /// Title, course list, and the summary.
    Title,
    /// The ride itself. Pause ducks this; it does not switch back to the title.
    Ride,
}

/// One file the player can start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackInfo {
    /// Stable id. Bundled tracks use the `res://` path. The rider's files use
    /// the absolute path.
    pub id: String,
    /// Path passed to the loader.
    pub path: String,
    /// Line for "now playing".
    pub display_name: String,
    /// Bundled mood tags. Empty means every mood.
    pub moods: Vec<CourseMood>,
}

/// Bundled library after the manifest has been read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    /// Menu track. Absent when the manifest has none.
    pub title: Option<TrackInfo>,
    /// Ride rotation, in manifest order. The shuffle reorders playback.
    pub rides: Vec<TrackInfo>,
}

/// What to start, and how long the crossfade should be.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayRequest {
    /// Track to load.
    pub track: TrackInfo,
    /// Title loops. Ride tracks play once.
    pub looping: bool,
    /// `0.0` starts at full level. A scene change uses [`SCENE_CROSSFADE_SECS`].
    pub fade_secs: f32,
}

/// What the player node should do. `Hold` leaves the current stream alone.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Keep playing. Volume may still have changed.
    Hold,
    /// Start this track. Crossfade when another stream is already up.
    Play(PlayRequest),
    /// Fade out. The library has nothing for this scene.
    Stop,
}

/// A file the folder scan did not take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedFile {
    /// File name, not the full path.
    pub name: String,
    /// Short reason, such as `FLAC is not supported`.
    pub reason: String,
}

/// Result of pointing the ride music at a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderReport {
    /// Playable files.
    pub found: usize,
    /// Files left out, with reasons. Hidden files are omitted, not listed.
    pub skipped: Vec<SkippedFile>,
    /// Set when the folder itself could not be read. `found` is then zero.
    pub error: Option<String>,
}

impl FolderReport {
    /// One line for the settings row.
    ///
    /// `3 tracks found, 2 skipped (FLAC is not supported)`.
    #[must_use]
    pub fn summary(&self) -> String {
        let count = self.found;
        let found = if count == 1 {
            "1 track found".to_owned()
        } else {
            format!("{count} tracks found")
        };
        if let Some(error) = &self.error {
            return format!("{found} ({error})");
        }
        if self.skipped.is_empty() {
            return found;
        }
        let mut groups: Vec<(&str, usize)> = Vec::new();
        for skip in &self.skipped {
            if let Some(group) = groups.iter_mut().find(|group| group.0 == skip.reason) {
                group.1 += 1;
            } else {
                groups.push((skip.reason.as_str(), 1));
            }
        }
        let mut text = found;
        for (reason, count) in groups {
            text.push_str(&format!(", {count} skipped ({reason})"));
        }
        text
    }
}

/// Split a directory listing into playable names and the rest.
///
/// Names are file names, not paths. Hidden names (a leading `.`) are dropped.
/// Playable names come back sorted by the lower-case name, then the original
/// spelling, so the order does not depend on the filesystem.
#[must_use]
pub fn inspect_names<I, S>(names: I) -> (Vec<String>, FolderReport)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut playable = Vec::new();
    let mut skipped = Vec::new();
    for name in names {
        let name = name.as_ref();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        let file_name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        if file_name.starts_with('.') {
            continue;
        }
        match playable_extension(file_name) {
            Ok(()) => playable.push(file_name.to_owned()),
            Err(reason) => skipped.push(SkippedFile {
                name: file_name.to_owned(),
                reason,
            }),
        }
    }
    playable.sort_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    let found = playable.len();
    (
        playable,
        FolderReport {
            found,
            skipped,
            error: None,
        },
    )
}

/// "now playing" text for a file that is not in the manifest.
///
/// The extension is removed. Hyphens and underscores become spaces.
#[must_use]
pub fn display_name_from_file(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => base,
    };
    let pretty = stem
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if pretty.is_empty() {
        base.to_owned()
    } else {
        pretty
    }
}

fn playable_extension(file_name: &str) -> Result<(), String> {
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return Err("file type is not supported".to_owned());
    };
    if stem.is_empty() {
        return Err("file type is not supported".to_owned());
    }
    if extension.eq_ignore_ascii_case("mp3")
        || extension.eq_ignore_ascii_case("ogg")
        || extension.eq_ignore_ascii_case("wav")
    {
        Ok(())
    } else if extension.is_empty() {
        Err("file type is not supported".to_owned())
    } else {
        Err(format!(
            "{} is not supported",
            extension.to_ascii_uppercase()
        ))
    }
}

/// Slider percent to bus decibels.
///
/// The gain is the square of the slider fraction, then converted with
/// `20 log10`. Equal steps on the slider then land closer to equal loudness
/// than a straight gain would, and 100 is unity (0 dB). Zero is silent:
/// the player stops the streams instead of feeding a huge negative gain.
#[must_use]
pub fn volume_db(percent: u8) -> Option<f32> {
    let percent = percent.min(100);
    if percent == 0 {
        return None;
    }
    let amplitude = (f32::from(percent) / 100.0).powi(2);
    Some(20.0 * amplitude.log10())
}

/// Bus gain with the pause-menu duck applied. Silence stays silence.
#[must_use]
pub fn bus_db(percent: u8, ducked: bool) -> Option<f32> {
    let db = volume_db(percent)?;
    Some(if ducked { db + DUCK_DB } else { db })
}

/// Seeded ride order. The same seed and the same calls produce the same tracks.
#[derive(Clone, Debug)]
pub struct Rotation {
    seed: u64,
    cycle: u64,
    queue: Vec<String>,
    last: Option<String>,
}

impl Rotation {
    /// `seed` picks the shuffle. Zero is allowed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            cycle: 0,
            queue: Vec::new(),
            last: None,
        }
    }

    /// Next id from `ids`. `None` when `ids` is empty.
    ///
    /// Ids that disappeared since the last call are dropped. A fresh cycle
    /// never starts with [`Self`]'s previous id when at least two ids remain.
    pub fn next(&mut self, ids: &[String]) -> Option<String> {
        if ids.is_empty() {
            self.queue.clear();
            return None;
        }
        self.queue
            .retain(|id| ids.iter().any(|candidate| candidate == id));
        if self.queue.is_empty() {
            self.deal(ids);
        }
        let id = self.queue.remove(0);
        self.last = Some(id.clone());
        Some(id)
    }

    fn deal(&mut self, ids: &[String]) {
        let mut order = ids.to_vec();
        order.sort();
        let mut rng = Rng::new(self.seed ^ self.cycle.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        shuffle(&mut order, &mut rng);
        if order.len() >= 2
            && self
                .last
                .as_ref()
                .is_some_and(|last| order.first() == Some(last))
        {
            order.swap(0, 1);
        }
        self.cycle = self.cycle.wrapping_add(1);
        self.queue = order;
    }
}

fn shuffle(items: &mut [String], rng: &mut Rng) {
    if items.len() < 2 {
        return;
    }
    for index in (1..items.len()).rev() {
        let swap = rng.index(index + 1);
        items.swap(index, swap);
    }
}

/// Small deterministic generator. Not for anything cryptographic.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // A zero state would stick at zero. Force a nonzero odd state.
        Self(
            seed.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(0xA24B_1C6D)
                | 1,
        )
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn index(&mut self, n: usize) -> usize {
        (self.next_u64() % u64::try_from(n).unwrap_or(1)) as usize
    }
}

/// Decides the stream. The Godot node only loads, fades, and reports finish.
#[derive(Clone, Debug)]
pub struct Director {
    scene: MusicScene,
    mood: CourseMood,
    ducked: bool,
    volume: u8,
    bundled: Library,
    user: Vec<TrackInfo>,
    rotation: Rotation,
    current_id: Option<String>,
    current_name: Option<String>,
    current_scene: Option<MusicScene>,
    failed: Vec<String>,
}

impl Director {
    /// Start silent, on the title, at [`DEFAULT_VOLUME_PERCENT`].
    #[must_use]
    pub fn new(bundled: Library, seed: u64) -> Self {
        Self {
            scene: MusicScene::Title,
            mood: CourseMood::default(),
            ducked: false,
            volume: DEFAULT_VOLUME_PERCENT,
            bundled,
            user: Vec::new(),
            rotation: Rotation::new(seed),
            current_id: None,
            current_name: None,
            current_scene: None,
            failed: Vec::new(),
        }
    }

    /// Replace the bundled library without clearing a folder the rider already set.
    pub fn install_bundled(&mut self, bundled: Library, seed: u64) {
        self.bundled = bundled;
        self.rotation = Rotation::new(seed);
    }

    /// Scene change. The same scene twice is [`Decision::Hold`], so a pause
    /// menu that sets `Ride` again does not restart the track.
    pub fn set_scene(&mut self, scene: MusicScene) -> Decision {
        let same =
            self.scene == scene && self.current_scene == Some(scene) && self.current_id.is_some();
        self.scene = scene;
        if same {
            return Decision::Hold;
        }
        self.failed.clear();
        let fade = if self.current_id.is_some() {
            SCENE_CROSSFADE_SECS
        } else {
            0.0
        };
        match scene {
            MusicScene::Title => self.play_title(fade),
            MusicScene::Ride => self.play_ride(fade),
        }
    }

    /// Course mood. On the ride, crossfades when the current track is not in
    /// the new playlist. A custom folder ignores mood filtering.
    pub fn set_mood(&mut self, mood: CourseMood) -> Decision {
        if self.mood == mood {
            return Decision::Hold;
        }
        self.mood = mood;
        self.failed.clear();
        if self.scene != MusicScene::Ride || self.current_id.is_none() {
            return Decision::Hold;
        }
        if !self.user.is_empty() {
            return Decision::Hold;
        }
        let ride_ids = self.ride_ids();
        if self
            .current_id
            .as_ref()
            .is_some_and(|id| ride_ids.iter().any(|candidate| candidate == id))
        {
            return Decision::Hold;
        }
        self.play_ride(SCENE_CROSSFADE_SECS)
    }

    /// Pause menu. Does not change the track.
    pub fn set_ducked(&mut self, ducked: bool) {
        self.ducked = ducked;
    }

    /// `percent` is clamped to 0–100.
    pub fn set_volume(&mut self, percent: u8) {
        self.volume = percent.min(100);
    }

    /// Ride tracks from the rider's folder. An empty list uses the bundled rides.
    ///
    /// The title track is unchanged. On the ride, a list that no longer
    /// contains the current file crossfades to the new rotation.
    pub fn set_user_tracks(&mut self, tracks: Vec<TrackInfo>) -> Decision {
        let same = ids_of(&self.user) == ids_of(&tracks);
        self.user = tracks;
        if same {
            return Decision::Hold;
        }
        self.failed.clear();
        if self.scene != MusicScene::Ride || self.current_id.is_none() {
            return Decision::Hold;
        }
        let ride_ids = self.ride_ids();
        if self
            .current_id
            .as_ref()
            .is_some_and(|id| ride_ids.iter().any(|candidate| candidate == id))
        {
            return Decision::Hold;
        }
        self.play_ride(SCENE_CROSSFADE_SECS)
    }

    /// The current stream ended, or the ride reached the crossfade point.
    pub fn on_finished(&mut self) -> Decision {
        self.failed.clear();
        match self.scene {
            MusicScene::Title => self.play_title(0.0),
            MusicScene::Ride => self.play_ride(TRACK_CROSSFADE_SECS),
        }
    }

    /// Skip to the next ride track. On the title this is [`Decision::Hold`].
    pub fn next_track(&mut self) -> Decision {
        if self.scene != MusicScene::Ride {
            return Decision::Hold;
        }
        self.failed.clear();
        self.play_ride(TRACK_CROSSFADE_SECS)
    }

    /// The loader could not read `id`. Try another file, then stop.
    pub fn skip_failed(&mut self, id: &str) -> Decision {
        if !self.failed.iter().any(|failed| failed == id) {
            self.failed.push(id.to_owned());
        }
        if self.current_id.as_deref() == Some(id) {
            self.current_id = None;
            self.current_name = None;
            self.current_scene = None;
        }
        match self.scene {
            MusicScene::Title => self.play_title(0.0),
            MusicScene::Ride => self.play_ride(0.0),
        }
    }

    /// Display name of the current track.
    #[must_use]
    pub fn now_playing(&self) -> Option<&str> {
        self.current_name.as_deref()
    }

    /// How many tracks the ride rotation currently has.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.ride_tracks().len()
    }

    /// False at volume 0. The player pauses the streams so they stop decoding.
    #[must_use]
    pub fn audible(&self) -> bool {
        self.volume > 0
    }

    /// Bus decibels, or silence when the slider is at 0.
    #[must_use]
    pub fn bus_db(&self) -> Option<f32> {
        bus_db(self.volume, self.ducked)
    }

    /// Current slider position.
    #[must_use]
    pub fn volume_percent(&self) -> u8 {
        self.volume
    }

    fn play_title(&mut self, fade: f32) -> Decision {
        let Some(track) = self.bundled.title.clone() else {
            self.clear_current();
            return Decision::Stop;
        };
        if self.failed.iter().any(|id| id == &track.id) {
            self.clear_current();
            return Decision::Stop;
        }
        self.accept(&track);
        Decision::Play(PlayRequest {
            track,
            looping: true,
            fade_secs: fade,
        })
    }

    fn play_ride(&mut self, fade: f32) -> Decision {
        let count = self.ride_tracks().len();
        if count == 0 {
            self.clear_current();
            return Decision::Stop;
        }
        for _ in 0..count {
            let ids = self.ride_ids();
            let Some(id) = self.rotation.next(&ids) else {
                break;
            };
            if self.failed.iter().any(|failed| failed == &id) {
                continue;
            }
            let Some(track) = self.find_track(&id) else {
                continue;
            };
            self.accept(&track);
            return Decision::Play(PlayRequest {
                track,
                looping: false,
                fade_secs: fade,
            });
        }
        self.clear_current();
        Decision::Stop
    }

    fn accept(&mut self, track: &TrackInfo) {
        self.current_id = Some(track.id.clone());
        self.current_name = Some(track.display_name.clone());
        self.current_scene = Some(self.scene);
    }

    fn clear_current(&mut self) {
        self.current_id = None;
        self.current_name = None;
        self.current_scene = None;
    }

    fn ride_tracks(&self) -> Vec<TrackInfo> {
        if !self.user.is_empty() {
            return self.user.clone();
        }
        let filtered: Vec<TrackInfo> = self
            .bundled
            .rides
            .iter()
            .filter(|track| track.moods.is_empty() || track.moods.contains(&self.mood))
            .cloned()
            .collect();
        if filtered.is_empty() {
            self.bundled.rides.clone()
        } else {
            filtered
        }
    }

    fn ride_ids(&self) -> Vec<String> {
        self.ride_tracks()
            .iter()
            .map(|track| track.id.clone())
            .collect()
    }

    fn find_track(&self, id: &str) -> Option<TrackInfo> {
        self.ride_tracks()
            .into_iter()
            .find(|track| track.id == id)
            .or_else(|| {
                self.bundled
                    .rides
                    .iter()
                    .find(|track| track.id == id)
                    .cloned()
            })
            .or_else(|| self.user.iter().find(|track| track.id == id).cloned())
    }
}

fn ids_of(tracks: &[TrackInfo]) -> Vec<&str> {
    tracks.iter().map(|track| track.id.as_str()).collect()
}

/// One row of `music.json`.
#[derive(Debug, Deserialize)]
struct ManifestTrack {
    file: String,
    title: String,
    artist: String,
    role: String,
    duration_secs: f64,
    source_page: String,
    download_url: String,
    licence: String,
    retrieved: String,
    sha256: String,
    #[serde(default)]
    moods: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    tracks: Vec<ManifestTrack>,
}

/// Build the bundled library from `music.json`.
///
/// `path_prefix` is prepended to each file name (`res://assets/music/`).
/// The first `title` row is the menu track. Later title rows are ignored so
/// they are not shuffled into the ride. A licence that does not say CC0 is
/// rejected: bundled music stays public domain.
///
/// # Errors
///
/// Returns an error when the JSON is unreadable, a row is missing a field,
/// a role is unknown, or a licence is not CC0.
pub fn library_from_manifest(json: &str, path_prefix: &str) -> Result<Library, String> {
    let file: ManifestFile = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let mut library = Library::default();
    for track in file.tracks {
        if track.file.is_empty() || track.file.contains("..") || track.file.contains('/') {
            return Err(format!(
                "music file name '{}' is not a bare file",
                track.file
            ));
        }
        if !track.licence.to_ascii_uppercase().contains("CC0") {
            return Err(format!(
                "{} is not marked CC0 ({})",
                track.file, track.licence
            ));
        }
        // Fields the credits need. Reject a half-filled row at load time.
        if track.title.is_empty()
            || track.artist.is_empty()
            || track.source_page.is_empty()
            || track.download_url.is_empty()
            || track.retrieved.is_empty()
            || track.sha256.len() != 64
            || !track.duration_secs.is_finite()
            || track.duration_secs <= 0.0
        {
            return Err(format!("{} is missing credit fields", track.file));
        }
        let mut parsed_moods = Vec::new();
        for mood in &track.moods {
            let Some(parsed) = CourseMood::parse(mood) else {
                return Err(format!("{} has unknown mood '{mood}'", track.file));
            };
            parsed_moods.push(parsed);
        }
        let info = TrackInfo {
            id: format!("{path_prefix}{}", track.file),
            path: format!("{path_prefix}{}", track.file),
            display_name: track.title,
            moods: parsed_moods,
        };
        match track.role.to_ascii_lowercase().as_str() {
            "title" => {
                if library.title.is_none() {
                    library.title = Some(info.clone());
                }
                if !info.moods.is_empty() {
                    library.rides.push(info);
                }
            }
            "ride" => library.rides.push(info),
            other => return Err(format!("{} has unknown role '{other}'", track.file)),
        }
    }
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(tracks: &[&str]) -> Vec<String> {
        tracks.iter().map(|name| (*name).to_owned()).collect()
    }

    fn track(id: &str, display: &str) -> TrackInfo {
        TrackInfo {
            id: id.to_owned(),
            path: format!("res://assets/music/{id}"),
            display_name: display.to_owned(),
            moods: Vec::new(),
        }
    }

    fn tagged_track(id: &str, display: &str, moods: &[CourseMood]) -> TrackInfo {
        TrackInfo {
            id: id.to_owned(),
            path: format!("res://assets/music/{id}"),
            display_name: display.to_owned(),
            moods: moods.to_vec(),
        }
    }

    fn sample() -> Library {
        Library {
            title: Some(track("title.ogg", "Morning")),
            rides: vec![
                track("a.mp3", "Alpha"),
                track("b.mp3", "Beta"),
                track("c.mp3", "Gamma"),
            ],
        }
    }

    fn play(decision: Decision) -> PlayRequest {
        match decision {
            Decision::Play(request) => request,
            other => panic!("expected play, got {other:?}"),
        }
    }

    #[test]
    fn playable_names_are_sorted_and_unsupported_types_say_why() {
        let (playable, report) = inspect_names([
            "B.mp3",
            "notes.flac",
            "also.FLAC",
            "a.ogg",
            ".hidden.mp3",
            "voice.m4a",
            "readme",
            "c.wav",
            "cover.MP3",
        ]);
        assert_eq!(
            playable,
            vec![
                "a.ogg".to_owned(),
                "B.mp3".to_owned(),
                "c.wav".to_owned(),
                "cover.MP3".to_owned()
            ]
        );
        assert_eq!(report.found, 4);
        assert_eq!(
            report.summary(),
            "4 tracks found, 2 skipped (FLAC is not supported), 1 skipped (M4A is not supported), 1 skipped (file type is not supported)"
        );
        let (three, flac_only) = inspect_names([
            "ride.mp3",
            "menu.ogg",
            "extra.wav",
            "nope.flac",
            "also.flac",
        ]);
        assert_eq!(three, names(&["extra.wav", "menu.ogg", "ride.mp3"]));
        assert_eq!(
            flac_only.summary(),
            "3 tracks found, 2 skipped (FLAC is not supported)"
        );
    }

    #[test]
    fn file_display_names_drop_the_extension_and_hyphens() {
        assert_eq!(display_name_from_file("night-life.mp3"), "night life");
        assert_eq!(display_name_from_file("/tmp/My_Track.OGG"), "My Track");
        assert_eq!(display_name_from_file("plain"), "plain");
    }

    #[test]
    fn volume_is_silent_at_zero_unity_at_full_and_ducks() {
        assert_eq!(volume_db(0), None);
        assert_eq!(volume_db(100), Some(0.0));
        let at_60 = volume_db(DEFAULT_VOLUME_PERCENT).unwrap();
        assert!(at_60 < -5.0 && at_60 > -15.0, "{at_60}");
        let mut previous = f32::NEG_INFINITY;
        for percent in 1..=100 {
            let db = volume_db(percent).unwrap();
            assert!(db > previous);
            previous = db;
        }
        assert!((bus_db(60, true).unwrap() - (at_60 + DUCK_DB)).abs() < 0.001);
        assert_eq!(bus_db(0, true), None);
        assert_eq!(volume_db(200), Some(0.0));
    }

    #[test]
    fn rotation_covers_every_track_before_repeating_and_is_deterministic() {
        let ids = names(&["a", "b", "c", "d"]);
        let run = |seed| {
            let mut rotation = Rotation::new(seed);
            let mut heard = Vec::new();
            for _ in 0..12 {
                heard.push(rotation.next(&ids).unwrap());
            }
            heard
        };
        assert_eq!(run(9), run(9));
        let heard = run(9);
        for cycle in 0..3 {
            let slice = &heard[cycle * 4..cycle * 4 + 4];
            let mut sorted = slice.to_vec();
            sorted.sort();
            assert_eq!(sorted, names(&["a", "b", "c", "d"]));
            if cycle > 0 {
                assert_ne!(slice[0], heard[cycle * 4 - 1]);
            }
        }
    }

    #[test]
    fn rotation_handles_empty_singleton_and_a_changing_list() {
        let mut rotation = Rotation::new(1);
        assert_eq!(rotation.next(&[]), None);
        let only = names(&["solo"]);
        for _ in 0..4 {
            assert_eq!(rotation.next(&only).unwrap(), "solo");
        }
        let mut rotation = Rotation::new(3);
        let ids = names(&["a", "b", "c"]);
        let played = rotation.next(&ids).unwrap();
        // The queue still holds the other original ids. They are not in the new
        // list, so the cycle is dealt again and must not open on `played`.
        let fresh = rotation.next(&names(&["fresh", played.as_str()])).unwrap();
        assert_eq!(fresh, "fresh");
        let mut rotation = Rotation::new(5);
        let played = rotation.next(&ids).unwrap();
        assert_eq!(rotation.next(&[]), None);
        assert_ne!(rotation.next(&ids).unwrap(), played);
    }

    #[test]
    fn director_holds_when_the_scene_is_set_twice_and_rotates_the_ride() {
        let mut director = Director::new(sample(), 11);
        let title = play(director.set_scene(MusicScene::Title));
        assert_eq!(title.track.display_name, "Morning");
        assert!(title.looping);
        assert_eq!(title.fade_secs, 0.0);
        assert_eq!(director.set_scene(MusicScene::Title), Decision::Hold);
        assert_eq!(director.now_playing(), Some("Morning"));
        assert_eq!(director.next_track(), Decision::Hold);

        let first = play(director.set_scene(MusicScene::Ride));
        assert!(!first.looping);
        assert_eq!(first.fade_secs, SCENE_CROSSFADE_SECS);
        assert_eq!(director.set_scene(MusicScene::Ride), Decision::Hold);
        let mut heard = vec![first.track.display_name];
        for _ in 0..2 {
            heard.push(play(director.on_finished()).track.display_name);
        }
        let mut sorted = heard.clone();
        sorted.sort();
        assert_eq!(sorted, names(&["Alpha", "Beta", "Gamma"]));
        let next_cycle = play(director.on_finished());
        assert_ne!(next_cycle.track.display_name, *heard.last().unwrap());
        assert_eq!(next_cycle.fade_secs, TRACK_CROSSFADE_SECS);
    }

    #[test]
    fn user_folder_replaces_rides_only_and_an_empty_folder_falls_back() {
        let mut director = Director::new(sample(), 4);
        let _title = director.set_scene(MusicScene::Title);
        let report_tracks = vec![track("/tmp/ride.mp3", "ride")];
        assert_eq!(
            director.set_user_tracks(report_tracks.clone()),
            Decision::Hold
        );
        assert_eq!(director.now_playing(), Some("Morning"));
        assert_eq!(director.track_count(), 1);
        let ride = play(director.set_scene(MusicScene::Ride));
        assert_eq!(ride.track.display_name, "ride");
        assert_eq!(director.set_user_tracks(report_tracks), Decision::Hold);
        let back = play(director.set_user_tracks(Vec::new()));
        assert!(["Alpha", "Beta", "Gamma"].contains(&back.track.display_name.as_str()));
        assert_eq!(director.track_count(), 3);
        let title = play(director.set_scene(MusicScene::Title));
        assert_eq!(title.track.display_name, "Morning");
    }

    #[test]
    fn a_missing_file_is_skipped_and_an_empty_ride_is_silent() {
        let mut director = Director::new(sample(), 8);
        let first = play(director.set_scene(MusicScene::Ride));
        let second = play(director.skip_failed(&first.track.id));
        assert_ne!(second.track.id, first.track.id);
        let mut guard = 0;
        let mut decision = director.skip_failed(&second.track.id);
        while matches!(decision, Decision::Play(_)) && guard < 6 {
            let request = play(decision);
            decision = director.skip_failed(&request.track.id);
            guard += 1;
        }
        assert_eq!(decision, Decision::Stop);
        assert_eq!(director.now_playing(), None);
        let mut silent = Director::new(Library::default(), 1);
        assert_eq!(silent.set_scene(MusicScene::Title), Decision::Stop);
        assert_eq!(silent.set_scene(MusicScene::Ride), Decision::Stop);
        silent.set_volume(0);
        silent.set_ducked(true);
        assert!(!silent.audible());
        assert_eq!(silent.bus_db(), None);
        assert_eq!(silent.now_playing(), None);
    }

    #[test]
    fn manifest_keeps_one_title_and_rejects_a_non_cc0_row() {
        let json = r#"{
            "tracks": [
                {"file":"menu.ogg","title":"Menu","artist":"A","role":"title","duration_secs":80.0,"source_page":"https://example.invalid/a","download_url":"https://example.invalid/a.ogg","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                {"file":"extra.ogg","title":"Extra","artist":"A","role":"title","duration_secs":70.0,"source_page":"https://example.invalid/e","download_url":"https://example.invalid/e.ogg","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
                {"file":"ride.mp3","title":"Ride","artist":"B","role":"ride","duration_secs":120.0,"source_page":"https://example.invalid/b","download_url":"https://example.invalid/b.mp3","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}
            ]
        }"#;
        let library = library_from_manifest(json, "res://assets/music/").unwrap();
        assert_eq!(library.title.unwrap().display_name, "Menu");
        assert_eq!(library.rides.len(), 1);
        assert_eq!(library.rides[0].path, "res://assets/music/ride.mp3");
        let bad = json.replace("CC0 1.0", "CC-BY 4.0");
        assert!(library_from_manifest(&bad, "res://assets/music/").is_err());
    }

    #[test]
    fn manifest_moods_filter_solitary_playlist() {
        let json = r#"{
            "tracks": [
                {"file":"zen-theme.ogg","title":"Zen","artist":"A","role":"title","duration_secs":80.0,"source_page":"https://example.invalid/a","download_url":"https://example.invalid/a.ogg","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","moods":["solitary"]},
                {"file":"bathing-in-the-sun.mp3","title":"Bath","artist":"B","role":"ride","duration_secs":120.0,"source_page":"https://example.invalid/b","download_url":"https://example.invalid/b.mp3","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","moods":["solitary","cruising","energetic"]},
                {"file":"forever.mp3","title":"Forever","artist":"B","role":"ride","duration_secs":110.0,"source_page":"https://example.invalid/c","download_url":"https://example.invalid/c.mp3","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","moods":["solitary","cruising","energetic"]},
                {"file":"cyber-march.mp3","title":"Cyber","artist":"C","role":"ride","duration_secs":90.0,"source_page":"https://example.invalid/d","download_url":"https://example.invalid/d.mp3","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","moods":["energetic"]},
                {"file":"intense-fight.mp3","title":"Fight","artist":"C","role":"ride","duration_secs":100.0,"source_page":"https://example.invalid/e","download_url":"https://example.invalid/e.mp3","licence":"CC0 1.0","retrieved":"2026-11-20","sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","moods":["energetic"]}
            ]
        }"#;
        let library = library_from_manifest(json, "res://assets/music/").unwrap();
        assert_eq!(library.title.as_ref().unwrap().display_name, "Zen");
        assert_eq!(library.rides.len(), 5);
        let mut director = Director::new(library, 3);
        director.set_mood(CourseMood::Solitary);
        assert_eq!(director.track_count(), 3);
        let first = play(director.set_scene(MusicScene::Ride));
        let id = first.track.id;
        assert!(
            ["zen-theme.ogg", "bathing-in-the-sun.mp3", "forever.mp3"]
                .iter()
                .any(|name| id.ends_with(name)),
            "{id}"
        );
        assert_eq!(director.set_mood(CourseMood::Solitary), Decision::Hold);
        assert!(
            library_from_manifest(
                &json.replace("\"solitary\"", "\"moody\""),
                "res://assets/music/"
            )
            .is_err()
        );
    }

    #[test]
    fn set_mood_crossfades_or_holds_on_the_ride() {
        let library = Library {
            title: Some(track("title.ogg", "Morning")),
            rides: vec![
                tagged_track("warm.mp3", "Warm", &[CourseMood::Solitary]),
                tagged_track("loud.mp3", "Loud", &[CourseMood::Energetic]),
            ],
        };
        let mut director = Director::new(library, 9);
        director.set_mood(CourseMood::Solitary);
        let first = play(director.set_scene(MusicScene::Ride));
        assert!(first.track.id.ends_with("warm.mp3"));
        assert_eq!(director.set_mood(CourseMood::Solitary), Decision::Hold);
        let crossfade = play(director.set_mood(CourseMood::Energetic));
        assert_ne!(crossfade.track.id, first.track.id);
        assert_eq!(crossfade.fade_secs, SCENE_CROSSFADE_SECS);
        assert!(crossfade.track.id.ends_with("loud.mp3"));
        assert_eq!(director.set_mood(CourseMood::Energetic), Decision::Hold);
    }

    #[test]
    fn user_folder_ignores_mood_filtering() {
        let library = Library {
            title: None,
            rides: vec![
                tagged_track("a.mp3", "A", &[CourseMood::Energetic]),
                tagged_track("b.mp3", "B", &[CourseMood::Solitary]),
            ],
        };
        let mut director = Director::new(library, 2);
        director.set_mood(CourseMood::Solitary);
        let user = vec![track("/tmp/custom.mp3", "custom")];
        director.set_user_tracks(user);
        assert_eq!(director.track_count(), 1);
        let ride = play(director.set_scene(MusicScene::Ride));
        assert_eq!(ride.track.display_name, "custom");
        assert_eq!(director.set_mood(CourseMood::Energetic), Decision::Hold);
    }

    #[test]
    fn bundled_music_json_parses() {
        let json = include_str!("../../../godot/assets/music/music.json");
        let library = library_from_manifest(json, "res://assets/music/").unwrap();
        assert!(library.title.is_some());
        assert_eq!(library.rides.len(), 7);
        let mut director = Director::new(library.clone(), 1);
        director.set_mood(CourseMood::Solitary);
        assert_eq!(director.track_count(), 3);
        director.set_mood(CourseMood::Energetic);
        assert_eq!(director.track_count(), 6);
        director.set_mood(CourseMood::Cruising);
        assert_eq!(director.track_count(), 4);
    }
}
