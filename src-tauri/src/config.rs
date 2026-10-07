use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::{AppHandle, Manager};

use crate::types::Settings;

const SETTINGS_FILE: &str = "settings.json";

/// Where `settings.json` lives. Created on demand.
pub fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("could not resolve the config directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Where the SQLite database and blob files live. Created on demand.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("could not resolve the data directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn blobs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("blobs");
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(config_dir(app)?.join(SETTINGS_FILE))
}

/// Load settings, falling back to defaults.
///
/// A missing or unreadable file is not an error: the app must still start, so we
/// return defaults and leave the file alone. A *partially* written file is
/// repaired by `#[serde(default)]` on the struct.
pub fn load(app: &AppHandle) -> Settings {
    read_raw(app).unwrap_or_default()
}

fn read_raw(app: &AppHandle) -> Option<Settings> {
    let path = settings_path(app).ok()?;
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("could not serialise settings: {e}"))?;
    write_atomic(&path, json.as_bytes())
}

/// Write via a sibling temp file + rename so a crash mid-write cannot truncate
/// the settings file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not move {} -> {}: {e}", tmp.display(), path.display()))
}
