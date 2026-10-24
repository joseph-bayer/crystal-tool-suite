//! Installing, updating, rolling back and uninstalling tools from the suite index.
//!
//! A release is a zip with the tool's folder contents (`manifest.json` at its root). It is
//! downloaded into `<app data>/downloads/`, checked against its published SHA-256 before anything
//! is unpacked, unpacked beside it, and only then moved into `<app data>/tools/<id>/`. The
//! version it replaces moves to `<app data>/previous/<id>/`, the one earlier version kept for
//! rolling back. A tool that's running is never replaced: its update waits until it stops.

use crate::catalog::{self, Catalog, Download, Release};
use crate::runner::{Runner, TOOLS_CHANGED};
use crate::tools::{self, parse_version, Tool};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::future::Future;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const PREVIOUS_DIR: &str = "previous";
const DOWNLOADS_DIR: &str = "downloads";

/// What the installer is doing to each tool.
#[derive(Default)]
pub struct Installer {
	state: Mutex<State>,
}

#[derive(Default)]
struct State {
	/// Tools being worked on, and what's happening to each.
	tasks: HashMap<String, String>,
	/// Tools with an update waiting for them to stop.
	waiting: HashSet<String>,
	/// The last failure for each tool.
	errors: HashMap<String, String>,
}

impl Installer {
	pub fn task(&self, id: &str) -> Option<String> {
		self.state.lock().unwrap().tasks.get(id).cloned()
	}

	pub fn waiting(&self, id: &str) -> bool {
		self.state.lock().unwrap().waiting.contains(id)
	}

	pub fn error(&self, id: &str) -> Option<String> {
		self.state.lock().unwrap().errors.get(id).cloned()
	}

	pub fn busy(&self) -> bool {
		let state = self.state.lock().unwrap();
		!state.tasks.is_empty() || !state.waiting.is_empty()
	}
}

pub fn installed_dir(app: &AppHandle, id: &str) -> PathBuf {
	tools::tools_dir(app).join(id)
}

pub fn previous_dir(app: &AppHandle, id: &str) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(PREVIOUS_DIR)
		.join(id)
}

fn downloads_dir(app: &AppHandle) -> PathBuf {
	app.path()
		.app_data_dir()
		.unwrap_or_default()
		.join(DOWNLOADS_DIR)
}

/// The installed copy of a tool, if the launcher installed one.
pub fn installed(app: &AppHandle, id: &str) -> Option<Tool> {
	tools::read_tool(&installed_dir(app, id), false).ok()
}

/// The earlier version kept for rolling back.
pub fn kept(app: &AppHandle, id: &str) -> Option<Tool> {
	tools::read_tool(&previous_dir(app, id), false).ok()
}

/// The bytes a folder and everything in it take up.
pub fn disk_use(path: &Path) -> u64 {
	let Ok(entries) = fs::read_dir(path) else {
		return 0;
	};
	entries
		.filter_map(|e| e.ok())
		.map(|e| match e.file_type() {
			Ok(t) if t.is_dir() => disk_use(&e.path()),
			Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
			_ => 0,
		})
		.sum()
}

/// Whether the index lists a newer version than the installed one.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
	match (parse_version(candidate), parse_version(installed)) {
		(Some(c), Some(i)) => c > i,
		_ => false,
	}
}

/// Installs the index's latest release of a tool, or updates to it. If the tool is running, the
/// update waits until it stops. Returns `true` if it's waiting.
pub fn update(app: &AppHandle, id: &str) -> Result<bool, String> {
	let release = app
		.state::<Catalog>()
		.tool(id)
		.ok_or_else(|| format!("the suite index doesn't list \"{id}\""))?
		.latest;
	if installed(app, id).is_none() && tools::find(app, id).is_ok_and(|t| t.dev) {
		return Err(format!(
			"\"{id}\" is listed in dev-tools.json; remove it there to install it"
		));
	}
	if !begin(app, id, format!("Downloading {}", release.version))? {
		let installer = app.state::<Installer>();
		installer
			.state
			.lock()
			.unwrap()
			.waiting
			.insert(id.to_string());
		// It may have stopped in the meantime, with nothing left to start the update.
		if !begin(app, id, format!("Downloading {}", release.version))? {
			let _ = app.emit(TOOLS_CHANGED, ());
			return Ok(true);
		}
		installer.state.lock().unwrap().waiting.remove(id);
	}
	let id = id.to_string();
	spawn(app, id.clone(), move |app| async move {
		install_release(&app, &id, &release).await
	});
	Ok(false)
}

