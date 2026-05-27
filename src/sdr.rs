use std::{fs, path::PathBuf};

use chrono::Local;

use crate::data::{LoadedConfig, Pop, SdrConfig};

const SDR_URL: &str = "https://api.steampowered.com/ISteamApps/GetSDRConfig/v1?appid=730";

pub fn fetch_live_config() -> Result<(LoadedConfig, String), String> {
    let response = reqwest::blocking::Client::builder()
        .user_agent("cs2-server-chooser/0.1")
        .build()
        .map_err(|err| err.to_string())?
        .get(SDR_URL)
        .send()
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .text()
        .map_err(|err| err.to_string())?;

    Ok((parse_config(&response, "live Valve SDR config")?, response))
}

pub fn load_cache() -> Result<LoadedConfig, String> {
    let json = fs::read_to_string(cache_path()).map_err(|err| err.to_string())?;
    parse_config(&json, "cached Valve SDR config")
}

pub fn save_cache(json: &str) -> Result<(), String> {
    let path = cache_path();
    let parent = path
        .parent()
        .ok_or_else(|| "Cache path has no parent".to_owned())?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    fs::write(path, json).map_err(|err| err.to_string())
}

fn parse_config(json: &str, source: &str) -> Result<LoadedConfig, String> {
    let raw: SdrConfig = serde_json::from_str(json).map_err(|err| err.to_string())?;
    let mut pops: Vec<_> = raw
        .pops
        .into_iter()
        .filter_map(|(code, raw_pop)| {
            let [lon, lat] = raw_pop.geo?;
            Some(Pop {
                code,
                desc: raw_pop.desc,
                lon,
                lat,
                tier: raw_pop.tier.unwrap_or(9),
                relays: raw_pop.relays.unwrap_or_default(),
                selected: false,
            })
        })
        .collect();
    pops.sort_by(|a, b| a.desc.cmp(&b.desc));

    Ok(LoadedConfig {
        revision: raw.revision,
        fetched_at: Local::now(),
        source: source.to_owned(),
        live: source.starts_with("live "),
        pops,
    })
}

fn cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("cs2-server-chooser").join("sdr_config.json")
}
