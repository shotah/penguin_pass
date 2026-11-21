//! How a saved ride is named on the summary screen.
//!
//! The file name stays whole. The folder is shortened with `~` and, when it
//! is wider than the line, cut out of the middle.

use std::path::Path;

/// How many characters of the folder path fit on the summary line.
pub const FOLDER_CHARS: usize = 42;

/// One line under the summary figures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RideFileNote {
    /// `Saved  name.fit`, or the reason the file was not written.
    pub headline: String,
    /// Folder, shortened and elided. Empty when the ride was not saved.
    pub folder: String,
    /// `true` when a file was written.
    pub saved: bool,
}

/// The summary line for a file that was written.
#[must_use]
pub fn saved_note(path: &Path, home: Option<&Path>, folder_chars: usize) -> RideFileNote {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let folder = path
        .parent()
        .map(|dir| elide_middle(&shorten_home(dir, home), folder_chars))
        .unwrap_or_default();
    RideFileNote {
        headline: format!("Saved  {name}"),
        folder,
        saved: true,
    }
}

/// The summary line when the ride was not written.
#[must_use]
pub fn unsaved_note(reason: &str) -> RideFileNote {
    RideFileNote {
        headline: reason.to_owned(),
        folder: String::new(),
        saved: false,
    }
}

/// Replace a home-directory prefix with `~`.
#[must_use]
pub fn shorten_home(path: &Path, home: Option<&Path>) -> String {
    let Some(home) = home.filter(|home| !home.as_os_str().is_empty()) else {
        return path.display().to_string();
    };
    if path == home {
        return "~".to_owned();
    }
    if let Ok(rest) = path.strip_prefix(home) {
        let tail = rest.display().to_string();
        if tail.is_empty() {
            return "~".to_owned();
        }
        return format!("~/{tail}");
    }
    path.display().to_string()
}

/// Keep the start and the end. The cut is a single `…`.
///
/// A `max_chars` of 0, or a string that already fits, is returned unchanged
/// when it fits. Shorter than two characters, the result is just `…`.
#[must_use]
pub fn elide_middle(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_owned();
    }
    if max_chars < 2 {
        return "…".to_owned();
    }
    let keep = max_chars - 1;
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[chars.len() - tail..].iter());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_saved_ride_shows_the_file_name_and_a_tilde_folder() {
        let path = PathBuf::from("/home/deck/rides/2026-11-20-2130-penguin-pass.fit");
        let note = saved_note(&path, Some(Path::new("/home/deck")), 80);
        assert_eq!(note.headline, "Saved  2026-11-20-2130-penguin-pass.fit");
        assert_eq!(note.folder, "~/rides");
        assert!(note.saved);
    }

    #[test]
    fn a_long_folder_is_cut_in_the_middle_and_keeps_both_ends() {
        let path =
            PathBuf::from("/home/deck/.local/share/godot/app_userdata/Penguin Pass/rides/ride.fit");
        let note = saved_note(&path, Some(Path::new("/home/deck")), 24);
        assert!(note.folder.starts_with("~/.local"));
        assert!(note.folder.ends_with("rides"));
        assert!(note.folder.contains('…'));
        assert!(note.folder.chars().count() <= 24);
        assert_eq!(elide_middle("short", 20), "short");
    }

    #[test]
    fn a_path_outside_home_stays_absolute() {
        let shown = shorten_home(
            Path::new("/tmp/pp-saved/rides"),
            Some(Path::new("/home/deck")),
        );
        assert_eq!(shown, "/tmp/pp-saved/rides");
    }

    #[test]
    fn an_unsaved_ride_is_the_reason_and_has_no_folder() {
        let note = unsaved_note("Ride too short to save");
        assert_eq!(note.headline, "Ride too short to save");
        assert!(note.folder.is_empty());
        assert!(!note.saved);
    }
}