/// Goes back to the kept earlier version, keeping the one it replaces in its place. With no
/// kept version, installs the index's previous release instead.
pub fn roll_back(app: &AppHandle, id: &str) -> Result<(), String> {
	let current = installed(app, id).ok_or("it isn't installed")?;
	let from_index = if kept(app, id).is_some() {
		None
	} else {
		let previous = app
			.state::<Catalog>()
			.tool(id)
			.and_then(|t| t.previous)
			.filter(|p| is_newer(&current.manifest.version, &p.version))
			.ok_or("there's no earlier version to roll back to")?;
		Some(previous)
	};
	if !begin(app, id, "Rolling back".to_string())? {
		return Err(format!("close {} first", current.manifest.name));
	}
	let id = id.to_string();
	spawn(app, id.clone(), move |app| async move {
		match from_index {
			Some(release) => install_release(&app, &id, &release).await,
			None => blocking(&app, &id, swap_with_kept).await,
		}
	});
	Ok(())
}

/// Removes a tool and its kept earlier version. Its settings and logs, and anything it wrote
/// into projects, are left alone.
pub fn uninstall(app: &AppHandle, id: &str) -> Result<(), String> {
	let current = installed(app, id).ok_or("it isn't installed")?;
	if !begin(app, id, "Uninstalling".to_string())? {
		return Err(format!("close {} first", current.manifest.name));
	}
	let id = id.to_string();
	spawn(app, id.clone(), move |app| async move {
		blocking(&app, &id, |app, id| {
			remove_dir(&installed_dir(app, id))?;
			remove_dir(&previous_dir(app, id))
		})
		.await
	});
	Ok(())
}

/// Called whenever a tool stops: runs its waiting update, if any.
pub fn tool_stopped(app: &AppHandle, id: &str) {
	let was_waiting = app
		.state::<Installer>()
		.state
		.lock()
		.unwrap()
		.waiting
		.remove(id);
	if was_waiting {
		if let Err(e) = update(app, id) {
			record_error(app, id, e);
		}
	}
}

/// Marks a tool as being worked on and keeps it from starting. `Ok(false)` if it's running or
/// starting; an error if something else is already being done to it.
fn begin(app: &AppHandle, id: &str, task: String) -> Result<bool, String> {
	let installer = app.state::<Installer>();
	let mut state = installer.state.lock().unwrap();
	if state.tasks.contains_key(id) {
		return Err("it's already being installed or removed".to_string());
	}
	if !app.state::<Runner>().hold(id) {
		return Ok(false);
	}
	state.tasks.insert(id.to_string(), task);
	state.errors.remove(id);
	drop(state);
	let _ = app.emit(TOOLS_CHANGED, ());
	Ok(true)
}

/// Runs the work for a tool begun with `begin`, then lets it start again and records how it went.
fn spawn<F, Fut>(app: &AppHandle, id: String, work: F)
where
	F: FnOnce(AppHandle) -> Fut + Send + 'static,
	Fut: Future<Output = Result<(), String>> + Send + 'static,
{
	let app = app.clone();
	tauri::async_runtime::spawn(async move {
		let result = work(app.clone()).await;
		app.state::<Runner>().release(&id);
		let installer = app.state::<Installer>();
		let mut state = installer.state.lock().unwrap();
		state.tasks.remove(&id);
		if let Err(e) = result {
			state.errors.insert(id, e);
		}
		drop(state);
		let _ = app.emit(TOOLS_CHANGED, ());
	});
}

fn set_task(app: &AppHandle, id: &str, task: &str) {
	app.state::<Installer>()
		.state
		.lock()
		.unwrap()
		.tasks
		.insert(id.to_string(), task.to_string());
	let _ = app.emit(TOOLS_CHANGED, ());
}

fn record_error(app: &AppHandle, id: &str, message: String) {
	app.state::<Installer>()
		.state
		.lock()
		.unwrap()
		.errors
		.insert(id.to_string(), message);
	let _ = app.emit(TOOLS_CHANGED, ());
}

/// Runs file work off the async runtime.
async fn blocking(
	app: &AppHandle,
	id: &str,
	work: impl FnOnce(&AppHandle, &str) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
	let app = app.clone();
	let id = id.to_string();
	tauri::async_runtime::spawn_blocking(move || work(&app, &id))
		.await
		.map_err(|e| e.to_string())?
}

