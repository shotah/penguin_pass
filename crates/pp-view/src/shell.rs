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
    /// Lifetime totals and badges.
    Progress,
    /// Pick a workout, or a free ride.
    Workout,
}

/// What the game should do.
///
/// `Menu` carries the whole settings value. That is larger than the other
/// variants, and the clippy limit is the reason for the allow.
#[allow(clippy::large_enum_variant)]
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
    /// Flip the loaded course between forward and reverse.
    ToggleCourseDirection,
    /// Choose a workout. `None` is a free ride.
    PickWorkout(Option<String>),
    /// Nudge workout targets by this many percent.
    WorkoutBias(i8),
    /// Skip the current workout step.
    SkipWorkout,
    /// Upload the saved ride.
    UploadStrava,
    /// Store this ramp-test FTP.
    AcceptFtp(u16),
    /// Make a new generated route for this row.
    Reroll(usize),
    /// Open the rides folder. The summary row asked.
    OpenRides,
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
    /// Thumbnail samples, `(distance_m, elevation_m)`.
    pub profile: Vec<(f32, f32)>,
    /// Left and right can reroll this row in the course picker.
    pub rerollable: bool,
}

/// The screen stack.
#[derive(Clone, Debug)]
pub struct Shell {
    screen: Screen,
    title_index: usize,
    pause_index: usize,
    summary_index: usize,
    course_index: usize,
    /// The loaded course is ridden backward. The direction row shows it.
    course_reversed: bool,
    confirm_index: usize,
    progress_index: usize,
    progress_rows: Vec<ShellRow>,
    progress_blurbs: Vec<String>,
    workout_name: String,
    workout_ids: Vec<Option<String>>,
    workout_rows: Vec<ShellRow>,
    summary_extra: Vec<ShellRow>,
    summary_ftp: Option<u16>,
    summary_upload: bool,
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
            course_reversed: false,
            confirm_index: 0,
            progress_index: 0,
            progress_rows: Vec::new(),
            progress_blurbs: Vec::new(),
            workout_name: "Free ride".to_owned(),
            workout_ids: Vec::new(),
            workout_rows: Vec::new(),
            summary_extra: Vec::new(),
            summary_ftp: None,
            summary_upload: false,
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

    /// Move the highlight when the pointer is over a row.
    pub fn hover(&mut self, index: usize) {
        match self.screen {
            Screen::Title => self.title_index = index.min(7),
            Screen::Pause => self.pause_index = index.min(8),
            Screen::Workout => self.course_index = index,
            Screen::Progress => self.progress_index = index,
            Screen::Summary => self.summary_index = index.min(2 + self.summary_extra.len()),
            Screen::ConfirmEnd | Screen::ConfirmQuit => {
                self.confirm_index = index.min(1);
            }
            Screen::Course => self.course_index = index,
            _ => {}
        }
    }

    /// The rider is waiting, so power keys must not move the bike.
    pub fn holds_rider(&self) -> bool {
        matches!(
            self.screen,
            Screen::Title | Screen::Summary | Screen::Course | Screen::ConfirmQuit
        ) || (matches!(self.screen, Screen::Progress | Screen::Workout) && self.settings_from_title)
    }

    /// Whether the course picker should say Reverse.
    pub fn show_course_reversed(&mut self, reversed: bool) {
        self.course_reversed = reversed;
    }

    /// Pending ride direction while the course picker is open.
    #[must_use]
    pub fn course_reversed(&self) -> bool {
        self.course_reversed
    }

    /// Focus row and direction when opening the course list.
    pub fn open_course_picker(&mut self, index: usize, reversed: bool) {
        self.course_index = index;
        self.course_reversed = reversed;
    }

    /// Name shown on the title's Workout row, and the picker list.
    pub fn set_workouts(&mut self, selected: &str, rows: Vec<ShellRow>, ids: Vec<Option<String>>) {
        self.workout_name = selected.to_owned();
        self.workout_rows = rows;
        self.workout_ids = ids;
    }

