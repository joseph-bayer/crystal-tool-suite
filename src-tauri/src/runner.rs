//! Starting and stopping tools: one process per tool on a free local port, shown in its own
//! window, stopped (with everything it started) when that window closes.

use crate::install;
use crate::job::Job;
use crate::tools::Tool;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// Sent to the launcher window whenever a tool starts, stops or fails.
pub const TOOLS_CHANGED: &str = "tools-changed";
const WINDOW_PREFIX: &str = "tool-";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(150);
/// Each tool's log keeps this many runs: `<id>.log`, `<id>.1.log`, `<id>.2.log`.
const LOG_RUNS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
	Starting,
	Running,
}

struct Run {
	run: u64,
	phase: Phase,
	port: u16,
	job: Option<Arc<Job>>,
}

/// The tools the launcher is running, the last error of each tool that failed, and the tools
/// being installed, which can't start until that's done.
#[derive(Default)]
pub struct Runner {
	runs: Mutex<HashMap<String, Run>>,
	errors: Mutex<HashMap<String, String>>,
	next_run: AtomicU64,
	held: Mutex<HashSet<String>>,
}

pub fn window_label(id: &str) -> String {
	format!("{WINDOW_PREFIX}{id}")
}

/// The tool id behind a tool window's label.
pub fn tool_of_window(label: &str) -> Option<&str> {
	label.strip_prefix(WINDOW_PREFIX)
}

pub fn url(port: u16) -> String {
	format!("http://127.0.0.1:{port}/")
}

/// A project folder a tool is opened onto, passed in its page's address as
/// `?project=<path>&kind=<kind>`.
#[derive(Clone, Debug)]
pub struct Onto {
	pub path: String,
	pub kind: String,
}

/// The address a tool's window opens: its page, with the project it's opened onto, if any.
pub fn page_url(port: u16, onto: Option<&Onto>) -> Result<tauri::Url, String> {
	let mut url: tauri::Url = url(port).parse().map_err(|e| format!("bad URL: {e}"))?;
	if let Some(onto) = onto {
		url.query_pairs_mut()
			.append_pair("project", &onto.path)
			.append_pair("kind", &onto.kind);
	}
	Ok(url)
}

pub fn logs_dir(app: &AppHandle) -> PathBuf {
	app.path().app_log_dir().unwrap_or_default()
}

pub fn log_file(app: &AppHandle, id: &str) -> PathBuf {
	logs_dir(app).join(format!("{id}.log"))
}

impl Runner {
	pub fn phase(&self, id: &str) -> Option<Phase> {
		self.runs.lock().unwrap().get(id).map(|r| r.phase)
	}

	pub fn error(&self, id: &str) -> Option<String> {
		self.errors.lock().unwrap().get(id).cloned()
	}

	/// The URL of a tool that's up and answering.
	pub fn running_url(&self, id: &str) -> Option<String> {
		self.runs
			.lock()
			.unwrap()
			.get(id)
			.filter(|r| r.phase == Phase::Running)
			.map(|r| url(r.port))
	}

	/// Keeps a tool from starting while its folder is replaced or removed. Fails, holding
	/// nothing, if the tool is running or starting.
	pub fn hold(&self, id: &str) -> bool {
		let runs = self.runs.lock().unwrap();
		if runs.contains_key(id) {
			return false;
		}
		self.held.lock().unwrap().insert(id.to_string());
		true
	}

	pub fn release(&self, id: &str) {
		self.held.lock().unwrap().remove(id);
	}

