//! Settings menu as a state machine.
//!
//! Focus stops at the ends (it does not wrap): the smoke test walks three
//! steps from the first trainer to "Disconnect". The Godot side only draws
//! [`MenuView`] and performs [`Action`].

use pp_session::{
    DashboardRun, MAX_BIKE_MASS_KG, MAX_FTP_W, MAX_RIDER_MASS_KG, MAX_TRAINER_DIFFICULTY,
    MIN_BIKE_MASS_KG, MIN_FTP_W, MIN_RIDER_MASS_KG, MIN_TRAINER_DIFFICULTY, Settings,
};

const MAIN_ROWS: usize = 26;

/// What the player just pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Move up.
    Up,
    /// Move down.
    Down,
    /// Decrease the selected value.
    Left,
    /// Increase the selected value.
    Right,
    /// Activate the selected row.
    Accept,
    /// Close this page, or the whole menu.
    Back,
}

/// What the game should do after a command.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Action {
    /// Stay as we are.
    None,
    /// The player confirmed quit.
    Quit,
    /// The ride was restarted.
    Restart,
    /// Replace the session settings and save them.
    Apply(Settings),
    /// Open a scan.
    Scan,
    /// Connect this id.
    Connect(String),
    /// Drop the trainer and save.
    Disconnect,
    /// Open the folder picker.
    ChooseMusicFolder,
    /// Show the rides folder in the file manager.
    OpenRides,
    /// Open the heart-rate list.
    OpenHeart,
    /// Connect the strap with this id.
    ConnectHeart(String),
    /// Forget the remembered strap.
    ForgetHeart,
    /// Connect, cancel, or disconnect Strava.
    Strava,
}

/// Which page is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The settings list.
    Main,
    /// Discovered trainers.
    Pairing,
    /// Restart confirmation.
    ConfirmRestart,
    /// Quit confirmation.
    ConfirmQuit,
    /// Heart-rate straps.
    Heart,
}

/// One discovered trainer, already ordered or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Address or fake id.
    pub id: String,
    /// Name shown to the player.
    pub name: String,
    /// Signal strength. Missing sorts last.
    pub rssi_dbm: Option<i16>,
    /// Power meter only: no FTMS and no FE-C, so resistance cannot be set.
    pub power_only: bool,
    /// Signal bars, 0 to 4.
    pub bars: u8,
    /// Fitness Machine Service.
    pub ftms: bool,
    /// Cycling Power Service.
    pub cycling_power: bool,
    /// Tacx FE-C.
    pub fec: bool,
    /// This is the trainer saved in settings.
    pub remembered: bool,
}

/// What the menu needs to read. It does not touch the session.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Current settings.
    pub settings: Settings,
    /// `SessionStatus::line`.
    pub status_line: String,
    /// Dashboard phase.
    pub dashboard: DashboardRun,
    /// First URL is the one shown.
    pub dashboard_urls: Vec<String>,
    /// Bind error, if the page failed.
    pub dashboard_error: Option<String>,
    /// Trainers from the last scan, in any order.
    pub devices: Vec<Device>,
    /// Last folder scan, for the music row. Empty when the row uses built-in tracks.
    pub music_report: String,
    /// Link phase for the pairing headline.
    pub phase: pp_session::LinkPhase,
    /// Plain sentence when the link failed.
    pub failure_reason: String,
    /// Trainer name while connecting or connected.
    pub connected_name: String,
    /// Short Zwift-controller status, empty when the row is off.
    pub controller_status: String,
    /// Heart-rate straps from the strap link.
    pub straps: Vec<Device>,
    /// One line for the heart-rate list: scanning, connected, failed.
    pub heart_line: String,
    /// Rider presets, id then display name, in menu order.
    pub riders: Vec<RiderOption>,
    /// `default` plus the kits this profile has unlocked.
    pub kits: Vec<RiderOption>,
    /// What the Strava row should say.
    pub strava: crate::extras::StravaRow,
}

/// One rider the settings row can show. The id is what gets saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiderOption {
    /// Settings id, such as `nils`.
    pub id: String,
    /// Name drawn in the row, such as `Nils`.
    pub name: String,
}