    /// Extra summary rows: an upload, then an FTP offer.
    pub fn set_summary_extra(&mut self, upload: bool, ftp_w: Option<u16>) {
        self.summary_upload = upload;
        self.summary_ftp = ftp_w;
        self.summary_extra.clear();
        if upload {
            self.summary_extra.push(row("Upload to Strava", ""));
        }
        if let Some(watts) = ftp_w {
            self.summary_extra
                .push(row(&format!("Use {watts} W as FTP"), ""));
        }
    }

    pub fn set_progress(&mut self, rows: Vec<ShellRow>, blurbs: Vec<String>) {
        self.progress_rows = rows;
        self.progress_blurbs = blurbs;
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
        if matches!(self.screen, Screen::Progress | Screen::Workout) {
            self.screen = if self.settings_from_title {
                Screen::Title
            } else {
                Screen::Pause
            };
            return;
        }
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
        courses: &[CourseCard],
    ) -> ShellEffect {
        if command == Command::Back && matches!(self.screen, Screen::Title) {
            return ShellEffect::None;
        }
        match self.screen {
            Screen::Title => self.title_command(command, fresh, courses.len()),
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
            Screen::Progress => self.progress_command(command, fresh),
            Screen::Workout => self.workout_command(command, fresh),
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
        heart: &str,
    ) -> ShellView {
        match self.screen {
            Screen::Title => list_view(
                Screen::Title,
                "",
                &title_rows(course_name, trainer, heart, &self.workout_name),
                self.title_index,
                course_detail,
            ),
            Screen::Pause => list_view(
                Screen::Pause,
                "Paused",
                &pause_rows(heart),
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
            Screen::Summary => {
                let mut rows = vec![
                    row("Ride again", ""),
                    row("Back to title", ""),
                    row("Open rides folder", ""),
                ];
                rows.extend(self.summary_extra.clone());
                list_view(
                    Screen::Summary,
                    "Ride complete",
                    &rows,
                    self.summary_index,
                    summary_line,
                )
            }
            Screen::Course => {
                let mut rows: Vec<ShellRow> = courses
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
                let direction_at = rows.len();
                rows.push(row(
                    "Direction",
                    if self.course_reversed {
                        "Reverse"
                    } else {
                        "Forward"
                    },
                ));
                let detail = if courses.is_empty() {
                    "Building course…".to_owned()
                } else if self.course_index >= direction_at {
                    "Left, right, or accept flips direction for your next pick.".to_owned()
                } else if courses
                    .get(self.course_index)
                    .is_some_and(|card| card.rerollable)
                {
                    "Left or right makes a new route. Accept starts it.".to_owned()
                } else {
                    "Accept starts the course.".to_owned()
                };
                list_view(Screen::Course, "Course", &rows, self.course_index, &detail)
            }
            Screen::Progress => {
                let count = self.progress_rows.len().max(1);
                let index = self.progress_index.min(count - 1);
                let blurb = self.progress_blurbs.get(index).cloned().unwrap_or_default();
                list_view(
                    Screen::Progress,
                    "Progress",
                    &self.progress_rows,
                    index,
                    &blurb,
                )
            }
            Screen::Workout => list_view(
                Screen::Workout,
                "Workout",
                &self.workout_rows,
                self.course_index,
                "Free ride, or a plan. Accept starts it from the title.",
            ),
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
        const N: usize = 8;
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
                    self.menu.open_heart();
                    return ShellEffect::Menu(Action::OpenHeart);
                }
                4 => {
                    self.settings_from_title = true;
                    self.screen = Screen::Settings;
                    self.menu.open_main();
                }
                5 => {
                    self.settings_from_title = true;
                    self.course_index = 0;
                    self.screen = Screen::Workout;
                }
                6 => {
                    self.screen = Screen::ConfirmQuit;
                    self.confirm_index = 0;
                }
                _ => {
                    self.settings_from_title = true;
                    self.progress_index = 0;
                    self.screen = Screen::Progress;
                }
            },
            Command::Back | Command::Left | Command::Right => {}
        }
        ShellEffect::None
    }