	/// Starts the tool and opens its window, or brings its window forward if it's already
	/// running. Opened onto a project, a running tool's window loads its page again with that
	/// project. Blocks until the tool answers or fails, so call it off the main thread.
	pub fn start(&self, app: &AppHandle, tool: &Tool, onto: Option<&Onto>) -> Result<(), String> {
		let id = tool.manifest.id.clone();
		let run = {
			let mut runs = self.runs.lock().unwrap();
			if self.held.lock().unwrap().contains(&id) {
				drop(runs);
				let message = format!(
					"{} is being installed or removed; try again when that's done",
					tool.manifest.name
				);
				self.errors
					.lock()
					.unwrap()
					.insert(id.clone(), message.clone());
				let _ = app.emit(TOOLS_CHANGED, ());
				return Err(message);
			}
			match runs.get(&id).map(|r| r.phase) {
				Some(Phase::Running) => {
					let port = runs[&id].port;
					drop(runs);
					if onto.is_some() {
						if let Some(window) = app.get_webview_window(&window_label(&id)) {
							window
								.navigate(page_url(port, onto)?)
								.map_err(|e| format!("couldn't open the project: {e}"))?;
						}
					}
					focus(app, &id);
					return Ok(());
				}
				// One copy each: the start already under way opens the window.
				Some(Phase::Starting) => return Ok(()),
				None => {}
			}
			let run = self.next_run.fetch_add(1, Ordering::Relaxed);
			runs.insert(
				id.clone(),
				Run {
					run,
					phase: Phase::Starting,
					port: 0,
					job: None,
				},
			);
			run
		};
		self.errors.lock().unwrap().remove(&id);
		let _ = app.emit(TOOLS_CHANGED, ());

		let result = self.launch(app, tool, run, onto);
		if let Err(message) = &result {
			let mut runs = self.runs.lock().unwrap();
			if let Some(failed) = runs.get(&id).filter(|r| r.run == run) {
				if let Some(job) = &failed.job {
					job.terminate();
				}
				runs.remove(&id);
			}
			drop(runs);
			self.errors
				.lock()
				.unwrap()
				.insert(id.clone(), message.clone());
			install::tool_stopped(app, &id);
		}
		let _ = app.emit(TOOLS_CHANGED, ());
		result
	}

	fn launch(
		&self,
		app: &AppHandle,
		tool: &Tool,
		run: u64,
		onto: Option<&Onto>,
	) -> Result<(), String> {
		let id = &tool.manifest.id;
		let name = &tool.manifest.name;
		let port = free_port().map_err(|e| format!("couldn't find a free port: {e}"))?;
		let log_path = log_file(app, id);
		let mut log =
			open_log(&log_path).map_err(|e| format!("couldn't create its log file: {e}"))?;

		let entry = tool.entry();
		let program = tool.program();
		let _ = writeln!(
			log,
			"[launcher] starting {} {} in {} with PORT={port}",
			program.display(),
			entry.args.join(" "),
			tool.folder.display()
		);

		let mut command = Command::new(&program);
		command
			.args(&entry.args)
			.current_dir(&tool.folder)
			.env("PORT", port.to_string())
			.stdin(Stdio::null())
			.stdout(log.try_clone().map_err(|e| e.to_string())?)
			.stderr(log.try_clone().map_err(|e| e.to_string())?);
		#[cfg(windows)]
		{
			use std::os::windows::process::CommandExt;
			// No console window for the tool's server.
			const CREATE_NO_WINDOW: u32 = 0x0800_0000;
			command.creation_flags(CREATE_NO_WINDOW);
		}
		let mut child = command
			.spawn()
			.map_err(|e| format!("couldn't start {}: {e}", program.display()))?;
		let job = match Job::for_child(&child) {
			Ok(job) => Arc::new(job),
			Err(e) => {
				let _ = child.kill();
				return Err(format!("couldn't set up {name}'s process: {e}"));
			}
		};
		{
			let mut runs = self.runs.lock().unwrap();
			let Some(r) = runs.get_mut(id).filter(|r| r.run == run) else {
				// Stopped while starting (the launcher is quitting).
				job.terminate();
				return Err(format!("{name} was stopped while starting"));
			};
			r.port = port;
			r.job = Some(job.clone());
		}

		// Watches for the tool's process ending on its own.
		{
			let app = app.clone();
			let id = id.clone();
			let name = name.clone();
			let job = job.clone();
			thread::spawn(move || {
				let status = child.wait();
				let runner = app.state::<Runner>();
				let mut runs = runner.runs.lock().unwrap();
				// A newer run of the same tool isn't this one's to clear.
				if runs.get(&id).is_some_and(|r| r.run == run) {
					let phase = runs.remove(&id).map(|r| r.phase);
					drop(runs);
					job.terminate();
					let code = status
						.ok()
						.and_then(|s| s.code())
						.map_or("unknown".to_string(), |c| c.to_string());
					runner.errors.lock().unwrap().insert(
						id.clone(),
						match phase {
							Some(Phase::Starting) => format!(
								"{name} stopped while starting (exit code {code}). Its log may say why."
							),
							_ => format!(
								"{name} stopped unexpectedly (exit code {code}). Its log may say why."
							),
						},
					);
					if let Some(window) = app.get_webview_window(&window_label(&id)) {
						let _ = window.close();
					}
					let _ = app.emit(TOOLS_CHANGED, ());
				}
			});
		}

		let deadline = Instant::now() + START_TIMEOUT;
		loop {
			if !self
				.runs
				.lock()
				.unwrap()
				.get(id)
				.is_some_and(|r| r.run == run)
			{
				// The watcher has recorded why.
				return Err(self.error(id).unwrap_or_else(|| {
					format!("{name} stopped while starting. Its log may say why.")
				}));
			}
			if answers(port) {
				break;
			}
			if Instant::now() >= deadline {
				return Err(format!(
					"{name} didn't answer at {} within {} seconds. Its log may say why.",
					url(port),
					START_TIMEOUT.as_secs()
				));
			}
			thread::sleep(POLL_INTERVAL);
		}

		let target = page_url(port, onto)?;
		WebviewWindowBuilder::new(app, window_label(id), WebviewUrl::External(target))
			.title(name)
			.inner_size(1280.0, 800.0)
			.build()
			.map_err(|e| format!("couldn't open {name}'s window: {e}"))?;

		let mut runs = self.runs.lock().unwrap();
		match runs.get_mut(id).filter(|r| r.run == run) {
			Some(r) => {
				r.phase = Phase::Running;
				Ok(())
			}
			None => Err(self
				.error(id)
				.unwrap_or_else(|| format!("{name} stopped while starting"))),
		}
	}