async fn install_release(app: &AppHandle, id: &str, release: &Release) -> Result<(), String> {
	let download = release.download().ok_or_else(|| {
		format!(
			"version {} has no download for {}",
			release.version,
			std::env::consts::OS
		)
	})?;
	let expected = expected_sha256(download).await?;

	let dir = downloads_dir(app);
	fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
	let archive = dir.join(format!("{id}-{}.zip", release.version));
	let actual = download_to(&download.url, &archive).await;
	let actual = match actual {
		Ok(sum) => sum,
		Err(e) => {
			let _ = fs::remove_file(&archive);
			return Err(e);
		}
	};
	if actual != expected {
		let _ = fs::remove_file(&archive);
		return Err(format!(
			"the download's checksum doesn't match the published one, so nothing was installed (expected {expected}, got {actual})"
		));
	}

	set_task(app, id, &format!("Installing {}", release.version));
	let version = release.version.clone();
	blocking(app, id, move |app, id| {
		let staging = downloads_dir(app).join(format!("{id}-new"));
		let result = unpack(&archive, &staging)
			.and_then(|()| check_unpacked(&staging, id, &version))
			.and_then(|()| move_into_place(app, id, &staging));
		let _ = fs::remove_file(&archive);
		let _ = fs::remove_dir_all(&staging);
		result
	})
	.await
}

/// The SHA-256 published for a download, as lowercase hex.
async fn expected_sha256(download: &Download) -> Result<String, String> {
	let sum = match (&download.sha256, &download.sha256sums_url) {
		(Some(sum), _) => sum.trim().to_ascii_lowercase(),
		(None, Some(sums_url)) => {
			let text = catalog::get(sums_url)
				.await?
				.text()
				.await
				.map_err(|e| format!("couldn't read {sums_url}: {e}"))?;
			let name = reqwest::Url::parse(&download.url)
				.ok()
				.and_then(|u| u.path_segments()?.next_back().map(str::to_string))
				.unwrap_or_default();
			sum_for(&text, &name).ok_or_else(|| {
				format!("{sums_url} has no checksum for {name}, so it wasn't installed")
			})?
		}
		(None, None) => {
			return Err("no checksum is published for this download, so it wasn't installed".into())
		}
	};
	if sum.len() != 64 || !sum.bytes().all(|b| b.is_ascii_hexdigit()) {
		return Err(format!(
			"the published checksum \"{sum}\" isn't a SHA-256, so it wasn't installed"
		));
	}
	Ok(sum)
}

/// The checksum for `name` in a `SHA256SUMS.txt` (`<hex>  <name>` or `<hex> *<name>` lines).
fn sum_for(text: &str, name: &str) -> Option<String> {
	text.lines().find_map(|line| {
		let (sum, file) = line.trim().split_once(char::is_whitespace)?;
		let file = file.trim_start();
		let file = file.strip_prefix('*').unwrap_or(file);
		(file == name).then(|| sum.to_ascii_lowercase())
	})
}

/// Saves a download to a file and returns its SHA-256.
async fn download_to(url: &str, path: &Path) -> Result<String, String> {
	let response = catalog::get(url).await?;
	let mut file = File::create(path).map_err(|e| format!("couldn't save the download: {e}"))?;
	let mut hasher = Sha256::new();
	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk.map_err(|e| format!("the download from {url} failed: {e}"))?;
		hasher.update(&chunk);
		file.write_all(&chunk)
			.map_err(|e| format!("couldn't save the download: {e}"))?;
	}
	file.flush().map_err(|e| e.to_string())?;
	Ok(hasher
		.finalize()
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect())
}

fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
	let bad = |e: &dyn std::fmt::Display| format!("the download isn't a usable zip: {e}");
	if into.exists() {
		fs::remove_dir_all(into).map_err(|e| e.to_string())?;
	}
	fs::create_dir_all(into).map_err(|e| e.to_string())?;
	let file = File::open(archive).map_err(|e| e.to_string())?;
	let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(&e))?;
	for i in 0..zip.len() {
		let mut entry = zip.by_index(i).map_err(|e| bad(&e))?;
		// Refuses names that would land outside the folder.
		let name = entry
			.enclosed_name()
			.ok_or_else(|| bad(&format!("it has an unsafe path \"{}\"", entry.name())))?;
		let out = into.join(name);
		if entry.is_dir() {
			fs::create_dir_all(&out).map_err(|e| e.to_string())?;
			continue;
		}
		if let Some(parent) = out.parent() {
			fs::create_dir_all(parent).map_err(|e| e.to_string())?;
		}
		let mut target = File::create(&out).map_err(|e| e.to_string())?;
		io::copy(&mut entry, &mut target).map_err(|e| bad(&e))?;
	}
	Ok(())
}

