//! Title, pause, summary, and course screens.
//!
//! Settings and pairing stay in [`crate::menu`]. This module decides which
//! screen is up and what a press does. Godot only draws [`ShellView`].

use crate::menu::{Action, Command, MenuState, Snapshot};

/// Where the player is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// Launch menu over the waiting rider.
    Title,
    /// The ride, no menu.
    Riding,
    /// Resume, Trainer, Settings, End ride.
    Pause,
    /// Settings, opened from the title or the pause menu.
    Settings,
    /// Confirm ending the ride.
    ConfirmEnd,
    /// Confirm quitting the game from the title.
    ConfirmQuit,
    /// Totals after the ride ends.
    Summary,
    /// Pick a course. The list comes from the session.
    Course,
}

/// What the game should do.
#[derive(Clone, Debug, PartialEq)]
pub enum ShellEffect {
    /// Nothing structural.
    None,
    /// Leave the title and ride.
    BeginRide,
    /// Close the pause menu and keep riding.
    Resume,
    /// Quit the process.
    Quit,
    /// The player confirmed the end of the ride. Show the summary next.
    EndRide,
    /// Summary: ride the same course again.
    RideAgain,
    /// Summary or cancel: back to the title at the start line.
    BackToTitle,
    /// A settings action. The menu state already moved.
    Menu(Action),
    /// Select this course row.
    PickCourse(usize),
}

/// One line on the title, pause, summary, or course list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellRow {
    /// Label.
    pub label: String,
    /// Right-hand detail. Empty for a plain button.
    pub detail: String,
}

/// What to draw, not including the settings overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellView {
    /// Which screen.
    pub screen: Screen,
    /// Title or empty when the settings menu draws itself.
    pub title: String,
    /// Rows.
    pub rows: Vec<ShellRow>,
    /// Focused row.
    pub selected: usize,
    /// One line under the rows.
    pub detail: String,
}

/// Course row the picker can show.
#[derive(Clone, Debug, PartialEq)]
pub struct CourseCard {
    /// Name.
    pub name: String,
    /// Length in metres.
    pub length_m: f64,
    /// Ascent in metres.
    pub ascent_m: f32,
    /// Steepest uphill grade, percent.
    pub max_grade_pct: f32,
}

/// The screen stack.
#[derive(Clone, Debug)]
pub struct Shell {
    screen: Screen,
    title_index: usize,
    pause_index: usize,
    summary_index: usize,
    course_index: usize,
    confirm_index: usize,
    /// Settings was opened from the title, so Back returns there.
    settings_from_title: bool,
    menu: MenuState,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            screen: Screen::Title,
            title_index: 0,
            pause_index: 0,
            summary_index: 0,
            course_index: 0,
            confirm_index: 0,
            settings_from_title: false,
            menu: MenuState::default(),
        }
    }
}

impl Shell {
    /// Start on the title, or already riding when the script asked to skip it.
    pub fn new(skip_title: bool) -> Self {
        let mut shell = Self::default();
        if skip_title {
            shell.screen = Screen::Riding;
        }
        shell
    }

    /// Current screen.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The ride HUD is visible only while riding with no menu over it.
    pub fn shows_hud(&self) -> bool {
        self.screen == Screen::Riding
    }

    /// The rider is waiting, so power keys must not move the bike.
    pub fn holds_rider(&self) -> bool {
        matches!(
            self.screen,
            Screen::Title | Screen::Summary | Screen::Course | Screen::ConfirmQuit
        )
    }

    /// Settings menu, when that page is up.
    pub fn menu_mut(&mut self) -> &mut MenuState {
        &mut self.menu
    }

    /// Settings closed. Back to the title or the pause menu.
    pub fn leave_settings(&mut self) {
        self.menu.close();
        self.screen = if self.settings_from_title {
            Screen::Title
        } else {
            Screen::Pause
        };
    }

    /// Open settings the way the smoke test does, from a ride.
    pub fn open_settings(&mut self) {
        self.settings_from_title = self.screen == Screen::Title;
        self.screen = Screen::Settings;
        self.menu.open_main();
    }

