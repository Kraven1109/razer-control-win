/// Persistent GUI settings stored in %APPDATA%\razercontrol\gui.json.
///
/// These are GUI-only features that don't need the daemon to persist —
/// gaming mode, display refresh rate switching, low-battery lighting, etc.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

fn config_dir() -> PathBuf {
    let appdata =
        std::env::var("APPDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string());
    PathBuf::from(appdata).join("razercontrol")
}

fn gui_config_path() -> PathBuf {
    config_dir().join("gui.json")
}

#[derive(Serialize, Deserialize, Default)]
pub struct GuiConfig {
    #[serde(default)]
    pub gaming_win_key: bool,
    #[serde(default)]
    pub gaming_alt_tab: bool,
    #[serde(default)]
    pub gaming_alt_f4: bool,
    #[serde(default)]
    pub bat_low_refresh: bool,
    #[serde(default)]
    pub low_bat_lighting: bool,
    #[serde(default = "default_low_bat_pct")]
    pub low_bat_pct: u8,
}

fn default_low_bat_pct() -> u8 { 20 }

impl GuiConfig {
    pub fn load() -> Self {
        fs::read(gui_config_path())
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let dir = config_dir();
        fs::create_dir_all(&dir)?;
        let json =
            serde_json::to_string_pretty(self).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(gui_config_path(), json)
    }
}
