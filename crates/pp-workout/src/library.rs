//! The built-in list and a folder of `.zwo` files.

use std::fs;
use std::io;
use std::path::Path;

use crate::builtin;
use crate::model::Workout;
use crate::zwo::{self, ZwoError};

/// A workout file that was skipped, or a step inside one that was skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadWarning {
    /// File name, when the warning belongs to one file.
    pub file: Option<String>,
    /// What was skipped, in plain text.
    pub message: String,
}

/// Workouts the rider can pick, plus anything the loader skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// Plans, in list order.
    pub workouts: Vec<Workout>,
    /// Skipped files and skipped steps. Empty for the built-in list.
    pub warnings: Vec<LoadWarning>,
}

impl Library {
    /// The ten workouts shipped with the game.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            workouts: builtin::workouts(),
            warnings: Vec::new(),
        }
    }

    /// Read every `.zwo` file in `path`. The search is not recursive.
    ///
    /// Files are taken in file-name order. A file that is not XML, or whose
    /// root is not `workout_file`, is skipped and named in [`Self::warnings`].
    /// The other files are still returned. A file that is not `.zwo` is ignored.
    ///
    /// # Errors
    ///
    /// Returns an error when `path` cannot be read as a directory. A single
    /// unreadable file becomes a warning instead.
    pub fn load_dir(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let mut names = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();
            if !file_path.is_file() || !is_zwo(&file_path) {
                continue;
            }
            if let Some(name) = file_path.file_name().and_then(|name| name.to_str()) {
                names.push(name.to_owned());
            }
        }
        names.sort();
        let mut workouts = Vec::new();
        let mut warnings = Vec::new();
        for name in names {
            let file_path = path.join(&name);
            let text = match fs::read_to_string(&file_path) {
                Ok(text) => text,
                Err(err) => {
                    warnings.push(LoadWarning {
                        file: Some(name),
                        message: format!("could not read file: {err}"),
                    });
                    continue;
                }
            };
            match zwo::parse_zwo(&name, &text) {
                Ok(parsed) => {
                    warnings.extend(parsed.warnings);
                    workouts.push(parsed.workout);
                }
                Err(ZwoError::Xml(detail)) => warnings.push(LoadWarning {
                    file: Some(name),
                    message: format!("malformed xml: {detail}"),
                }),
                Err(ZwoError::NotAWorkout) => warnings.push(LoadWarning {
                    file: Some(name),
                    message: "root element is not workout_file".to_owned(),
                }),
            }
        }
        Ok(Self { workouts, warnings })
    }
}

fn is_zwo(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zwo"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_dir_reads_zwo_files_and_keeps_going() {
        let dir = std::env::temp_dir().join(format!("pp-workout-load-{}", std::process::id()));
        let _guard = DirGuard(dir.clone());
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("b-sample.zwo"),
            r#"<workout_file>
                <name>Second</name>
                <workout><SteadyState Duration="60" Power="0.70"/></workout>
               </workout_file>"#,
        )
        .expect("write");
        fs::write(
            dir.join("a-evening.ZWO"),
            "<workout_file><workout><FreeRide Duration=\"45\"/></workout></workout_file>",
        )
        .expect("write");
        fs::write(dir.join("bad.zwo"), "<workout_file><workout></workout").expect("write");
        fs::write(dir.join("notes.txt"), "not a workout").expect("write");
        let nested = dir.join("nested");
        fs::create_dir_all(&nested).expect("nested");
        fs::write(
            nested.join("hidden.zwo"),
            "<workout_file><workout><FreeRide Duration=\"10\"/></workout></workout_file>",
        )
        .expect("write nested");

        let library = Library::load_dir(&dir).expect("load");
        assert_eq!(library.workouts.len(), 2);
        assert_eq!(library.workouts[0].id, "a-evening");
        assert_eq!(library.workouts[0].name, "a-evening");
        assert!((library.workouts[0].duration_s() - 45.0).abs() < 1e-3);
        assert_eq!(library.workouts[1].name, "Second");
        assert!(library.warnings.iter().any(|warning| {
            warning.file.as_deref() == Some("bad.zwo") && warning.message.contains("malformed")
        }));
        assert!(!library.warnings.iter().any(|warning| {
            warning
                .file
                .as_deref()
                .is_some_and(|file| file.contains("hidden") || file.contains("notes"))
        }));

        let missing = dir.join("no-such-folder");
        assert!(Library::load_dir(&missing).is_err());
    }

    struct DirGuard(std::path::PathBuf);

    impl Drop for DirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
