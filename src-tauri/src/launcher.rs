//! The launcher updating itself, through Tauri's updater: on start it looks for a newer version;
//! "Update and restart" downloads it, checks its signature against the public key built into
//! the launcher, runs its installer and reopens the launcher.

use crate::install::Installer;
use crate::runner::{Runner, TOOLS_CHANGED};
use crate::settings;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Updater, UpdaterExt};

#[derive(Default)]
pub struct LauncherUpdate {
	state: Mutex<LauncherState>,
}

#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherState {
	/// False when this build has no update key, so it can't update itself.
	pub configured: bool,
	pub checking: bool,
	pub installing: bool,
	pub available: Option<Available>,
	pub error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Available {
	pub version: String,
	pub notes: Option<String>,
}

impl LauncherUpdate {
	pub fn snapshot(&self) -> LauncherState {
		self.state.lock().unwrap().clone()
	}
}

/// The public key the launcher's updates are signed with, if this build has one.
fn pubkey(app: &AppHandle) -> Option<String> {
	app.config()
		.plugins
		.0
		.get("updater")
		.and_then(|u| u.get("pubkey"))
		.and_then(|k| k.as_str())
		.filter(|k| !k.trim().is_empty())
		.map(str::to_string)
}

fn updater(app: &AppHandle) -> Result<Updater, String> {
	let url = settings::load(app)?.launcher_update_url;
	let url = url
		.parse()
		.map_err(|e| format!("\"{url}\" isn't a URL: {e}"))?;
	let app_for_exit = app.clone();
	app.updater_builder()
		.endpoints(vec![url])
		.map_err(|e| e.to_string())?
		// Windows runs the installer and ends the launcher at once; the tools go first.
		.on_before_exit(move || app_for_exit.state::<Runner>().stop_all())
		.build()
		.map_err(|e| e.to_string())
}

fn changed(app: &AppHandle, f: impl FnOnce(&mut LauncherState)) {
	f(&mut app.state::<LauncherUpdate>().state.lock().unwrap());
	let _ = app.emit(TOOLS_CHANGED, ());
}

/// Looks for a newer launcher.
pub async fn check(app: &AppHandle) {
	if pubkey(app).is_none() {
		changed(app, |s| s.configured = false);
		return;
	}
	changed(app, |s| {
		s.configured = true;
		s.checking = true;
	});
	let result = match updater(app) {
		Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
		Err(e) => Err(e),
	};
	changed(app, |s| {
		s.checking = false;
		match result {
			Ok(update) => {
				s.available = update.map(|u| Available {
					version: u.version,
					notes: u.body,
				});
				s.error = None;
			}
			Err(e) => s.error = Some(format!("couldn't check for a launcher update: {e}")),
		}
	});
}

/// Downloads and installs the newer launcher, then restarts. Refused while tools are open or
/// being installed, since restarting stops them.
pub async fn update_and_restart(app: &AppHandle) -> Result<(), String> {
	if app.state::<Runner>().any_running() {
		return Err("close your tools before updating the launcher".into());
	}
	if app.state::<Installer>().busy() {
		return Err("wait for tool installs and updates to finish first".into());
	}
	changed(app, |s| {
		s.installing = true;
		s.error = None;
	});
	let result = async {
		let update = updater(app)?
			.check()
			.await
			.map_err(|e| e.to_string())?
			.ok_or("there's no newer launcher any more")?;
		update
			.download_and_install(|_, _| {}, || {})
			.await
			.map_err(|e| e.to_string())
	}
	.await;
	match result {
		// Windows doesn't get here: the installer has taken over and reopens the launcher.
		Ok(()) => app.restart(),
		Err(e) => {
			let message = format!("couldn't update the launcher: {e}");
			changed(app, |s| {
				s.installing = false;
				s.error = Some(message.clone());
			});
			Err(message)
		}
	}
}
