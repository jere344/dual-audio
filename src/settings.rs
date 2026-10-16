//! Settings persisted as JSON in the user's config directory
//! (`%AppData%\DualAudio` on Windows, `~/.config/DualAudio` on Linux).
//! The format is compatible with the original Python release.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "DualAudio";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeviceSettings {
    pub enabled: bool,
    pub volume: f32,
    pub delay_ms: u32,
}

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: 1.0,
            delay_ms: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Keyed by device name, so settings survive device id changes.
    pub devices: BTreeMap<String, DeviceSettings>,
    /// Name of the capture source; `None` means the system default output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default())
        .join(APP_ID)
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(settings_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(config_dir())?;
            let json = serde_json::to_string_pretty(self)?;
            std::fs::write(settings_path(), json)
        };
        if let Err(e) = write() {
            eprintln!("Cannot save settings: {e}");
        }
    }
}