/// The unpacked folder must be the tool and version the index promised.
fn check_unpacked(folder: &Path, id: &str, version: &str) -> Result<(), String> {
	let tool = tools::read_tool(folder, false)
		.map_err(|e| format!("the downloaded tool can't be used: {e}"))?;
	if tool.manifest.id != id {
		return Err(format!(
			"the download is the tool \"{}\", not \"{id}\"",
			tool.manifest.id
		));
	}
	if tool.manifest.version != version {
		return Err(format!(
			"the download is version {}, not {version}",
			tool.manifest.version
		));
	}
	Ok(())
}

/// Puts an unpacked tool in place, moving the installed version (if any) to be the kept one.
fn move_into_place(app: &AppHandle, id: &str, staging: &Path) -> Result<(), String> {
	let current = installed_dir(app, id);
	let previous = previous_dir(app, id);
	fs::create_dir_all(tools::tools_dir(app)).map_err(|e| e.to_string())?;
	let had_current = current.exists();
	if had_current {
		remove_dir(&previous)?;
		if let Some(parent) = previous.parent() {
			fs::create_dir_all(parent).map_err(|e| e.to_string())?;
		}
		rename(&current, &previous)?;
	}
	if let Err(e) = rename(staging, &current) {
		if had_current {
			let _ = rename(&previous, &current);
		}
		return Err(e);
	}
	Ok(())
}

/// Swaps the installed version and the kept one.
fn swap_with_kept(app: &AppHandle, id: &str) -> Result<(), String> {
	let current = installed_dir(app, id);
	let previous = previous_dir(app, id);
	let aside = downloads_dir(app).join(format!("{id}-swap"));
	fs::create_dir_all(downloads_dir(app)).map_err(|e| e.to_string())?;
	remove_dir(&aside)?;
	rename(&current, &aside)?;
	if let Err(e) = rename(&previous, &current) {
		let _ = rename(&aside, &current);
		return Err(e);
	}
	rename(&aside, &previous)
}

/// Renames a folder, trying again briefly: Windows can hold files of a process that just ended,
/// or one a virus scanner is reading.
fn rename(from: &Path, to: &Path) -> Result<(), String> {
	let mut tries = 0;
	loop {
		match fs::rename(from, to) {
			Ok(()) => return Ok(()),
			Err(_) if tries < 10 => {
				tries += 1;
				thread::sleep(Duration::from_millis(300));
			}
			Err(e) => {
				return Err(format!(
					"couldn't move {} to {}: {e}",
					from.display(),
					to.display()
				))
			}
		}
	}
}

fn remove_dir(path: &Path) -> Result<(), String> {
	if !path.exists() {
		return Ok(());
	}
	let mut tries = 0;
	loop {
		match fs::remove_dir_all(path) {
			Ok(()) => return Ok(()),
			Err(_) if tries < 10 => {
				tries += 1;
				thread::sleep(Duration::from_millis(300));
			}
			Err(e) => return Err(format!("couldn't remove {}: {e}", path.display())),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_sums() {
		let text = "abc123  other.zip\nDEF456 *tool-1.0.0-windows.zip\n";
		assert_eq!(
			sum_for(text, "tool-1.0.0-windows.zip").as_deref(),
			Some("def456")
		);
		assert_eq!(sum_for(text, "other.zip").as_deref(), Some("abc123"));
		assert_eq!(sum_for(text, "missing.zip"), None);
	}

	#[test]
	fn refuses_paths_outside_the_folder() {
		let dir = std::env::temp_dir().join(format!("cts-unpack-test-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let archive = dir.join("evil.zip");
		let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
		zip.start_file("../evil.txt", zip::write::SimpleFileOptions::default())
			.unwrap();
		zip.write_all(b"x").unwrap();
		zip.finish().unwrap();

		let result = unpack(&archive, &dir.join("out"));
		assert!(result.is_err_and(|e| e.contains("unsafe path")));
		assert!(!dir.join("evil.txt").exists());
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn compares_versions() {
		assert!(is_newer("1.1.0", "1.0.0"));
		assert!(!is_newer("1.0.0", "1.0.0"));
		assert!(!is_newer("1.0.0", "1.10.0"));
	}
}
