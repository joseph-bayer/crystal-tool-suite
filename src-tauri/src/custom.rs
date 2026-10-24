//! Custom tools: tools the suite doesn't ship, added by the user. Each is a web page, opened in
//! its own launcher window, or a program on this computer, which the launcher starts and then
//! leaves alone. They're kept in `<app data>/custom-tools.json`, their thumbnails copied into
//! `<app data>/thumbnails/`.

use crate::catalog::{self, SuggestedTool};
use crate::images;
use crate::jsonfile;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const CUSTOM_TOOLS_FILE: &str = "custom-tools.json";
const THUMBNAILS_DIR: &str = "thumbnails";
const WINDOW_PREFIX: &str = "custom-";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
	Web,
	Program,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomTool {
	pub id: String,
	pub name: String,
	pub kind: Kind,
	/// A web page tool's address.
	#[serde(default)]
	pub url: Option<String>,
	/// A program tool's file.
	#[serde(default)]
	pub program: Option<String>,
	/// A program's arguments, as one command-line string.
	#[serde(default)]
	pub args: Option<String>,
	/// The thumbnail's file name in the thumbnails folder.
	#[serde(default)]
	pub thumbnail: Option<String>,
	/// Where a program came from, for a suggested one.
	#[serde(default)]
	pub homepage: Option<String>,
	/// The suite index entry it was added from, if any.
	#[serde(default)]
	pub suggestion: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
	version: u32,
	tools: Vec<CustomTool>,
}

/// What the page sends to add or change a custom tool.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
	/// Absent for a new tool.
	pub id: Option<String>,
	pub name: String,
	pub kind: Kind,
	pub url: Option<String>,
	pub program: Option<String>,
	pub args: Option<String>,
	pub thumbnail: ThumbnailChange,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum ThumbnailChange {
	Keep,
	Remove,
	/// Copy this image file in.
	Set {
		path: String,
	},
}

pub fn file(app: &AppHandle) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(CUSTOM_TOOLS_FILE)
}

fn thumbnails_dir(app: &AppHandle) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(THUMBNAILS_DIR)
}

pub fn thumbnail_path(app: &AppHandle, tool: &CustomTool) -> Option<PathBuf> {
	tool.thumbnail
		.as_ref()
		// Only a plain file name, so it stays in the folder.
		.filter(|t| Path::new(t).file_name().is_some_and(|f| f == t.as_str()))
		.map(|t| thumbnails_dir(app).join(t))
}

pub fn list(app: &AppHandle) -> Result<Vec<CustomTool>, String> {
	let Some(value) = jsonfile::read(&file(app))? else {
		return Ok(Vec::new());
	};
	let stored: Stored = serde_json::from_value(value)
		.map_err(|e| format!("{} isn't valid: {e}", file(app).display()))?;
	Ok(stored.tools)
}

fn find(app: &AppHandle, id: &str) -> Result<CustomTool, String> {
	list(app)?
		.into_iter()
		.find(|t| t.id == id)
		.ok_or_else(|| "that custom tool isn't there any more".to_string())
}

/// Changes the stored list, re-reading it first.
fn change(app: &AppHandle, mut f: impl FnMut(&mut Vec<CustomTool>)) -> Result<(), String> {
	// A file the launcher can't read is reported, not rewritten.
	list(app)?;
	let path = file(app);
	let mut failure = None;
	jsonfile::update(
		&path,
		&json!({ "version": 1, "tools": [] }),
		|value| {
			let mut stored: Stored = match serde_json::from_value(value.clone()) {
				Ok(s) => s,
				Err(e) => {
					// Written back as it was.
					failure = Some(format!("{} isn't valid: {e}", path.display()));
					return value;
				}
			};
			failure = None;
			stored.version = 1;
			f(&mut stored.tools);
			serde_json::to_value(stored).unwrap_or(value)
		},
		false,
	)?;
	match failure {
		Some(e) => Err(e),
		None => Ok(()),
	}
}

fn new_id() -> String {
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	format!("{nanos:x}")
}

fn check_web_url(url: &str) -> Result<String, String> {
	let url = url.trim();
	let parsed = reqwest::Url::parse(url).map_err(|_| format!("\"{url}\" isn't a web address"))?;
	if !catalog::web_page(&parsed) {
		return Err(format!(
			"\"{url}\" isn't a web page address (http:// or https://)"
		));
	}
	Ok(parsed.to_string())
}

