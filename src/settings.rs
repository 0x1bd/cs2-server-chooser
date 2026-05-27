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
    let path = selection_path();
    let parent = path
        .parent()
        .ok_or_else(|| "Selection path has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let settings = SelectionSettings { allowed_pops };
    let json = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(path, json).map_err(|err| err.to_string())
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("cs2-server-chooser")
}

fn selection_path() -> PathBuf {
    config_dir().join("selection.json")
}