    fn pause_command(&mut self, command: Command, fresh: bool) -> ShellEffect {
        const N: usize = 9;
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
                    self.menu.open_heart();
                    return ShellEffect::Menu(Action::OpenHeart);
                }
                3 => {
                    self.settings_from_title = false;
                    self.screen = Screen::Settings;
                    self.menu.open_main();
                }
                4 => {
                    self.screen = Screen::ConfirmEnd;
                    self.confirm_index = 0;
                }
                5 => {
                    self.settings_from_title = false;
                    self.progress_index = 0;
                    self.screen = Screen::Progress;
                }
                6 => return ShellEffect::WorkoutBias(-5),
                7 => return ShellEffect::WorkoutBias(5),
                _ => return ShellEffect::SkipWorkout,
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

    fn progress_command(&mut self, command: Command, fresh: bool) -> ShellEffect {
        let count = self.progress_rows.len().max(1);
        match command {
            Command::Up => move_focus(&mut self.progress_index, count, false, fresh),
            Command::Down => move_focus(&mut self.progress_index, count, true, fresh),
            Command::Back | Command::Accept => {
                self.screen = if self.settings_from_title {
                    Screen::Title
                } else {
                    Screen::Pause
                };
            }
            Command::Left | Command::Right => {}
        }
        ShellEffect::None
    }

    fn workout_command(&mut self, command: Command, fresh: bool) -> ShellEffect {
        let count = self.workout_ids.len().max(1);
        match command {
            Command::Up => move_focus(&mut self.course_index, count, false, fresh),
            Command::Down => move_focus(&mut self.course_index, count, true, fresh),
            Command::Back => {
                self.screen = if self.settings_from_title {
                    Screen::Title
                } else {
                    Screen::Pause
                };
            }
            Command::Accept => {
                let index = self.course_index.min(count - 1);
                let id = self.workout_ids.get(index).cloned().unwrap_or(None);
                self.screen = Screen::Title;
                self.title_index = 0;
                return ShellEffect::PickWorkout(id);
            }
            Command::Left | Command::Right => {}
        }
        ShellEffect::None
    }

    fn summary_command(&mut self, command: Command, fresh: bool) -> ShellEffect {
        let count = 3 + self.summary_extra.len();
        match command {
            Command::Up => move_focus(&mut self.summary_index, count, false, fresh),
            Command::Down => move_focus(&mut self.summary_index, count, true, fresh),
            Command::Left | Command::Right => {}
            Command::Back => {
                self.screen = Screen::Title;
                self.title_index = 0;
                return ShellEffect::BackToTitle;
            }
            Command::Accept => match self.summary_index {
                0 => {
                    self.screen = Screen::Riding;
                    return ShellEffect::RideAgain;
                }
                1 => {
                    self.screen = Screen::Title;
                    self.title_index = 0;
                    return ShellEffect::BackToTitle;
                }
                2 => return ShellEffect::OpenRides,
                _ => {
                    let extra = self.summary_index - 3;
                    if self.summary_upload && extra == 0 {
                        return ShellEffect::UploadStrava;
                    }
                    if let Some(watts) = self.summary_ftp {
                        return ShellEffect::AcceptFtp(watts);
                    }
                }
            },
        }
        ShellEffect::None
    }

    fn course_command(
        &mut self,
        command: Command,
        fresh: bool,
        courses: &[CourseCard],
    ) -> ShellEffect {
        // `courses` is the catalog. The last row is Direction, not a course.
        let count = courses.len();
        let direction_at = count;
        let total = count + 1;
        match command {
            Command::Up => move_focus(&mut self.course_index, total, false, fresh),
            Command::Down => move_focus(&mut self.course_index, total, true, fresh),
            Command::Back => {
                self.screen = Screen::Title;
            }
            Command::Accept => {
                let index = self.course_index.min(total - 1);
                if index == direction_at {
                    self.course_reversed = !self.course_reversed;
                    return ShellEffect::None;
                }
                self.screen = Screen::Title;
                return ShellEffect::PickCourse(index);
            }
            Command::Left | Command::Right => {
                let index = self.course_index.min(total - 1);
                if index == direction_at {
                    if fresh {
                        self.course_reversed = !self.course_reversed;
                    }
                    return ShellEffect::None;
                }
                if index > 0 && courses.get(index).is_some_and(|card| card.rerollable) {
                    return ShellEffect::Reroll(index);
                }
            }
        }
        ShellEffect::None
    }
}