/// Strings for one frame of the overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuView {
    /// Page title.
    pub title: String,
    /// Row text, without the selection marker.
    pub lines: Vec<String>,
    /// Explanation under the rows.
    pub detail: String,
    /// Highlighted row.
    pub selected: usize,
    /// Signal bars for each row. `None` hides the bars (settings and the disconnect row).
    pub bars: Vec<Option<u8>>,
    /// The detail line is a failure, drawn in the warning colour.
    pub warn: bool,
}

/// The overlay's memory.
#[derive(Clone, Debug)]
pub struct MenuState {
    /// Whether the overlay is showing.
    pub open: bool,
    page: Page,
    index: usize,
    selected_id: String,
    /// Shown when a save fails. The game sets this.
    pub save_error: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            open: false,
            page: Page::Main,
            index: 0,
            selected_id: String::new(),
            save_error: String::new(),
        }
    }
}

impl MenuState {
    /// Open the pairing page and clear the remembered row.
    pub fn open_pairing(&mut self) {
        self.open = true;
        self.page = Page::Pairing;
        self.index = 0;
        self.selected_id.clear();
    }

    /// Show the main page.
    pub fn open_main(&mut self) {
        self.open = true;
        self.page = Page::Main;
        self.index = 0;
    }

    /// Open the heart-rate list.
    pub fn open_heart(&mut self) {
        self.open = true;
        self.page = Page::Heart;
        self.index = 0;
        self.selected_id.clear();
    }

    /// Move the highlight. A mouse hover uses this.
    pub fn hover(&mut self, index: usize) {
        if self.open {
            self.index = index;
        }
    }

    /// Hide the overlay.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Current page.
    pub fn page(&self) -> Page {
        self.page
    }

    /// Apply one command. The caller performs the [`Action`] on the session.
    pub fn handle(&mut self, command: Command, snap: &Snapshot) -> Action {
        if !self.open {
            return Action::None;
        }
        let action = match self.page {
            Page::Main => self.main_command(command, snap),
            Page::Pairing => self.pairing_command(command, snap),
            Page::Heart => self.heart_command(command, snap),
            Page::ConfirmRestart => self.confirm(command, true),
            Page::ConfirmQuit => self.confirm(command, false),
        };
        self.sync_pairing_cursor(snap);
        action
    }

    /// Text for the current page. Pairing may move the cursor to follow a device.
    pub fn view(&mut self, snap: &Snapshot) -> MenuView {
        match self.page {
            Page::Main => self.main_view(snap),
            Page::Pairing => self.pairing_view(snap),
            Page::Heart => self.heart_view(snap),
            Page::ConfirmRestart => confirm_view(
                "Restart the ride?",
                "Distance and time go back to zero.",
                self.index,
            ),
            Page::ConfirmQuit => confirm_view(
                "Quit Penguin Pass?",
                "The ride stops. Settings are already saved.",
                self.index,
            ),
        }
    }

    fn main_command(&mut self, command: Command, snap: &Snapshot) -> Action {
        match command {
            Command::Up => self.index = self.index.saturating_sub(1),
            Command::Down => self.index = (self.index + 1).min(MAIN_ROWS - 1),
            Command::Back => self.close(),
            Command::Accept => match self.index {
                0 => {
                    self.page = Page::Pairing;
                    self.index = 0;
                    self.selected_id.clear();
                    return Action::Scan;
                }
                15 => return Action::ChooseMusicFolder,
                16 => {
                    self.page = Page::ConfirmRestart;
                    self.index = 0;
                }
                17 => {
                    self.page = Page::ConfirmQuit;
                    self.index = 0;
                }
                18 => return Action::OpenRides,
                19 | 20 | 25 => return Action::Apply(toggled(snap.settings.clone(), self.index)),
                24 => return Action::Strava,
                1 | 8 | 9 | 10 | 14 => {
                    return Action::Apply(toggled(snap.settings.clone(), self.index));
                }
                _ => {}
            },
            Command::Left | Command::Right => {
                if self.index == 15 && matches!(command, Command::Right) {
                    return Action::ChooseMusicFolder;
                }
                let increase = matches!(command, Command::Right);
                if let Some(settings) = stepped(
                    snap.settings.clone(),
                    self.index,
                    increase,
                    &snap.riders,
                    &snap.kits,
                ) {
                    return Action::Apply(settings);
                }
            }
        }
        Action::None
    }

