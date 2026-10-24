//! The launcher's own settings, in `<app data>/settings.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";

/// Placeholders until the suite's repos exist: the suite index, and the launcher's update feed
/// (the updater plugin's `latest.json`).
const DEFAULT_INDEX_URL: &str =
	"https://raw.githubusercontent.com/joseph-bayer/crystal-tool-suite/main/index.json";
const DEFAULT_LAUNCHER_UPDATE_URL: &str =
	"https://github.com/joseph-bayer/crystal-tool-suite/releases/latest/download/latest.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
	/// Where the list of the suite's tools is read from.
	pub index_url: String,
	/// Where the launcher looks for a newer version of itself.
	pub launcher_update_url: String,
}

impl Default for Settings {
	fn default() -> Self {
		Settings {
			index_url: DEFAULT_INDEX_URL.to_string(),
			launcher_update_url: DEFAULT_LAUNCHER_UPDATE_URL.to_string(),
		}
	}
}

pub fn settings_file(app: &AppHandle) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(SETTINGS_FILE)
}

/// The saved settings, or the defaults if there are none yet.
pub fn load(app: &AppHandle) -> Result<Settings, String> {
	let path = settings_file(app);
	if !path.exists() {
		return Ok(Settings::default());
	}
	let text =
		fs::read_to_string(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
	serde_json::from_str(&text).map_err(|e| format!("{} isn't valid: {e}", path.display()))
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
	let path = settings_file(app);
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir).map_err(|e| e.to_string())?;
	}
	let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
	fs::write(&path, text).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}