    /// Close every menu and ride.
    pub fn close_menus(&mut self) {
        self.menu.close();
        if self.screen != Screen::Title && self.screen != Screen::Summary {
            self.screen = Screen::Riding;
        }
    }

    /// Handle one press. `fresh` wraps; a key repeat does not.
    pub fn handle(
        &mut self,
        command: Command,
        fresh: bool,
        snap: &Snapshot,
        courses: usize,
    ) -> ShellEffect {
        if command == Command::Back && matches!(self.screen, Screen::Title) {
            return ShellEffect::None;
        }
        match self.screen {
            Screen::Title => self.title_command(command, fresh, courses),
            Screen::Riding => {
                if matches!(command, Command::Back) {
                    self.screen = Screen::Pause;
                    self.pause_index = 0;
                }
                ShellEffect::None
            }
            Screen::Pause => self.pause_command(command, fresh),
            Screen::Settings => self.settings_command(command, snap),
            Screen::ConfirmEnd => self.confirm(command, true),
            Screen::ConfirmQuit => self.confirm(command, false),
            Screen::Summary => self.summary_command(command, fresh),
            Screen::Course => self.course_command(command, fresh, courses),
        }
    }

    /// Rows for the active screen. Settings returns an empty view; the menu draws that.
    pub fn view(
        &self,
        course_name: &str,
        trainer: &str,
        course_detail: &str,
        courses: &[CourseCard],
        summary_line: &str,
    ) -> ShellView {
        match self.screen {
            Screen::Title => list_view(
                Screen::Title,
                "",
                &title_rows(course_name, trainer),
                self.title_index,
                course_detail,
            ),
            Screen::Pause => list_view(
                Screen::Pause,
                "Paused",
                &pause_rows(),
                self.pause_index,
                "The ride keeps going underneath.",
            ),
            Screen::ConfirmEnd => list_view(
                Screen::ConfirmEnd,
                "End this ride?",
                &[row("Keep riding", ""), row("End ride", "")],
                self.confirm_index,
                "You will see the summary, then the title screen.",
            ),
            Screen::ConfirmQuit => list_view(
                Screen::ConfirmQuit,
                "Quit Penguin Pass?",
                &[row("Keep playing", ""), row("Quit", "")],
                self.confirm_index,
                "The game closes.",
            ),
            Screen::Summary => list_view(
                Screen::Summary,
                "Ride complete",
                &[row("Ride again", ""), row("Back to title", "")],
                self.summary_index,
                summary_line,
            ),
            Screen::Course => {
                let rows: Vec<ShellRow> = courses
                    .iter()
                    .map(|card| {
                        row(
                            &card.name,
                            &format!(
                                "{:.1} km · {:.0} m up · {:.0}%",
                                card.length_m / 1000.0,
                                card.ascent_m,
                                card.max_grade_pct
                            ),
                        )
                    })
                    .collect();
                let detail = if rows.is_empty() {
                    "Building course…".to_owned()
                } else {
                    "Choose a course. The rider waits at its start.".to_owned()
                };
                list_view(Screen::Course, "Course", &rows, self.course_index, &detail)
            }
            Screen::Riding | Screen::Settings => ShellView {
                screen: self.screen,
                title: String::new(),
                rows: Vec::new(),
                selected: 0,
                detail: String::new(),
            },
        }
    }

    fn title_command(&mut self, command: Command, fresh: bool, _courses: usize) -> ShellEffect {
        const N: usize = 5;
        match command {
            Command::Up => move_focus(&mut self.title_index, N, false, fresh),
            Command::Down => move_focus(&mut self.title_index, N, true, fresh),
            Command::Accept => match self.title_index {
                0 => {
                    self.screen = Screen::Riding;
                    return ShellEffect::BeginRide;
                }
                1 => {
                    self.screen = Screen::Course;
                    self.course_index = 0;
                }
                2 => {
                    self.settings_from_title = true;
                    self.screen = Screen::Settings;
                    self.menu.open_pairing();
                    return ShellEffect::Menu(Action::Scan);
                }
                3 => {
                    self.settings_from_title = true;
                    self.screen = Screen::Settings;
                    self.menu.open_main();
                }
                _ => {
                    self.screen = Screen::ConfirmQuit;
                    self.confirm_index = 0;
                }
            },
            Command::Back | Command::Left | Command::Right => {}
        }
        ShellEffect::None
    }