    fn pairing_command(&mut self, command: Command, snap: &Snapshot) -> Action {
        let devices = rank(&snap.devices);
        let count = devices.len() + 1;
        let action = match command {
            Command::Up => {
                self.index = self.index.saturating_sub(1);
                Action::None
            }
            Command::Down => {
                self.index = (self.index + 1).min(count.saturating_sub(1));
                Action::None
            }
            Command::Back => {
                self.page = Page::Main;
                self.index = 0;
                Action::None
            }
            Command::Accept => {
                if self.index >= devices.len() {
                    Action::Disconnect
                } else {
                    self.selected_id = devices[self.index].id.clone();
                    Action::Connect(devices[self.index].id.clone())
                }
            }
            Command::Left | Command::Right => Action::None,
        };
        if let Some(device) = devices.get(self.index) {
            self.selected_id = device.id.clone();
        } else {
            self.selected_id.clear();
        }
        action
    }

    fn heart_command(&mut self, command: Command, snap: &Snapshot) -> Action {
        let count = snap.straps.len() + 1;
        match command {
            Command::Up => self.index = self.index.saturating_sub(1),
            Command::Down => self.index = (self.index + 1).min(count.saturating_sub(1)),
            Command::Back => {
                self.page = Page::Main;
                self.index = 0;
            }
            Command::Accept => {
                if self.index >= snap.straps.len() {
                    return Action::ForgetHeart;
                }
                return Action::ConnectHeart(snap.straps[self.index].id.clone());
            }
            Command::Left | Command::Right => {}
        }
        Action::None
    }

    fn heart_view(&mut self, snap: &Snapshot) -> MenuView {
        let mut lines = Vec::new();
        let mut bars = Vec::new();
        for strap in &snap.straps {
            let saved = if strap.remembered { "  saved" } else { "" };
            lines.push(format!("{}{saved}", strap.name));
            bars.push(Some(strap.bars));
        }
        lines.push("Forget this strap".to_owned());
        bars.push(None);
        let index = self.index.min(lines.len().saturating_sub(1));
        self.index = index;
        MenuView {
            title: "Heart rate".to_owned(),
            lines,
            detail: snap.heart_line.clone(),
            selected: index,
            bars,
            warn: snap.heart_line.to_ascii_lowercase().contains("fail"),
        }
    }

    fn confirm(&mut self, command: Command, restart: bool) -> Action {
        match command {
            Command::Up | Command::Down | Command::Left | Command::Right => {
                self.index = if self.index == 0 { 1 } else { 0 };
            }
            Command::Back => {
                self.page = Page::Main;
                self.index = 0;
            }
            Command::Accept => {
                let yes = self.index == 0;
                self.page = Page::Main;
                self.index = 0;
                if yes {
                    return if restart {
                        Action::Restart
                    } else {
                        Action::Quit
                    };
                }
            }
        }
        Action::None
    }

    fn sync_pairing_cursor(&mut self, snap: &Snapshot) {
        if self.page != Page::Pairing {
            return;
        }
        let devices = rank(&snap.devices);
        if self.index < devices.len()
            && let Some(position) = devices
                .iter()
                .position(|device| device.id == self.selected_id)
        {
            self.index = position;
        }
    }