fn check_program(path: &str) -> Result<String, String> {
	let path = path.trim();
	if path.is_empty() {
		return Err("pick the program to start".into());
	}
	if !Path::new(path).is_file() {
		return Err(format!("{path} isn't a file"));
	}
	Ok(path.to_string())
}

/// Copies an image in as a tool's thumbnail, returning its file name there.
fn copy_thumbnail(app: &AppHandle, id: &str, from: &Path) -> Result<String, String> {
	let ext = images::check(from)?;
	let dir = thumbnails_dir(app);
	fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
	// A new name each time, so the old one can go once the list points at the new one.
	let name = format!("{id}-{}.{ext}", new_id());
	fs::copy(from, dir.join(&name))
		.map_err(|e| format!("couldn't copy {}: {e}", from.display()))?;
	Ok(name)
}

fn remove_thumbnail(app: &AppHandle, tool: &CustomTool) {
	if let Some(path) = thumbnail_path(app, tool) {
		let _ = fs::remove_file(path);
	}
}

/// Adds a custom tool, or changes one.
pub fn save(app: &AppHandle, edit: Edit) -> Result<(), String> {
	let name = edit.name.trim().to_string();
	if name.is_empty() {
		return Err("give it a name".into());
	}
	let (url, program, args) = match edit.kind {
		Kind::Web => (
			Some(check_web_url(edit.url.as_deref().unwrap_or(""))?),
			None,
			None,
		),
		Kind::Program => (
			None,
			Some(check_program(edit.program.as_deref().unwrap_or(""))?),
			edit.args
				.map(|a| a.trim().to_string())
				.filter(|a| !a.is_empty()),
		),
	};
	let old = match &edit.id {
		Some(id) => Some(find(app, id)?),
		None => None,
	};
	let id = edit.id.clone().unwrap_or_else(new_id);
	let thumbnail = match &edit.thumbnail {
		ThumbnailChange::Keep => old.as_ref().and_then(|o| o.thumbnail.clone()),
		ThumbnailChange::Remove => None,
		ThumbnailChange::Set { path } => Some(copy_thumbnail(app, &id, Path::new(path))?),
	};
	let tool = CustomTool {
		id: id.clone(),
		name,
		kind: edit.kind,
		url,
		program,
		args,
		thumbnail: thumbnail.clone(),
		homepage: old.as_ref().and_then(|o| o.homepage.clone()),
		suggestion: old.as_ref().and_then(|o| o.suggestion.clone()),
	};
	let mut found = edit.id.is_none();
	let result = change(app, |tools| match tools.iter_mut().find(|t| t.id == id) {
		Some(t) => {
			*t = tool.clone();
			found = true;
		}
		None if edit.id.is_none() => tools.push(tool.clone()),
		None => {}
	});
	let kept_new_thumbnail = result.is_ok() && found;
	match (&edit.thumbnail, kept_new_thumbnail) {
		// The new image is unused if the save failed.
		(ThumbnailChange::Set { .. }, false) => {
			if let Some(t) = thumbnail {
				let _ = fs::remove_file(thumbnails_dir(app).join(t));
			}
		}
		// The old image is unused once the change is saved.
		(ThumbnailChange::Set { .. } | ThumbnailChange::Remove, true) => {
			if let Some(old) = &old {
				remove_thumbnail(app, old);
			}
		}
		_ => {}
	}
	result?;
	if !found {
		return Err("that custom tool isn't there any more".into());
	}
	Ok(())
}

pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
	let tool = find(app, id)?;
	change(app, |tools| tools.retain(|t| t.id != id))?;
	remove_thumbnail(app, &tool);
	if let Some(window) = app.get_webview_window(&window_label(id)) {
		let _ = window.close();
	}
	Ok(())
}