    fn pause_command(&mut self, command: Command, fresh: bool) -> ShellEffect {
        const N: usize = 4;
        match command {
            Command::Up => move_focus(&mut self.pause_index, N, false, fresh),
            Command::Down => move_focus(&mut self.pause_index, N, true, fresh),
            Command::Back => {
                self.screen = Screen::Riding;
                return ShellEffect::Resume;
            }
            Command::Accept => match self.pause_index {
                0 => {
                    self.screen = Screen::Riding;
                    return ShellEffect::Resume;
                }
                1 => {
                    self.settings_from_title = false;
                    self.screen = Screen::Settings;
                    self.menu.open_pairing();
                    return ShellEffect::Menu(Action::Scan);
                }
                2 => {
                    self.settings_from_title = false;
                    self.screen = Screen::Settings;
                    self.menu.open_main();
                }
                _ => {
                    self.screen = Screen::ConfirmEnd;
                    self.confirm_index = 0;
                }
            },
            Command::Left | Command::Right => {}
        }
        ShellEffect::None
    }

    fn settings_command(&mut self, command: Command, snap: &Snapshot) -> ShellEffect {
        let action = self.menu.handle(command, snap);
        if matches!(action, Action::Quit) {
            self.screen = Screen::Summary;
            self.summary_index = 0;
            self.menu.close();
            return ShellEffect::EndRide;
        }
        if self.menu.page() == crate::menu::Page::Main && !self.menu.open {
            self.screen = if self.settings_from_title {
                Screen::Title
            } else {
                Screen::Pause
            };
        }
        ShellEffect::Menu(action)
    }

    fn confirm(&mut self, command: Command, end_ride: bool) -> ShellEffect {
        match command {
            Command::Left | Command::Right | Command::Up | Command::Down => {
                self.confirm_index = if self.confirm_index == 0 { 1 } else { 0 };
            }
            Command::Back => {
                self.screen = if end_ride {
                    Screen::Pause
                } else {
                    Screen::Title
                };
                self.confirm_index = 0;
            }
            Command::Accept => {
                let yes = self.confirm_index == 1;
                self.confirm_index = 0;
                if !yes {
                    self.screen = if end_ride {
                        Screen::Pause
                    } else {
                        Screen::Title
                    };
                    return ShellEffect::None;
                }
                if end_ride {
                    self.screen = Screen::Summary;
                    self.summary_index = 0;
                    return ShellEffect::EndRide;
                }
                return ShellEffect::Quit;
            }
        }
        ShellEffect::None
    }

    fn summary_command(&mut self, command: Command, fresh: bool) -> ShellEffect {
        match command {
            Command::Up | Command::Down | Command::Left | Command::Right => {
                if fresh {
                    self.summary_index = if self.summary_index == 0 { 1 } else { 0 };
                }
            }
            Command::Accept => {
                if self.summary_index == 0 {
                    self.screen = Screen::Riding;
                    return ShellEffect::RideAgain;
                }
                self.screen = Screen::Title;
                self.title_index = 0;
                return ShellEffect::BackToTitle;
            }
            Command::Back => {}
        }
        ShellEffect::None
    }

    fn course_command(&mut self, command: Command, fresh: bool, count: usize) -> ShellEffect {
        let count = count.max(1);
        match command {
            Command::Up => move_focus(&mut self.course_index, count, false, fresh),
            Command::Down => move_focus(&mut self.course_index, count, true, fresh),
            Command::Back => {
                self.screen = Screen::Title;
            }
            Command::Accept => {
                let index = self.course_index.min(count - 1);
                self.screen = Screen::Title;
                return ShellEffect::PickCourse(index);
            }
            Command::Left | Command::Right => {}
        }
        ShellEffect::None
    }
}