    fn main_view(&self, snap: &Snapshot) -> MenuView {
        let settings = &snap.settings;
        let lines = vec![
            format!("Trainer    {}", snap.status_line),
            format!(
                "Resistance follows the road    {}",
                on_off(settings.control_trainer)
            ),
            format!(
                "Trainer difficulty    {}%",
                (settings.trainer_difficulty * 100.0).round() as i32
            ),
            format!("Rider weight    {:.1} kg", settings.rider_mass_kg),
            format!("Bike weight    {:.1} kg", settings.bike_mass_kg),
            format!("FTP    {} W", settings.ftp_w),
            format!("Rider    ◀ {} ▶", rider_name(snap)),
            format!("Kit    ◀ {} ▶", kit_name(snap)),
            format!(
                "Zwift controllers    {}{}",
                on_off(settings.zwift_controllers),
                zwift_note(snap)
            ),
            format!("Companion dashboard    {}", dashboard_value(snap)),
            format!("Full screen    {}", on_off(settings.fullscreen)),
            format!("Music volume    {}%", settings.music_volume),
            format!("Ambient sound    {}%", settings.ambient_volume),
            format!("Sound effects    {}%", settings.effects_volume),
            format!("Rider breathing    {}", on_off(settings.rider_breathing)),
            format!("Music folder    {}", music_folder_line(snap)),
            "Restart ride".to_owned(),
            "Quit".to_owned(),
            "Open rides folder".to_owned(),
            format!("Fish and powerups    {}", on_off(settings.arcade)),
            format!("Ghost    {}", on_off(settings.ghost)),
            format!(
                "Time of day    ◀ {} ▶",
                crate::extras::time_label(&settings.time_of_day)
            ),
            format!(
                "Weather    ◀ {} ▶",
                crate::extras::weather_label(&settings.weather)
            ),
            format!(
                "Other riders    ◀ {} ▶",
                crate::extras::bots_label(settings.bots_count)
            ),
            crate::extras::strava_line(&strava_row(snap)),
            format!(
                "Strava auto-upload    {}",
                on_off(settings.strava_auto_upload)
            ),
        ];
        let detail = if !self.save_error.is_empty() {
            self.save_error.clone()
        } else {
            match self.index {
                2 => "How much of the hill the trainer makes you feel. On-screen speed always uses the real hill.".to_owned(),
                6 => "Left and right change the rider.".to_owned(),
                7 => "Colours on that rider. Locked kits stay off this list.".to_owned(),
                8 => "Look for Zwift Click, Play, and Ride controllers.".to_owned(),
                9 => dashboard_detail(snap),
                0 => "Open this to scan and connect. The ride keeps going.".to_owned(),
                11 => "How loud the music is. Left and right change it immediately.".to_owned(),
                12 => "Birds, wind, and the odd bell or stream. Left and right change it immediately.".to_owned(),
                13 => "The bike, the rush of air, and the ride's cues.".to_owned(),
                14 => "A quiet breath that follows how hard you are working.".to_owned(),
                15 => music_folder_detail(snap),
                18 => "Show the folder of saved rides in the file manager.".to_owned(),
                19 => "Fish to steer for, and the powerups they award.".to_owned(),
                20 => "A pale rider on your best first lap.".to_owned(),
                21 => "Day, a clock that moves with the ride, dawn, sunset, or night.".to_owned(),
                22 => "Clear, a changing sky, cloud, rain, snow, or fog.".to_owned(),
                23 => "Computer riders on the road. Off, or a small group.".to_owned(),
                24 => "Open Strava in the browser, or disconnect.".to_owned(),
                25 => "Send a saved ride when Strava is connected.".to_owned(),
                _ => String::new(),
            }
        };
        MenuView {
            title: "Settings".to_owned(),
            selected: self.index.min(lines.len().saturating_sub(1)),
            lines,
            detail,
            bars: Vec::new(),
            warn: false,
        }
    }