/// Adds a suggested tool from the suite index. A program needs the file the user picked.
pub async fn add_suggested(
	app: &AppHandle,
	suggestion: &SuggestedTool,
	program: Option<String>,
) -> Result<(), String> {
	let id = new_id();
	let tool = CustomTool {
		id: id.clone(),
		name: suggestion.name.clone(),
		kind: suggestion.kind,
		url: match suggestion.kind {
			Kind::Web => Some(check_web_url(suggestion.url.as_deref().unwrap_or(""))?),
			Kind::Program => None,
		},
		program: match suggestion.kind {
			Kind::Web => None,
			Kind::Program => Some(check_program(program.as_deref().unwrap_or(""))?),
		},
		args: None,
		thumbnail: match &suggestion.icon {
			// An icon that can't be fetched leaves the placeholder; the tool is still added.
			Some(icon) => download_icon(app, &id, icon).await.ok(),
			None => None,
		},
		homepage: suggestion.homepage.clone(),
		suggestion: Some(suggestion.id.clone()),
	};
	let thumbnail = tool.clone();
	change(app, |tools| tools.push(tool.clone())).inspect_err(|_| {
		remove_thumbnail(app, &thumbnail);
	})
}

async fn download_icon(app: &AppHandle, id: &str, url: &str) -> Result<String, String> {
	let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
	let ext = parsed
		.path_segments()
		.and_then(|mut s| s.next_back())
		.and_then(|f| images::extension(Path::new(f)))
		.ok_or("the icon isn't a known image type")?;
	let bytes = catalog::get(url)
		.await?
		.bytes()
		.await
		.map_err(|e| e.to_string())?;
	if bytes.len() as u64 > images::MAX_IMAGE_BYTES {
		return Err("the icon is too large".into());
	}
	let dir = thumbnails_dir(app);
	fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
	let name = format!("{id}-{}.{ext}", new_id());
	fs::write(dir.join(&name), &bytes).map_err(|e| e.to_string())?;
	Ok(name)
}

pub fn window_label(id: &str) -> String {
	format!("{WINDOW_PREFIX}{id}")
}

/// Opens a web page tool in its window (or brings its window forward), or starts a program.
pub fn open(app: &AppHandle, id: &str) -> Result<(), String> {
	let tool = find(app, id)?;
	match tool.kind {
		Kind::Web => {
			let label = window_label(id);
			if let Some(window) = app.get_webview_window(&label) {
				let _ = window.show();
				let _ = window.unminimize();
				let _ = window.set_focus();
				return Ok(());
			}
			let url = check_web_url(tool.url.as_deref().unwrap_or(""))?;
			// Like the suite's tool windows, it's given no access to the launcher.
			WebviewWindowBuilder::new(
				app,
				label,
				WebviewUrl::External(url.parse().map_err(|e| format!("bad URL: {e}"))?),
			)
			.title(&tool.name)
			.inner_size(1280.0, 800.0)
			.build()
			.map_err(|e| format!("couldn't open {}'s window: {e}", tool.name))?;
			Ok(())
		}
		Kind::Program => {
			let program = check_program(tool.program.as_deref().unwrap_or(""))?;
			start_program(Path::new(&program), tool.args.as_deref().unwrap_or(""))
				.map_err(|e| format!("couldn't start {}: {e}", tool.name))
		}
	}
}

/// Starts a program the way Explorer would, from its own folder, and lets it run on its own:
/// the launcher doesn't watch it or stop it.
#[cfg(windows)]
fn start_program(program: &Path, args: &str) -> Result<(), String> {
	use std::os::windows::ffi::OsStrExt;
	use windows_sys::Win32::UI::Shell::ShellExecuteW;
	use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

	let wide = |s: &std::ffi::OsStr| s.encode_wide().chain([0]).collect::<Vec<u16>>();
	let verb = wide("open".as_ref());
	let file = wide(program.as_os_str());
	let params = wide(args.as_ref());
	let dir = wide(program.parent().unwrap_or(Path::new(".")).as_os_str());
	// SAFETY: every string is NUL-terminated and outlives the call.
	let result = unsafe {
		ShellExecuteW(
			std::ptr::null_mut(),
			verb.as_ptr(),
			file.as_ptr(),
			if args.is_empty() {
				std::ptr::null()
			} else {
				params.as_ptr()
			},
			dir.as_ptr(),
			SW_SHOWNORMAL,
		)
	};
	// Values above 32 mean it started.
	if result as isize > 32 {
		Ok(())
	} else {
		Err(std::io::Error::last_os_error().to_string())
	}
}

#[cfg(not(windows))]
fn start_program(program: &Path, args: &str) -> Result<(), String> {
	std::process::Command::new(program)
		.args(args.split_whitespace())
		.current_dir(program.parent().unwrap_or(Path::new(".")))
		.stdin(std::process::Stdio::null())
		.spawn()
		.map(|_| ())
		.map_err(|e| e.to_string())
}
