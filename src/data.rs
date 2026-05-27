use std::{collections::BTreeMap, net::Ipv4Addr};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct SdrConfig {
    pub revision: u64,
    pub pops: BTreeMap<String, RawPop>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawPop {
    pub desc: String,
    pub geo: Option<[f64; 2]>,
    pub tier: Option<u8>,
    pub relays: Option<Vec<Relay>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Relay {
    pub ipv4: Ipv4Addr,
    pub port_range: [u16; 2],
}

#[derive(Debug, Clone)]
pub struct Pop {
    pub code: String,
    pub desc: String,
    pub lon: f64,
    pub lat: f64,
    pub tier: u8,
    pub relays: Vec<Relay>,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub revision: u64,
    pub fetched_at: DateTime<Local>,
    pub source: String,
    pub live: bool,
    pub pops: Vec<Pop>,
}

impl LoadedConfig {
    pub fn selectable_count(&self) -> usize {
        self.pops
            .iter()
            .filter(|pop| !pop.relays.is_empty())
            .count()
    }

    pub fn allowed_count(&self) -> usize {
        self.pops
            .iter()
            .filter(|pop| pop.selected && !pop.relays.is_empty())
            .count()
    }

    pub fn blocked_count(&self) -> usize {
        self.selectable_count().saturating_sub(self.allowed_count())
    }

    pub fn relay_count(&self) -> usize {
        self.pops.iter().map(|pop| pop.relays.len()).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierFilter {
    All,
    ValvePrimary,
    ValveAny
}