    fn pairing_view(&mut self, snap: &Snapshot) -> MenuView {
        let devices = rank(&snap.devices);
        if self.index < devices.len()
            && let Some(position) = devices
                .iter()
                .position(|device| device.id == self.selected_id)
        {
            self.index = position;
        }
        let mut lines = Vec::new();
        let mut bars = Vec::new();
        for device in &devices {
            let mut chips = String::new();
            if device.remembered {
                chips.push_str("  saved");
            }
            if device.ftms {
                chips.push_str("  FTMS");
            }
            if device.cycling_power {
                chips.push_str("  Power");
            }
            if device.fec {
                chips.push_str("  FE-C");
            }
            if device.power_only {
                chips.push_str("  power only, no resistance");
            }
            if snap.phase == pp_session::LinkPhase::Connecting && snap.connected_name == device.name
            {
                chips.push_str("  connecting");
            }
            if snap.phase == pp_session::LinkPhase::Connected && snap.connected_name == device.name
            {
                chips.push_str("  connected");
            }
            lines.push(format!(
                "{}    {}{chips}",
                device.name,
                signal(device.rssi_dbm)
            ));
            bars.push(Some(device.bars));
        }
        lines.push("Disconnect and use the simulated rider".to_owned());
        bars.push(None);
        let index = self.index.min(lines.len().saturating_sub(1));
        self.index = index;
        let hint = if snap.status_line.contains("Bluetooth unavailable") {
            "Bluetooth is unavailable on this computer. The simulated rider keeps going. Up and Down still change power.".to_owned()
        } else if devices.is_empty() && snap.status_line.contains("Scanning") {
            "Looking for trainers. Wake yours by pedalling. Close other apps or bike computers that may already be connected to it.".to_owned()
        } else {
            snap.status_line.clone()
        };
        let failed = snap.phase == pp_session::LinkPhase::Failed && !snap.failure_reason.is_empty();
        let hint = if failed {
            snap.failure_reason.clone()
        } else {
            hint
        };
        let detail = if self.save_error.is_empty() {
            hint
        } else {
            format!("{hint} {}", self.save_error)
        };
        MenuView {
            title: "Trainers".to_owned(),
            lines,
            detail,
            selected: index,
            bars,
            warn: failed,
        }
    }
}

/// Strongest signal first. Equal signals stay in name, then id, order.
pub fn rank(devices: &[Device]) -> Vec<Device> {
    let mut devices = devices.to_vec();
    devices.sort_by(|left, right| {
        right
            .rssi_dbm
            .unwrap_or(i16::MIN)
            .cmp(&left.rssi_dbm.unwrap_or(i16::MIN))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.id.cmp(&right.id))
    });
    devices
}

fn toggled(mut settings: Settings, index: usize) -> Settings {
    match index {
        1 => settings.control_trainer = !settings.control_trainer,
        14 => settings.rider_breathing = !settings.rider_breathing,
        19 => settings.arcade = !settings.arcade,
        20 => settings.ghost = !settings.ghost,
        25 => settings.strava_auto_upload = !settings.strava_auto_upload,
        8 => settings.zwift_controllers = !settings.zwift_controllers,
        9 => settings.dashboard_enabled = !settings.dashboard_enabled,
        10 => settings.fullscreen = !settings.fullscreen,
        _ => {}
    }
    settings
}

fn strava_row(snap: &Snapshot) -> crate::extras::StravaRow {
    snap.strava.clone()
}

fn kit_name(snap: &Snapshot) -> String {
    snap.kits
        .iter()
        .find(|kit| kit.id == snap.settings.kit)
        .map(|kit| kit.name.clone())
        .unwrap_or_else(|| "Default".to_owned())
}

fn rider_name(snap: &Snapshot) -> String {
    snap.riders
        .iter()
        .find(|rider| rider.id == snap.settings.rider)
        .map(|rider| rider.name.clone())
        .unwrap_or_else(|| snap.settings.rider.clone())
}

