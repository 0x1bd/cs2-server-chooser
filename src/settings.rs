use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
struct SelectionSettings {
    allowed_pops: BTreeSet<String>,
}

pub fn load_allowed_pops() -> BTreeSet<String> {
    fs::read_to_string(selection_path())
        .ok()
        .and_then(|json| serde_json::from_str::<SelectionSettings>(&json).ok())
        .map(|settings| settings.allowed_pops)
        .unwrap_or_default()
}

pub fn save_allowed_pops(allowed_pops: BTreeSet<String>) -> Result<(), String> {
    write_allowed_pops(&selection_path(), allowed_pops)
}

pub fn export_allowed_pops(path: &Path, allowed_pops: BTreeSet<String>) -> Result<(), String> {
    write_allowed_pops(path, allowed_pops)
}

pub fn import_allowed_pops(path: &Path) -> Result<BTreeSet<String>, String> {
    fs::read_to_string(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))
        .and_then(|json| {
            serde_json::from_str::<SelectionSettings>(&json)
                .map(|settings| settings.allowed_pops)
                .map_err(|err| format!("Failed to parse {}: {err}", path.display()))
        })
}

pub fn default_export_path() -> PathBuf {
    config_dir().join("selection-export.json")
}

fn write_allowed_pops(path: &Path, allowed_pops: BTreeSet<String>) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Selection path {} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let settings = SelectionSettings { allowed_pops };
    let json = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(path, json).map_err(|err| err.to_string())
}

pub fn config_dir() -> PathBuf {
    platform_config_base().join("cs2-server-chooser")
}

fn selection_path() -> PathBuf {
    config_dir().join("selection.json")
}

#[cfg(target_os = "windows")]
fn platform_config_base() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(target_os = "windows"))]
fn platform_config_base() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}