fn title_rows(course: &str, trainer: &str) -> Vec<ShellRow> {
    vec![
        row("Start ride", ""),
        row("Course", course),
        row("Trainer", trainer),
        row("Settings", ""),
        row("Quit", ""),
    ]
}

fn pause_rows() -> Vec<ShellRow> {
    vec![
        row("Resume", ""),
        row("Trainer", ""),
        row("Settings", ""),
        row("End ride", ""),
    ]
}

fn row(label: &str, detail: &str) -> ShellRow {
    ShellRow {
        label: label.to_owned(),
        detail: detail.to_owned(),
    }
}

fn list_view(
    screen: Screen,
    title: &str,
    rows: &[ShellRow],
    selected: usize,
    detail: &str,
) -> ShellView {
    ShellView {
        screen,
        title: title.to_owned(),
        rows: rows.to_vec(),
        selected: selected.min(rows.len().saturating_sub(1)),
        detail: detail.to_owned(),
    }
}

fn move_focus(index: &mut usize, count: usize, down: bool, fresh: bool) {
    if count == 0 {
        return;
    }
    if down {
        if *index + 1 >= count {
            if fresh {
                *index = 0;
            }
        } else {
            *index += 1;
        }
    } else if *index == 0 {
        if fresh {
            *index = count - 1;
        }
    } else {
        *index -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Snapshot;
    use pp_session::Settings;

    fn snap() -> Snapshot {
        Snapshot {
            settings: Settings::default(),
            status_line: "Simulated rider".to_owned(),
            dashboard: pp_session::DashboardRun::Off,
            dashboard_urls: Vec::new(),
            dashboard_error: None,
            devices: Vec::new(),
            music_report: String::new(),
        }
    }

    #[test]
    fn title_starts_the_ride_and_quit_confirms() {
        let mut shell = Shell::new(false);
        assert_eq!(shell.screen(), Screen::Title);
        assert!(!shell.shows_hud());
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), 1),
            ShellEffect::BeginRide
        ));
        assert!(shell.shows_hud());
        shell.handle(Command::Back, true, &snap(), 1);
        assert_eq!(shell.screen(), Screen::Pause);
        let view = shell.view("Penguin Pass", "Not paired", "", &[], "");
        assert_eq!(view.rows[0].label, "Resume");
        assert_eq!(view.rows[3].label, "End ride");
        shell.handle(Command::Down, true, &snap(), 1);
        shell.handle(Command::Down, true, &snap(), 1);
        shell.handle(Command::Down, true, &snap(), 1);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), 1),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::ConfirmEnd);
        shell.handle(Command::Right, true, &snap(), 1);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), 1),
            ShellEffect::EndRide
        ));
        assert_eq!(shell.screen(), Screen::Summary);
        shell.handle(Command::Down, true, &snap(), 1);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), 1),
            ShellEffect::BackToTitle
        ));
        assert_eq!(shell.screen(), Screen::Title);
        assert!(!shell.shows_hud());
    }

    #[test]
    fn a_fresh_press_wraps_and_a_hold_stops() {
        let mut shell = Shell::new(false);
        shell.title_index = 4;
        shell.handle(Command::Down, true, &snap(), 1);
        assert_eq!(shell.title_index, 0);
        shell.title_index = 4;
        shell.handle(Command::Down, false, &snap(), 1);
        assert_eq!(shell.title_index, 4);
    }

    #[test]
    fn skip_title_starts_on_the_ride_and_back_does_nothing_on_the_title() {
        let shell = Shell::new(true);
        assert_eq!(shell.screen(), Screen::Riding);
        let mut shell = Shell::new(false);
        assert!(matches!(
            shell.handle(Command::Back, true, &snap(), 1),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Title);
        let view = shell.view("Penguin Pass", "Searching", "5.0 km", &[], "");
        assert_eq!(view.rows[1].detail, "Penguin Pass");
        assert_eq!(view.rows[2].detail, "Searching");
    }
}
