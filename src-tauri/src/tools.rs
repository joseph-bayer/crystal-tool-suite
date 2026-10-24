//! Finding installed tools and reading their manifests.
//!
//! Installed tools live in `<app data>/tools/<tool>/`, each with a `manifest.json`. For
//! development, `<app data>/dev-tools.json` can list more tool folders (a JSON array of paths),
//! which are read the same way. An installed tool wins over a listed folder with the same id.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const MANIFEST_FILE: &str = "manifest.json";
const TOOLS_DIR: &str = "tools";
const DEV_TOOLS_FILE: &str = "dev-tools.json";

/// What a tool declares about itself.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
	pub id: String,
	pub name: String,
	pub version: String,
	#[serde(default)]
	pub description: String,
	/// An image file, relative to the tool's folder.
	#[serde(default)]
	pub icon: Option<String>,
	/// What to run, keyed by platform: `windows`, `macos` or `linux`.
	pub entry: HashMap<String, Entry>,
	/// The folder kinds the tool works with, e.g. `cse-project`.
	#[serde(default)]
	pub folders: Vec<String>,
	/// The oldest launcher version that can run the tool.
	#[serde(default)]
	pub min_launcher: Option<String>,
}

/// A program and its arguments. The launcher runs it from the tool's folder with `PORT` set.
#[derive(Deserialize, Clone, Debug)]
pub struct Entry {
	/// A path relative to the tool's folder, or a program name looked up on `PATH`.
	pub command: String,
	#[serde(default)]
	pub args: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Tool {
	pub folder: PathBuf,
	pub manifest: Manifest,
	/// Listed in `dev-tools.json` rather than installed, so the launcher doesn't update it.
	pub dev: bool,
}

impl Tool {
	pub fn entry(&self) -> &Entry {
		// Present: checked when the manifest was read.
		&self.manifest.entry[std::env::consts::OS]
	}

	/// The program to run: a file in the tool's folder if there is one by that name, else a
	/// program on `PATH`.
	/// The tool's icon file, if it names one inside its folder.
	pub fn icon_file(&self) -> Option<PathBuf> {
		let icon = Path::new(self.manifest.icon.as_deref()?);
		// Kept inside the tool's folder.
		let inside = icon
			.components()
			.all(|c| matches!(c, std::path::Component::Normal(_)));
		inside.then(|| self.folder.join(icon))
	}

	pub fn program(&self) -> PathBuf {
		let entry = self.entry();
		let local = self.folder.join(&entry.command);
		if local.is_file() {
			local
		} else {
			PathBuf::from(&entry.command)
		}
	}
}

/// A tool folder that couldn't be used, and why.
#[derive(Clone, Debug)]
pub struct Problem {
	pub folder: PathBuf,
	pub message: String,
}

pub fn tools_dir(app: &AppHandle) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(TOOLS_DIR)
}

pub fn dev_tools_file(app: &AppHandle) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(DEV_TOOLS_FILE)
}

/// Every tool the launcher can run, plus the folders it couldn't use.
pub fn discover(app: &AppHandle) -> (Vec<Tool>, Vec<Problem>) {
	// Each folder, and whether it came from the dev list.
	let mut folders: Vec<(PathBuf, bool)> = Vec::new();
	let mut problems = Vec::new();

	let installed = tools_dir(app);
	if let Ok(entries) = fs::read_dir(&installed) {
		let mut found: Vec<PathBuf> = entries
			.filter_map(|e| e.ok())
			.map(|e| e.path())
			.filter(|p| p.is_dir())
			.collect();
		found.sort();
		folders.extend(found.into_iter().map(|f| (f, false)));
	}

	let dev_file = dev_tools_file(app);
	if dev_file.exists() {
		match fs::read_to_string(&dev_file)
			.map_err(|e| e.to_string())
			.and_then(|text| serde_json::from_str::<Vec<PathBuf>>(&text).map_err(|e| e.to_string()))
		{
			Ok(dev) => folders.extend(dev.into_iter().map(|f| (f, true))),
			Err(e) => problems.push(Problem {
				folder: dev_file.clone(),
				message: format!("couldn't read this list of tool folders: {e}"),
			}),
		}
	}

	let mut tools = Vec::new();
	let mut ids = HashSet::new();
	for (folder, dev) in folders {
		match read_tool(&folder, dev) {
			Ok(tool) if !ids.insert(tool.manifest.id.clone()) => problems.push(Problem {
				folder,
				message: format!(
					"another tool folder already has the id \"{}\"",
					tool.manifest.id
				),
			}),
			Ok(tool) => tools.push(tool),
			Err(message) => problems.push(Problem { folder, message }),
		}
	}
	(tools, problems)
}

pub fn find(app: &AppHandle, id: &str) -> Result<Tool, String> {
	discover(app)
		.0
		.into_iter()
		.find(|t| t.manifest.id == id)
		.ok_or_else(|| format!("no tool with the id \"{id}\" is installed"))
}

pub fn read_tool(folder: &Path, dev: bool) -> Result<Tool, String> {
	let path = folder.join(MANIFEST_FILE);
	let text =
		fs::read_to_string(&path).map_err(|e| format!("couldn't read {MANIFEST_FILE}: {e}"))?;
	let manifest: Manifest =
		serde_json::from_str(&text).map_err(|e| format!("{MANIFEST_FILE} isn't valid: {e}"))?;

	if !valid_id(&manifest.id) {
		return Err(format!(
			"the id \"{}\" must be lowercase letters, digits and dashes",
			manifest.id
		));
	}
	if !manifest.entry.contains_key(std::env::consts::OS) {
		return Err(format!(
			"{} has no entry for {}",
			manifest.name,
			std::env::consts::OS
		));
	}
	if let Some(min) = &manifest.min_launcher {
		let ours = env!("CARGO_PKG_VERSION");
		match (parse_version(min), parse_version(ours)) {
			(Some(min_v), Some(ours_v)) if min_v > ours_v => {
				return Err(format!(
					"{} needs launcher {min} or later; this is {ours}",
					manifest.name
				))
			}
			(None, _) => return Err(format!("minLauncher \"{min}\" isn't a version like 1.2.3")),
			_ => {}
		}
	}
	Ok(Tool {
		folder: folder.to_path_buf(),
		manifest,
		dev,
	})
}

/// Ids become file and window names, so they're kept to `a-z`, `0-9` and `-`.
pub fn valid_id(id: &str) -> bool {
	!id.is_empty()
		&& !id.starts_with('-')
		&& id
			.chars()
			.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `major.minor.patch`; anything after a `-` or `+` is ignored.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
	let core = v.split(['-', '+']).next()?;
	let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
	let version = (parts.next()??, parts.next()??, parts.next()??);
	parts.next().is_none().then_some(version)
}
