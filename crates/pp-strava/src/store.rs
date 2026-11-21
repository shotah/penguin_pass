//! The Strava TOML file: keys, tokens, and uploaded file names.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const HEADER: &str = "\
# Penguin Pass Strava connection.
#
# client_secret, access_token, and refresh_token are secrets.
# The file is readable and writable by this user only.
";

/// Why the store file could not be replaced.
#[derive(Debug)]
pub(crate) enum StoreError {
    CreateDir(std::io::Error),
    Write(std::io::Error),
    Rename(std::io::Error),
    Encode(toml::ser::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateDir(err) => {
                write!(formatter, "could not create the store directory: {err}")
            }
            Self::Write(err) => write!(formatter, "could not write the store: {err}"),
            Self::Rename(err) => write!(formatter, "could not replace the store: {err}"),
            Self::Encode(err) => write!(formatter, "could not encode the store: {err}"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct UploadRow {
    pub(crate) file: String,
    pub(crate) activity_id: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Store {
    #[serde(default)]
    pub(crate) client_id: String,
    #[serde(default)]
    pub(crate) client_secret: String,
    #[serde(default)]
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: String,
    #[serde(default)]
    pub(crate) expires_at: u64,
    #[serde(default)]
    pub(crate) athlete: String,
    #[serde(default)]
    pub(crate) uploads: Vec<UploadRow>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("athlete", &self.athlete)
            .field("uploads", &self.uploads.len())
            .finish()
    }
}

impl Store {
    pub(crate) fn has_keys(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    pub(crate) fn has_tokens(&self) -> bool {
        !self.access_token.is_empty() && !self.refresh_token.is_empty()
    }

    pub(crate) fn clear_tokens(&mut self) {
        self.access_token.clear();
        self.refresh_token.clear();
        self.expires_at = 0;
        self.athlete.clear();
    }

    pub(crate) fn remember_upload(&mut self, file: String, activity_id: u64) {
        if let Some(row) = self.uploads.iter_mut().find(|row| row.file == file) {
            row.activity_id = activity_id;
        } else {
            self.uploads.push(UploadRow { file, activity_id });
        }
    }

    pub(crate) fn was_uploaded(&self, file: &str) -> bool {
        self.uploads.iter().any(|row| row.file == file)
    }
}

/// Read the store. A missing file is empty. A file that does not parse is
/// empty and reported as corrupt; the bytes on disk are left as they are.
pub(crate) fn load(path: &Path) -> (Store, bool) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return (Store::default(), false),
    };
    if text.trim().is_empty() {
        return (Store::default(), false);
    }
    match toml::from_str(&text) {
        Ok(store) => (store, false),
        Err(_) => (Store::default(), true),
    }
}

pub(crate) fn save(path: &Path, store: &Store) -> Result<(), StoreError> {
    let mut body = String::from(HEADER);
    body.push_str(&toml::to_string_pretty(store).map_err(StoreError::Encode)?);
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(StoreError::CreateDir)?;
    }
    let tmp = temp_path(path);
    match write_private(&tmp, body.as_bytes()) {
        Ok(()) => {}
        Err(err) => {
            let _ = fs::remove_file(&tmp);
            return Err(err);
        }
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(StoreError::Rename(err));
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(StoreError::Write)?;
    file.write_all(bytes).map_err(StoreError::Write)?;
    file.sync_all().map_err(StoreError::Write)?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}
