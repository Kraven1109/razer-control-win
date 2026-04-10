/// Persistent configuration stored in %APPDATA%\razercontrol\

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, prelude::*};
use std::path::PathBuf;

fn config_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA")
        .unwrap_or_else(|_| "C:\\ProgramData".to_string());
    PathBuf::from(appdata).join("razercontrol")
}

fn settings_path() -> PathBuf {
    config_dir().join("daemon.json")
}

fn effects_path() -> PathBuf {
    config_dir().join("effects.json")
}

// ── Power profile per AC state ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Copy, Clone)]
pub struct PowerConfig {
    pub power_mode: u8,
    pub cpu_boost: u8,
    pub gpu_boost: u8,
    pub fan_rpm: i32,
    pub brightness: u8,
    pub logo_state: u8,
    pub screensaver: bool,
    pub idle: u32,
    /// RAPL PL1 (sustained) — unused on Windows (no sysfs), kept for format
    /// compatibility with exported configs from the Linux build.
    #[serde(default)]
    pub rapl_pl1_watts: u32,
    #[serde(default)]
    pub rapl_pl2_watts: u32,
}

impl PowerConfig {
    pub fn new() -> PowerConfig {
        PowerConfig {
            power_mode: 0,
            cpu_boost: 1,
            gpu_boost: 0,
            fan_rpm: 0,
            brightness: 128,
            logo_state: 0,
            screensaver: false,
            idle: 0,
            rapl_pl1_watts: 0,
            rapl_pl2_watts: 0,
        }
    }
}

// ── Main configuration ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct Configuration {
    pub power: [PowerConfig; 2],
    pub sync: bool,
    pub no_light: f64,
    pub standard_effect: u8,
    pub standard_effect_params: Vec<u8>,
}

impl Configuration {
    pub fn new() -> Configuration {
        Configuration {
            power: [PowerConfig::new(), PowerConfig::new()],
            sync: false,
            no_light: 0.0,
            standard_effect: 0,
            standard_effect_params: vec![],
        }
    }

    pub fn read_from_config() -> io::Result<Configuration> {
        let data = fs::read(settings_path())?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn reset_fan_profiles_to_auto(&mut self) -> bool {
        let mut changed = false;
        for slot in &mut self.power {
            if slot.fan_rpm != 0 {
                slot.fan_rpm = 0;
                changed = true;
            }
        }
        changed
    }

    pub fn write_to_file(&self) -> io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;
        let mut f = fs::File::create(settings_path())?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        f.write_all(json.as_bytes())
    }

    pub fn read_effects_file() -> io::Result<serde_json::Value> {
        let data = fs::read(effects_path())?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn write_effects_save(json: serde_json::Value) -> io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;
        let mut f = fs::File::create(effects_path())?;
        let s = serde_json::to_string_pretty(&json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        f.write_all(s.as_bytes())
    }
}