fn title_rows(course: &str, trainer: &str, heart: &str, workout: &str) -> Vec<ShellRow> {
    vec![
        row("Start ride", ""),
        row("Course", course),
        row("Trainer", trainer),
        row("Heart rate", heart),
        row("Settings", ""),
        row("Workout", workout),
        row("Quit", ""),
        row("Progress", ""),
    ]
}

fn pause_rows(heart: &str) -> Vec<ShellRow> {
    vec![
        row("Resume", ""),
        row("Trainer", ""),
        row("Heart rate", heart),
        row("Settings", ""),
        row("End ride", ""),
        row("Progress", ""),
        row("Intensity −5%", ""),
        row("Intensity +5%", ""),
        row("Skip interval", ""),
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
            phase: pp_session::LinkPhase::Idle,
            failure_reason: String::new(),
            connected_name: String::new(),
            controller_status: String::new(),
            straps: Vec::new(),
            heart_line: String::new(),
            riders: Vec::new(),
            kits: Vec::new(),
            strava: crate::extras::StravaRow::Connect,
        }
    }

    #[test]
    fn title_starts_the_ride_and_quit_confirms() {
        let mut shell = Shell::new(false);
        assert_eq!(shell.screen(), Screen::Title);
        assert!(!shell.shows_hud());
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::BeginRide
        ));
        assert!(shell.shows_hud());
        shell.handle(Command::Back, true, &snap(), &[]);
        assert_eq!(shell.screen(), Screen::Pause);
        let view = shell.view("Penguin Pass", "Not paired", "", &[], "", "");
        assert_eq!(view.rows[0].label, "Resume");
        assert_eq!(view.rows[4].label, "End ride");
        shell.handle(Command::Down, true, &snap(), &[]);
        shell.handle(Command::Down, true, &snap(), &[]);
        shell.handle(Command::Down, true, &snap(), &[]);
        shell.handle(Command::Down, true, &snap(), &[]);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::ConfirmEnd);
        shell.handle(Command::Right, true, &snap(), &[]);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::EndRide
        ));
        assert_eq!(shell.screen(), Screen::Summary);
        shell.handle(Command::Down, true, &snap(), &[]);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::BackToTitle
        ));
        assert_eq!(shell.screen(), Screen::Title);
        assert!(!shell.shows_hud());
    }

    #[test]
    fn hover_reaches_summary_extra_rows() {
        let mut shell = Shell::new(false);
        shell.screen = Screen::Summary;
        shell.set_summary_extra(true, Some(200));
        shell.hover(3);
        assert_eq!(shell.view("", "", "", &[], "", "").selected, 3);
        shell.hover(4);
        assert_eq!(shell.view("", "", "", &[], "", "").selected, 4);
    }

    #[test]
    fn a_fresh_press_wraps_and_a_hold_stops() {
        let mut shell = Shell::new(false);
        shell.title_index = 7;
        shell.handle(Command::Down, true, &snap(), &[]);
        assert_eq!(shell.title_index, 0);
        shell.title_index = 7;
        shell.handle(Command::Down, false, &snap(), &[]);
        assert_eq!(shell.title_index, 7);
    }

    #[test]
    fn skip_title_starts_on_the_ride_and_back_does_nothing_on_the_title() {
        let shell = Shell::new(true);
        assert_eq!(shell.screen(), Screen::Riding);
        let mut shell = Shell::new(false);
        assert!(matches!(
            shell.handle(Command::Back, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Title);
        let view = shell.view("Penguin Pass", "Searching", "5.0 km", &[], "", "");
        assert_eq!(view.rows[1].detail, "Penguin Pass");
        assert_eq!(view.rows[2].detail, "Searching");
    }

    #[test]
    fn a_pad_reaches_workout_progress_pause_and_the_summary() {
        let mut shell = Shell::new(false);
        shell.set_workouts(
            "Free ride",
            vec![row("Free ride", ""), row("Easy spin", "30 min")],
            vec![None, Some("easy".to_owned())],
        );
        for _ in 0..5 {
            shell.handle(Command::Down, true, &snap(), &[]);
        }
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Workout);
        shell.handle(Command::Down, true, &snap(), &[]);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::PickWorkout(Some(id)) if id == "easy"
        ));
        assert_eq!(shell.screen(), Screen::Title);
        assert_eq!(shell.title_index, 0);
        for _ in 0..5 {
            shell.handle(Command::Down, true, &snap(), &[]);
        }
        shell.handle(Command::Accept, true, &snap(), &[]);
        assert_eq!(shell.screen(), Screen::Workout);
        assert!(matches!(
            shell.handle(Command::Back, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Title);
        shell.title_index = 7;
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Progress);
        shell.set_progress(
            vec![row("Rides", "1"), row("First splash", "Earned")],
            vec![String::new(), "Finish a ride.".to_owned()],
        );
        shell.handle(Command::Down, true, &snap(), &[]);
        assert_eq!(shell.view("", "", "", &[], "", "").selected, 1);
        shell.handle(Command::Up, true, &snap(), &[]);
        assert_eq!(shell.view("", "", "", &[], "", "").selected, 0);
        shell.handle(Command::Back, true, &snap(), &[]);
        assert_eq!(shell.screen(), Screen::Title);
        shell.title_index = 0;
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::BeginRide
        ));
        assert!(shell.shows_hud());
        shell.handle(Command::Back, true, &snap(), &[]);
        assert_eq!(shell.screen(), Screen::Pause);
        shell.pause_index = 5;
        shell.handle(Command::Accept, true, &snap(), &[]);
        assert_eq!(shell.screen(), Screen::Progress);
        shell.handle(Command::Back, true, &snap(), &[]);
        assert_eq!(shell.screen(), Screen::Pause);
        shell.pause_index = 6;
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::WorkoutBias(-5)
        ));
        shell.pause_index = 7;
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::WorkoutBias(5)
        ));
        shell.pause_index = 8;
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::SkipWorkout
        ));
        assert!(matches!(
            shell.handle(Command::Back, true, &snap(), &[]),
            ShellEffect::Resume
        ));
        assert_eq!(shell.screen(), Screen::Riding);
        shell.screen = Screen::Summary;
        assert!(matches!(
            shell.handle(Command::Back, true, &snap(), &[]),
            ShellEffect::BackToTitle
        ));
        assert_eq!(shell.screen(), Screen::Title);
        assert!(!shell.shows_hud());
    }

    #[test]
    fn settings_from_the_title_cycles_the_new_rows_and_back_returns() {
        let mut shell = Shell::new(false);
        for _ in 0..4 {
            shell.handle(Command::Down, true, &snap(), &[]);
        }
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Settings);
        for _ in 0..21 {
            shell.handle(Command::Down, true, &snap(), &[]);
        }
        let ShellEffect::Menu(Action::Apply(settings)) =
            shell.handle(Command::Right, true, &snap(), &[])
        else {
            panic!("time of day");
        };
        assert_eq!(settings.time_of_day, "auto");
        shell.handle(Command::Down, true, &snap(), &[]);
        let ShellEffect::Menu(Action::Apply(settings)) =
            shell.handle(Command::Right, true, &snap(), &[])
        else {
            panic!("weather");
        };
        assert_eq!(settings.weather, "auto");
        shell.handle(Command::Down, true, &snap(), &[]);
        let ShellEffect::Menu(Action::Apply(settings)) =
            shell.handle(Command::Left, true, &snap(), &[])
        else {
            panic!("riders");
        };
        assert_eq!(settings.bots_count, 4);
        shell.handle(Command::Down, true, &snap(), &[]);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::Menu(Action::Strava)
        ));
        shell.handle(Command::Down, true, &snap(), &[]);
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::Menu(Action::Apply(_))
        ));
        assert!(matches!(
            shell.handle(Command::Back, true, &snap(), &[]),
            ShellEffect::Menu(Action::None)
        ));
        assert_eq!(shell.screen(), Screen::Title);
        assert!(shell.holds_rider());
    }

    #[test]
    fn the_title_course_row_shows_reverse_and_the_picker_toggles_it() {
        let mut shell = Shell::new(false);
        let titled = shell.view(
            "Penguin Pass (Reverse)",
            "Not paired",
            "5.0 km",
            &[],
            "",
            "",
        );
        assert_eq!(titled.rows[1].label, "Course");
        assert_eq!(titled.rows[1].detail, "Penguin Pass (Reverse)");

        shell.handle(Command::Down, true, &snap(), &course_cards(8));
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &course_cards(8)),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Course);
        let cards = course_cards(8);
        let opened = shell.view("Penguin Pass", "", "", &cards, "", "");
        assert_eq!(opened.selected, 0);
        assert_eq!(opened.rows[0].label, "Course 0");
        assert_eq!(opened.rows[8].label, "Direction");
        assert_eq!(opened.rows[8].detail, "Forward");
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &course_cards(8)),
            ShellEffect::PickCourse(0)
        ));
        assert_eq!(shell.screen(), Screen::Title);

        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &course_cards(8)),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Course);
        for _ in 0..8 {
            shell.handle(Command::Down, true, &snap(), &course_cards(8));
        }
        let at_direction = shell.view("Penguin Pass", "", "", &cards, "", "");
        assert_eq!(at_direction.selected, 8);
        assert_eq!(at_direction.rows[8].label, "Direction");
        assert_eq!(
            at_direction.detail,
            "Left, right, or accept flips direction for your next pick."
        );
        shell.handle(Command::Down, false, &snap(), &course_cards(8));
        assert_eq!(
            shell.view("Penguin Pass", "", "", &cards, "", "").selected,
            8
        );
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &course_cards(8)),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Course);
        assert_eq!(
            shell.view("Penguin Pass", "", "", &cards, "", "").rows[8].detail,
            "Reverse"
        );
        assert!(matches!(
            shell.handle(Command::Right, true, &snap(), &course_cards(8)),
            ShellEffect::None
        ));
        assert!(matches!(
            shell.handle(Command::Left, true, &snap(), &course_cards(8)),
            ShellEffect::None
        ));
        assert!(matches!(
            shell.handle(Command::Right, false, &snap(), &course_cards(8)),
            ShellEffect::None
        ));
        shell.show_course_reversed(true);
        let reversed = shell.view("Penguin Pass (Reverse)", "", "", &cards, "", "");
        assert_eq!(reversed.rows[8].detail, "Reverse");

        shell.course_index = 0;
        shell.handle(Command::Up, true, &snap(), &course_cards(8));
        assert_eq!(
            shell.view("Penguin Pass", "", "", &cards, "", "").selected,
            8
        );
        shell.course_index = 1;
        assert!(matches!(
            shell.handle(Command::Left, true, &snap(), &course_cards(8)),
            ShellEffect::Reroll(1)
        ));
        shell.open_course_picker(3, true);
        assert_eq!(shell.course_index, 3);
        assert!(shell.course_reversed());
    }

    #[test]
    fn the_workout_picker_reaches_every_plan() {
        let mut shell = Shell::new(false);
        let mut rows = vec![row("Free ride", "")];
        let mut ids = vec![None];
        for index in 0..15 {
            rows.push(row(
                &format!("Workout {index}"),
                "45 min · 58 TSS · IF 0.88 · 540 kJ",
            ));
            ids.push(Some(format!("w{index}")));
        }
        shell.set_workouts("Free ride", rows, ids);
        for _ in 0..5 {
            shell.handle(Command::Down, true, &snap(), &[]);
        }
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::None
        ));
        assert_eq!(shell.screen(), Screen::Workout);
        for _ in 0..15 {
            shell.handle(Command::Down, true, &snap(), &[]);
        }
        let view = shell.view("", "", "", &[], "", "");
        assert_eq!(view.rows.len(), 16);
        assert_eq!(view.selected, 15);
        assert!(view.rows[15].detail.contains("TSS"));
        assert!(view.rows[15].detail.contains("IF"));
        assert!(view.rows[15].detail.contains("kJ"));
        assert!(matches!(
            shell.handle(Command::Accept, true, &snap(), &[]),
            ShellEffect::PickWorkout(Some(id)) if id == "w14"
        ));
    }

    fn course_cards(count: usize) -> Vec<CourseCard> {
        (0..count)
            .map(|index| CourseCard {
                name: format!("Course {index}"),
                length_m: 5_000.0,
                ascent_m: 120.0,
                max_grade_pct: 8.0,
                profile: vec![(0.0, 0.0), (5_000.0, 40.0)],
                rerollable: index > 0,
            })
            .collect()
    }
}