	/// Stops a tool and everything it started, then runs an update that was waiting for it.
	/// Its window, if open, is left to the caller.
	pub fn stop(&self, app: &AppHandle, id: &str) {
		let removed = self.runs.lock().unwrap().remove(id);
		if let Some(job) = removed.and_then(|r| r.job) {
			job.terminate();
		}
		let _ = app.emit(TOOLS_CHANGED, ());
		install::tool_stopped(app, id);
	}

	pub fn any_running(&self) -> bool {
		!self.runs.lock().unwrap().is_empty()
	}

	pub fn stop_all(&self) {
		for (_, run) in self.runs.lock().unwrap().drain() {
			if let Some(job) = run.job {
				job.terminate();
			}
		}
	}
}

fn focus(app: &AppHandle, id: &str) {
	if let Some(window) = app.get_webview_window(&window_label(id)) {
		let _ = window.show();
		let _ = window.unminimize();
		let _ = window.set_focus();
	}
}

/// A port nothing is listening on right now, chosen by the system.
fn free_port() -> std::io::Result<u16> {
	let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
	Ok(listener.local_addr()?.port())
}

/// Whether something answers HTTP on the port. Any status counts: the tool is up.
fn answers(port: u16) -> bool {
	let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
	let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
		return false;
	};
	let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
	let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
	let request = format!("GET / HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
	if stream.write_all(request.as_bytes()).is_err() {
		return false;
	}
	let mut head = [0u8; 5];
	stream.read_exact(&mut head).is_ok() && &head == b"HTTP/"
}

/// Moves earlier runs' logs down one (`x.log` to `x.1.log`, ...), dropping the oldest, and
/// opens a fresh `x.log`.
fn open_log(path: &Path) -> std::io::Result<File> {
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir)?;
	}
	let numbered = |n: usize| {
		if n == 0 {
			path.to_path_buf()
		} else {
			path.with_extension(format!("{n}.log"))
		}
	};
	let _ = fs::remove_file(numbered(LOG_RUNS - 1));
	for n in (0..LOG_RUNS - 1).rev() {
		let _ = fs::rename(numbered(n), numbered(n + 1));
	}
	OpenOptions::new().create(true).append(true).open(path)
}