fn stepped(
    mut settings: Settings,
    index: usize,
    increase: bool,
    riders: &[RiderOption],
    kits: &[RiderOption],
) -> Option<Settings> {
    match index {
        1 | 8 | 9 | 10 | 14 | 19 | 20 | 25 => Some(toggled(settings, index)),
        2 => {
            settings.trainer_difficulty = step_f32(
                settings.trainer_difficulty,
                0.05,
                increase,
                MIN_TRAINER_DIFFICULTY,
                MAX_TRAINER_DIFFICULTY,
            );
            Some(settings)
        }
        3 => {
            settings.rider_mass_kg = step_f32(
                settings.rider_mass_kg,
                0.5,
                increase,
                MIN_RIDER_MASS_KG,
                MAX_RIDER_MASS_KG,
            );
            Some(settings)
        }
        4 => {
            settings.bike_mass_kg = step_f32(
                settings.bike_mass_kg,
                0.5,
                increase,
                MIN_BIKE_MASS_KG,
                MAX_BIKE_MASS_KG,
            );
            Some(settings)
        }
        5 => {
            let next = i32::from(settings.ftp_w) + if increase { 5 } else { -5 };
            settings.ftp_w = u16::try_from(next.clamp(i32::from(MIN_FTP_W), i32::from(MAX_FTP_W)))
                .unwrap_or(MIN_FTP_W);
            Some(settings)
        }
        6 => {
            if riders.is_empty() {
                return None;
            }
            let pos = riders
                .iter()
                .position(|rider| rider.id == settings.rider)
                .unwrap_or(0);
            let next = if increase {
                (pos + 1) % riders.len()
            } else {
                (pos + riders.len() - 1) % riders.len()
            };
            settings.rider = riders[next].id.clone();
            Some(settings)
        }
        7 => {
            if kits.is_empty() {
                return None;
            }
            let pos = kits
                .iter()
                .position(|kit| kit.id == settings.kit)
                .unwrap_or(0);
            let next = if increase {
                (pos + 1) % kits.len()
            } else {
                (pos + kits.len() - 1) % kits.len()
            };
            settings.kit = kits[next].id.clone();
            Some(settings)
        }
        21 => {
            settings.time_of_day =
                crate::extras::cycle_time(&settings.time_of_day, increase).to_owned();
            Some(settings)
        }
        22 => {
            settings.weather = crate::extras::cycle_weather(&settings.weather, increase).to_owned();
            Some(settings)
        }
        23 => {
            settings.bots_count = crate::extras::cycle_bots(settings.bots_count, increase);
            Some(settings)
        }
        11 => {
            settings.music_volume = step_volume(settings.music_volume, increase);
            Some(settings)
        }
        12 => {
            settings.ambient_volume = step_volume(settings.ambient_volume, increase);
            Some(settings)
        }
        13 => {
            settings.effects_volume = step_volume(settings.effects_volume, increase);
            Some(settings)
        }
        15 => {
            if !increase {
                settings.music_folder = None;
                Some(settings)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn step_volume(value: u8, increase: bool) -> u8 {
    let next = i32::from(value) + if increase { 5 } else { -5 };
    u8::try_from(next.clamp(0, 100)).unwrap_or(0)
}

fn step_f32(value: f32, step: f32, increase: bool, min: f32, max: f32) -> f32 {
    let next = if increase { value + step } else { value - step };
    (next.clamp(min, max) * 100.0).round() / 100.0
}

fn zwift_note(snap: &Snapshot) -> String {
    if !snap.settings.zwift_controllers || snap.controller_status.is_empty() {
        return String::new();
    }
    format!("  {}", snap.controller_status)
}

fn music_folder_line(snap: &Snapshot) -> String {
    match snap.settings.music_folder.as_ref() {
        Some(path) => path.display().to_string(),
        None => "Built-in tracks".to_owned(),
    }
}

fn music_folder_detail(snap: &Snapshot) -> String {
    let mut text =
        "mp3, ogg and wav files in this folder replace the ride music. Other files are skipped."
            .to_owned();
    if snap.settings.missing_music_folder().is_some() {
        text.push_str(" That folder is not on this computer.");
    }
    if !snap.music_report.is_empty() {
        text.push(' ');
        text.push_str(&snap.music_report);
    }
    text
}

fn on_off(value: bool) -> &'static str {
    if value { "On" } else { "Off" }
}

fn signal(rssi_dbm: Option<i16>) -> &'static str {
    match rssi_dbm {
        Some(rssi) if rssi >= -60 => "strong",
        Some(rssi) if rssi >= -75 => "ok",
        Some(_) => "weak",
        None => "no signal",
    }
}

fn dashboard_value(snap: &Snapshot) -> String {
    match snap.dashboard {
        DashboardRun::Off => "Off".to_owned(),
        DashboardRun::Starting => "Starting".to_owned(),
        DashboardRun::Running => snap
            .dashboard_urls
            .first()
            .cloned()
            .unwrap_or_else(|| "On".to_owned()),
        DashboardRun::Failed => "Could not start".to_owned(),
    }
}

fn dashboard_detail(snap: &Snapshot) -> String {
    if let Some(error) = &snap.dashboard_error {
        return format!("The companion dashboard did not start: {error}");
    }
    match snap.dashboard {
        DashboardRun::Running => snap
            .dashboard_urls
            .first()
            .map(|url| format!("Open {url} in a phone or tablet browser."))
            .unwrap_or_else(|| "The companion dashboard is running.".to_owned()),
        DashboardRun::Starting => "The companion dashboard is starting.".to_owned(),
        DashboardRun::Off => {
            "Turn this on to view live metrics in a phone or tablet browser.".to_owned()
        }
        DashboardRun::Failed => "The companion dashboard did not start.".to_owned(),
    }
}

fn confirm_view(title: &str, detail: &str, index: usize) -> MenuView {
    MenuView {
        title: title.to_owned(),
        lines: vec!["Yes".to_owned(), "No".to_owned()],
        detail: detail.to_owned(),
        selected: index.min(1),
        bars: Vec::new(),
        warn: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            settings: Settings::default(),
            status_line: "Simulated rider".to_owned(),
            dashboard: DashboardRun::Off,
            dashboard_urls: Vec::new(),
            dashboard_error: None,
            music_report: String::new(),
            phase: pp_session::LinkPhase::Idle,
            failure_reason: String::new(),
            connected_name: String::new(),
            controller_status: String::new(),
            straps: Vec::new(),
            heart_line: String::new(),
            riders: vec![
                RiderOption {
                    id: "nils".to_owned(),
                    name: "Nils".to_owned(),
                },
                RiderOption {
                    id: "sigrid".to_owned(),
                    name: "Sigrid".to_owned(),
                },
            ],
            kits: vec![RiderOption {
                id: "default".to_owned(),
                name: "Default".to_owned(),
            }],
            strava: crate::extras::StravaRow::Connect,
            devices: vec![
                Device {
                    id: "FAKE:KICKR".to_owned(),
                    name: "Fake Kickr".to_owned(),
                    rssi_dbm: Some(-72),
                    power_only: false,
                    bars: 2,
                    ftms: true,
                    cycling_power: true,
                    fec: false,
                    remembered: false,
                },
                Device {
                    id: "FAKE:NEO".to_owned(),
                    name: "Fake Neo 2T".to_owned(),
                    rssi_dbm: Some(-48),
                    power_only: true,
                    bars: 4,
                    ftms: false,
                    cycling_power: true,
                    fec: false,
                    remembered: false,
                },
            ],
        }
    }

    #[test]
    fn the_rider_row_shows_the_display_name_and_cycles_ids() {
        let mut menu = MenuState::default();
        menu.open_main();
        menu.index = 6;
        let view = menu.view(&snap());
        assert_eq!(view.lines[6], "Rider    ◀ Nils ▶");
        assert!(view.detail.contains("Left and right"));
        let Action::Apply(settings) = menu.handle(Command::Right, &snap()) else {
            panic!("rider");
        };
        assert_eq!(settings.rider, "sigrid");
        let mut next = snap();
        next.settings = settings;
        let view = menu.view(&next);
        assert_eq!(view.lines[6], "Rider    ◀ Sigrid ▶");
    }

    #[test]
    fn the_kit_row_lists_only_the_kits_it_was_given() {
        let mut menu = MenuState::default();
        menu.open_main();
        let mut snap = snap();
        snap.kits.push(RiderOption {
            id: "kit-sunrise".to_owned(),
            name: "Sunrise".to_owned(),
        });
        menu.index = 7;
        let view = menu.view(&snap);
        assert_eq!(view.lines[7], "Kit    ◀ Default ▶");
        let Action::Apply(settings) = menu.handle(Command::Right, &snap) else {
            panic!("kit");
        };
        assert_eq!(settings.kit, "kit-sunrise");
        assert_eq!(settings.rider, "nils");
    }

    #[test]
    fn weight_and_difficulty_step_and_clamp() {
        let mut menu = MenuState::default();
        menu.open_main();
        menu.handle(Command::Down, &snap());
        menu.handle(Command::Down, &snap());
        let Action::Apply(settings) = menu.handle(Command::Left, &snap()) else {
            panic!("difficulty");
        };
        assert!((settings.trainer_difficulty - 0.45).abs() < 0.001);
        menu.handle(Command::Down, &snap());
        let mut current = snap();
        current.settings = settings;
        let Action::Apply(settings) = menu.handle(Command::Right, &current) else {
            panic!("weight");
        };
        assert!((settings.rider_mass_kg - 75.5).abs() < 0.01);
        current.settings.ftp_w = MAX_FTP_W;
        menu.index = 5;
        let Action::Apply(settings) = menu.handle(Command::Right, &current) else {
            panic!("ftp");
        };
        assert_eq!(settings.ftp_w, MAX_FTP_W);
        menu.index = 0;
        menu.handle(Command::Up, &current);
        assert_eq!(menu.index, 0);
    }

    #[test]
    fn pairing_lists_the_strongest_first_and_disconnect_is_the_last_row() {
        let ranked = rank(&snap().devices);
        assert_eq!(ranked[0].name, "Fake Neo 2T");
        assert_eq!(ranked[1].name, "Fake Kickr");
        let mut menu = MenuState::default();
        menu.open_main();
        assert!(matches!(
            menu.handle(Command::Accept, &snap()),
            Action::Scan
        ));
        let view = menu.view(&snap());
        assert_eq!(view.title, "Trainers");
        assert!(view.lines[0].contains("Fake Neo 2T"));
        assert!(view.lines[0].contains("strong"));
        assert!(view.lines[0].contains("power only, no resistance"));
        assert!(view.lines[1].contains("ok"));
        assert!(!view.lines[1].contains("power only"));
        menu.handle(Command::Down, &snap());
        menu.handle(Command::Down, &snap());
        menu.handle(Command::Down, &snap());
        assert!(matches!(
            menu.handle(Command::Accept, &snap()),
            Action::Disconnect
        ));
    }

    #[test]
    fn confirm_yes_quits_and_no_returns_and_bluetooth_has_a_plain_hint() {
        let mut menu = MenuState::default();
        menu.open_main();
        menu.index = 17;
        menu.handle(Command::Accept, &snap());
        assert_eq!(menu.page(), Page::ConfirmQuit);
        menu.handle(Command::Down, &snap());
        assert!(matches!(
            menu.handle(Command::Accept, &snap()),
            Action::None
        ));
        assert_eq!(menu.page(), Page::Main);
        menu.index = 17;
        menu.handle(Command::Accept, &snap());
        assert!(matches!(
            menu.handle(Command::Accept, &snap()),
            Action::Quit
        ));
        let mut offline = snap();
        offline.status_line = "Bluetooth unavailable: off".to_owned();
        offline.devices.clear();
        menu.open_main();
        menu.handle(Command::Accept, &offline);
        let view = menu.view(&offline);
        assert!(view.detail.contains("Bluetooth is unavailable"));
        menu.save_error = "Could not save settings: denied".to_owned();
        let view = menu.view(&snap());
        assert!(view.detail.contains("Could not save"));
    }

    #[test]
    fn equal_signals_stay_in_name_order() {
        let devices = vec![
            Device {
                id: "b".to_owned(),
                name: "Beta".to_owned(),
                rssi_dbm: Some(-70),
                power_only: false,
                bars: 2,
                ftms: false,
                cycling_power: false,
                fec: false,
                remembered: false,
            },
            Device {
                id: "a".to_owned(),
                name: "Alpha".to_owned(),
                rssi_dbm: Some(-70),
                power_only: false,
                bars: 2,
                ftms: false,
                cycling_power: false,
                fec: false,
                remembered: false,
            },
        ];
        let ranked = rank(&devices);
        assert_eq!(ranked[0].name, "Alpha");
        assert_eq!(signal(None), "no signal");
        assert_eq!(signal(Some(-80)), "weak");
    }
}
