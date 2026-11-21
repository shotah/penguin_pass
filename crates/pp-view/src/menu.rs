//! Settings menu as a state machine.
//!
//! Focus stops at the ends (it does not wrap): the smoke test walks three
//! steps from the first trainer to "Disconnect". The Godot side only draws
//! [`MenuView`] and performs [`Action`].

use pp_session::{
    DashboardRun, MAX_BIKE_MASS_KG, MAX_FTP_W, MAX_RIDER_MASS_KG, MAX_TRAINER_DIFFICULTY,
    MIN_BIKE_MASS_KG, MIN_FTP_W, MIN_RIDER_MASS_KG, MIN_TRAINER_DIFFICULTY, Settings,
};

const MAIN_ROWS: usize = 10;

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
    /// Show the main page.
    pub fn open_main(&mut self) {
        self.open = true;
        self.page = Page::Main;
        self.index = 0;
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
                8 => {
                    self.page = Page::ConfirmRestart;
                    self.index = 0;
                }
                9 => {
                    self.page = Page::ConfirmQuit;
                    self.index = 0;
                }
                1 | 6 | 7 => return Action::Apply(toggled(snap.settings.clone(), self.index)),
                _ => {}
            },
            Command::Left | Command::Right => {
                let increase = matches!(command, Command::Right);
                if let Some(settings) = stepped(snap.settings.clone(), self.index, increase) {
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
            format!("TV dashboard    {}", dashboard_value(snap)),
            format!("Full screen    {}", on_off(settings.fullscreen)),
            "Restart ride".to_owned(),
            "Quit".to_owned(),
        ];
        let detail = if !self.save_error.is_empty() {
            self.save_error.clone()
        } else {
            match self.index {
                2 => "How much of the hill the trainer makes you feel. On-screen speed always uses the real hill.".to_owned(),
                6 => dashboard_detail(snap),
                0 => "Open this to scan and connect. The ride keeps going.".to_owned(),
                _ => String::new(),
            }
        };
        MenuView {
            title: "Settings".to_owned(),
            selected: self.index.min(lines.len().saturating_sub(1)),
            lines,
            detail,
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
        for device in &devices {
            lines.push(format!("{}    {}", device.name, signal(device.rssi_dbm)));
        }
        lines.push("Disconnect and use the simulated rider".to_owned());
        let index = self.index.min(lines.len().saturating_sub(1));
        self.index = index;
        let hint = if snap.status_line.contains("Bluetooth unavailable") {
            "Bluetooth is unavailable on this computer. The simulated rider keeps going. Up and Down still change power.".to_owned()
        } else if devices.is_empty() && snap.status_line.contains("Scanning") {
            "Looking for trainers. Wake yours by pedalling. Close other apps or bike computers that may already be connected to it.".to_owned()
        } else {
            snap.status_line.clone()
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
        6 => settings.tv_dashboard_enabled = !settings.tv_dashboard_enabled,
        7 => settings.fullscreen = !settings.fullscreen,
        _ => {}
    }
    settings
}

fn stepped(mut settings: Settings, index: usize, increase: bool) -> Option<Settings> {
    match index {
        1 | 6 | 7 => Some(toggled(settings, index)),
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
        _ => None,
    }
}

fn step_f32(value: f32, step: f32, increase: bool, min: f32, max: f32) -> f32 {
    let next = if increase { value + step } else { value - step };
    (next.clamp(min, max) * 100.0).round() / 100.0
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
        return format!("The TV page did not start: {error}");
    }
    match snap.dashboard {
        DashboardRun::Running => snap
            .dashboard_urls
            .first()
            .map(|url| format!("Open {url} on the TV or a phone."))
            .unwrap_or_else(|| "The TV page is running.".to_owned()),
        DashboardRun::Starting => "The TV page is starting.".to_owned(),
        DashboardRun::Off => "Turn this on to show the ride on another screen.".to_owned(),
        DashboardRun::Failed => "The TV page did not start.".to_owned(),
    }
}

fn confirm_view(title: &str, detail: &str, index: usize) -> MenuView {
    MenuView {
        title: title.to_owned(),
        lines: vec!["Yes".to_owned(), "No".to_owned()],
        detail: detail.to_owned(),
        selected: index.min(1),
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
            devices: vec![
                Device {
                    id: "FAKE:KICKR".to_owned(),
                    name: "Fake Kickr".to_owned(),
                    rssi_dbm: Some(-72),
                },
                Device {
                    id: "FAKE:NEO".to_owned(),
                    name: "Fake Neo 2T".to_owned(),
                    rssi_dbm: Some(-48),
                },
            ],
        }
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
        assert!(matches!(menu.handle(Command::Accept, &snap()), Action::Scan));
        let view = menu.view(&snap());
        assert_eq!(view.title, "Trainers");
        assert!(view.lines[0].contains("Fake Neo 2T"));
        assert!(view.lines[0].contains("strong"));
        assert!(view.lines[1].contains("ok"));
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
        menu.index = 9;
        menu.handle(Command::Accept, &snap());
        assert_eq!(menu.page(), Page::ConfirmQuit);
        menu.handle(Command::Down, &snap());
        assert!(matches!(menu.handle(Command::Accept, &snap()), Action::None));
        assert_eq!(menu.page(), Page::Main);
        menu.index = 9;
        menu.handle(Command::Accept, &snap());
        assert!(matches!(menu.handle(Command::Accept, &snap()), Action::Quit));
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
            },
            Device {
                id: "a".to_owned(),
                name: "Alpha".to_owned(),
                rssi_dbm: Some(-70),
            },
        ];
        let ranked = rank(&devices);
        assert_eq!(ranked[0].name, "Alpha");
        assert_eq!(signal(None), "no signal");
        assert_eq!(signal(Some(-80)), "weak");
    }
}
