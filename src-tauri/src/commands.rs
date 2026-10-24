//! What the launcher window's page can ask for.

use crate::catalog::{self, Catalog, Release, SuggestedTool};
use crate::custom::{self, CustomTool, Kind};
use crate::images;
use crate::install::{self, Installer};
use crate::launcher::{self, LauncherState, LauncherUpdate};
use crate::projects::{self, Project};
use crate::report;
use crate::runner::{self, Onto, Phase, Runner};
use crate::settings::{self, Settings};
use crate::tools;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolList {
	tools: Vec<ToolRow>,
	problems: Vec<ProblemRow>,
	tools_dir: String,
	dev_tools_file: String,
	settings_file: String,
	/// Why the suite index couldn't be read, if it couldn't.
	index_error: Option<String>,
	index_checking: bool,
	launcher_version: String,
	launcher: LauncherState,
	/// Remembered project folders, most recent first.
	projects: Vec<Project>,
	projects_error: Option<String>,
	known_paths_file: String,
	custom_tools: Vec<CustomRow>,
	custom_error: Option<String>,
	custom_tools_file: String,
	/// Well-known tools from the suite index, to add as custom tools.
	suggestions: Vec<SuggestionRow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomRow {
	#[serde(flatten)]
	tool: CustomTool,
	/// The thumbnail as a `data:` URL.
	image: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionRow {
	#[serde(flatten)]
	suggestion: SuggestedTool,
	/// Already added as a custom tool.
	added: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRow {
	id: String,
	name: String,
	description: String,
	/// The version that runs, if any.
	version: Option<String>,
	folder: Option<String>,
	/// Listed in `dev-tools.json`: run as it is, never installed over.
	dev: bool,
	/// Bytes used by the installed version and the kept earlier one.
	disk_use: Option<u64>,
	/// The earlier version kept for rolling back.
	kept_version: Option<String>,
	/// The index's latest release, when the index lists the tool.
	latest: Option<ReleaseRow>,
	update_available: bool,
	/// What "Roll back" goes back to: the kept version, else the index's previous release.
	roll_back_to: Option<String>,
	/// `stopped`, `starting` or `running`.
	state: &'static str,
	/// Why the last start failed, or why the tool stopped on its own.
	error: Option<String>,
	/// What the installer is doing to it right now.
	task: Option<String>,
	/// An update is waiting for the tool to be closed.
	update_waiting: bool,
	/// Why the last install, update, roll back or uninstall failed.
	install_error: Option<String>,
	/// A `data:` URL of the installed tool's icon, else the index's icon address.
	icon: Option<String>,
	/// The folder kinds the installed tool works with.
	folders: Vec<String>,
	/// The index gives a GitHub repo to report bugs on.
	can_report: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRow {
	version: String,
	notes: Option<String>,
	has_notes_url: bool,
}

#[derive(Serialize)]
pub struct ProblemRow {
	folder: String,
	message: String,
}

impl From<&Release> for ReleaseRow {
	fn from(r: &Release) -> Self {
		ReleaseRow {
			version: r.version.clone(),
			notes: r.notes.clone(),
			has_notes_url: r.notes_url.is_some(),
		}
	}
}

#[tauri::command]
pub async fn list_tools(app: AppHandle) -> Result<ToolList, String> {
	tauri::async_runtime::spawn_blocking(move || build_list(&app))
		.await
		.map_err(|e| e.to_string())
}

fn build_list(app: &AppHandle) -> ToolList {
	let runner = app.state::<Runner>();
	let installer = app.state::<Installer>();
	let catalog = app.state::<Catalog>().snapshot();
	let index_tools = catalog.index.as_ref().map_or(&[][..], |i| &i.tools[..]);
	let (found, problems) = tools::discover(app);

	let mut rows = Vec::new();
	let mut seen = HashSet::new();
	for tool in found {
		let id = tool.manifest.id.clone();
		seen.insert(id.clone());
		let listed = index_tools.iter().find(|t| t.id == id);
		let icon = tool
			.icon_file()
			.and_then(|f| images::data_url(&f))
			.or_else(|| listed.and_then(|t| web_icon(t.icon.as_deref())));
		let kept = (!tool.dev).then(|| install::kept(app, &id)).flatten();
		let roll_back_to = if tool.dev {
			None
		} else if let Some(kept) = &kept {
			Some(kept.manifest.version.clone())
		} else {
			listed
				.and_then(|t| t.previous.as_ref())
				.filter(|p| install::is_newer(&tool.manifest.version, &p.version))
				.map(|p| p.version.clone())
		};
		rows.push(ToolRow {
			name: tool.manifest.name.clone(),
			description: tool.manifest.description.clone(),
			disk_use: (!tool.dev).then(|| {
				install::disk_use(&tool.folder)
					+ install::disk_use(&install::previous_dir(app, &id))
			}),
			kept_version: kept.map(|k| k.manifest.version),
			update_available: !tool.dev
				&& listed
					.is_some_and(|t| install::is_newer(&t.latest.version, &tool.manifest.version)),
			latest: listed.map(|t| (&t.latest).into()),
			roll_back_to,
			folder: Some(tool.folder.display().to_string()),
			version: Some(tool.manifest.version),
			dev: tool.dev,
			state: state_of(&runner, &id),
			error: runner.error(&id),
			task: installer.task(&id),
			update_waiting: installer.waiting(&id),
			install_error: installer.error(&id),
			icon,
			folders: tool.manifest.folders.clone(),
			can_report: listed.is_some_and(can_report),
			id,
		});
	}
	// Tools the index offers that aren't here yet.
	for t in index_tools.iter().filter(|t| !seen.contains(&t.id)) {
		rows.push(ToolRow {
			id: t.id.clone(),
			name: t.name.clone(),
			description: t.description.clone(),
			version: None,
			folder: None,
			dev: false,
			disk_use: None,
			kept_version: None,
			latest: Some((&t.latest).into()),
			update_available: false,
			roll_back_to: None,
			state: "stopped",
			error: None,
			task: installer.task(&t.id),
			update_waiting: false,
			install_error: installer.error(&t.id),
			icon: web_icon(t.icon.as_deref()),
			folders: Vec::new(),
			can_report: can_report(t),
		});
	}

	let (projects, projects_error) = match projects::list() {
		Ok(p) => (p, None),
		Err(e) => (Vec::new(), Some(e)),
	};
	let (custom_list, custom_error) = match custom::list(app) {
		Ok(c) => (c, None),
		Err(e) => (Vec::new(), Some(e)),
	};
	let suggestions = catalog
		.index
		.as_ref()
		.map_or(&[][..], |i| &i.custom_tools[..])
		.iter()
		.map(|s| SuggestionRow {
			added: custom_list
				.iter()
				.any(|c| c.suggestion.as_deref() == Some(&s.id)),
			suggestion: SuggestedTool {
				icon: web_icon(s.icon.as_deref()),
				..s.clone()
			},
		})
		.collect();
	let custom_tools = custom_list
		.into_iter()
		.map(|tool| CustomRow {
			image: custom::thumbnail_path(app, &tool).and_then(|p| images::data_url(&p)),
			tool,
		})
		.collect();

	ToolList {
		tools: rows,
		problems: problems
			.into_iter()
			.map(|p| ProblemRow {
				folder: p.folder.display().to_string(),
				message: p.message,
			})
			.collect(),
		tools_dir: tools::tools_dir(app).display().to_string(),
		dev_tools_file: tools::dev_tools_file(app).display().to_string(),
		settings_file: settings::settings_file(app).display().to_string(),
		index_error: catalog.error,
		index_checking: catalog.checking,
		launcher_version: app.package_info().version.to_string(),
		launcher: app.state::<LauncherUpdate>().snapshot(),
		projects,
		projects_error,
		known_paths_file: projects::known_paths_file().display().to_string(),
		custom_tools,
		custom_error,
		custom_tools_file: custom::file(app).display().to_string(),
		suggestions,
	}
}

/// An icon address from the index the page may load: https (or this computer, for testing).
fn web_icon(icon: Option<&str>) -> Option<String> {
	icon.filter(|i| reqwest::Url::parse(i).is_ok_and(|u| catalog::allowed(&u)))
		.map(str::to_string)
}

fn can_report(tool: &catalog::IndexTool) -> bool {
	tool.repo
		.as_deref()
		.and_then(|r| report::issue_url(r, ""))
		.is_some()
}

fn state_of(runner: &Runner, id: &str) -> &'static str {
	match runner.phase(id) {
		None => "stopped",
		Some(Phase::Starting) => "starting",
		Some(Phase::Running) => "running",
	}
}

#[derive(Deserialize)]
pub struct ProjectArg {
	path: String,
	kind: String,
}

/// Starts a tool and opens its window, or focuses its window if it's running. Resolves once the
/// window is open; a failure is also kept for `list_tools` to show. With a project, the tool is
/// opened onto that folder, which moves to the top of the recent projects.
#[tauri::command]
pub async fn start_tool(
	app: AppHandle,
	id: String,
	project: Option<ProjectArg>,
) -> Result<(), String> {
	if let Some(p) = &project {
		if !Path::new(&p.path).is_dir() {
			return Err(format!("{} isn't there any more", p.path));
		}
	}
	tauri::async_runtime::spawn_blocking(move || {
		let tool = tools::find(&app, &id)?;
		let onto = project.map(|p| Onto {
			path: p.path,
			kind: p.kind,
		});
		app.state::<Runner>().start(&app, &tool, onto.as_ref())?;
		if let Some(onto) = onto {
			// The tool is open either way; this only reorders the list.
			let _ = projects::touch(&onto.kind, &onto.path, &id);
			let _ = app.emit(runner::TOOLS_CHANGED, ());
		}
		Ok(())
	})
	.await
	.map_err(|e| e.to_string())?
}

/// Opens a running tool's page in the default browser.
#[tauri::command]
pub async fn open_in_browser(
	app: AppHandle,
	runner: State<'_, Runner>,
	id: String,
) -> Result<(), String> {
	let url = runner.running_url(&id).ok_or("the tool isn't running")?;
	app.opener()
		.open_url(url, None::<&str>)
		.map_err(|e| e.to_string())
}

/// Opens the logs folder, with the tool's latest log selected if it has one.
#[tauri::command]
pub async fn open_logs(app: AppHandle, id: String) -> Result<(), String> {
	let log = runner::log_file(&app, &id);
	if log.is_file() {
		return app
			.opener()
			.reveal_item_in_dir(log)
			.map_err(|e| e.to_string());
	}
	let dir = runner::logs_dir(&app);
	fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
	app.opener()
		.open_path(dir.display().to_string(), None::<&str>)
		.map_err(|e| e.to_string())
}

/// Reads the suite index again and looks for a launcher update.
#[tauri::command]
pub async fn check_updates(app: AppHandle) -> Result<(), String> {
	check_all(&app).await;
	Ok(())
}

pub async fn check_all(app: &AppHandle) {
	match settings::load(app) {
		Ok(s) => app.state::<Catalog>().refresh(&s.index_url).await,
		Err(e) => app.state::<Catalog>().fail(e),
	}
	let _ = app.emit(runner::TOOLS_CHANGED, ());
	launcher::check(app).await;
}

/// Installs or updates a tool to the index's latest version. Resolves with `true` when the
/// update waits for the tool to be closed; progress and failures show in `list_tools`.
#[tauri::command]
pub async fn install_tool(app: AppHandle, id: String) -> Result<bool, String> {
	install::update(&app, &id)
}

#[tauri::command]
pub async fn roll_back_tool(app: AppHandle, id: String) -> Result<(), String> {
	install::roll_back(&app, &id)
}

#[tauri::command]
pub async fn uninstall_tool(app: AppHandle, id: String) -> Result<(), String> {
	install::uninstall(&app, &id)
}

/// Opens the release notes page the index gives for a tool's latest version.
#[tauri::command]
pub async fn open_release_notes(app: AppHandle, id: String) -> Result<(), String> {
	let url = app
		.state::<Catalog>()
		.tool(&id)
		.and_then(|t| t.latest.notes_url)
		.ok_or("the index gives no release notes page for it")?;
	if !reqwest::Url::parse(&url).is_ok_and(|u| catalog::allowed(&u)) {
		return Err(format!("the release notes address {url} isn't https"));
	}
	app.opener()
		.open_url(url, None::<&str>)
		.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings, String> {
	settings::load(&app)
}

/// Saves the settings and checks again with them.
#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
	settings::save(&app, &settings)?;
	check_all(&app).await;
	Ok(())
}

#[tauri::command]
pub async fn update_launcher(app: AppHandle) -> Result<(), String> {
	launcher::update_and_restart(&app).await
}

/// Opens a new GitHub issue on a suite tool's repo, prefilled with the tool's version, the
/// launcher's and the OS.
#[tauri::command]
pub async fn report_bug(app: AppHandle, id: String) -> Result<(), String> {
	let listed = app
		.state::<Catalog>()
		.tool(&id)
		.ok_or("the suite index doesn't list it")?;
	let repo = listed
		.repo
		.as_deref()
		.ok_or("the suite index gives no repo for it")?;
	let version = match tools::find(&app, &id) {
		Ok(t) if t.dev => format!("{} (development copy)", t.manifest.version),
		Ok(t) => t.manifest.version,
		Err(_) => "(not installed)".to_string(),
	};
	let body = report::body(
		&listed.name,
		&version,
		&app.package_info().version.to_string(),
		&report::os(),
	);
	let url = report::issue_url(repo, &body).ok_or("its repo isn't on GitHub")?;
	app.opener()
		.open_url(url, None::<&str>)
		.map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedImage {
	path: String,
	data_url: String,
}

/// Asks for an image file to use as a thumbnail. `None` if the dialog was cancelled.
#[tauri::command]
pub async fn pick_image(app: AppHandle) -> Result<Option<PickedImage>, String> {
	let picked = tauri::async_runtime::spawn_blocking(move || {
		let mut dialog = app
			.dialog()
			.file()
			.set_title("Choose a thumbnail")
			.add_filter("Images", images::EXTENSIONS);
		if let Some(main) = app.get_webview_window("main") {
			dialog = dialog.set_parent(&main);
		}
		dialog.blocking_pick_file()
	})
	.await
	.map_err(|e| e.to_string())?;
	let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
		return Ok(None);
	};
	images::check(&path)?;
	let data_url = images::data_url(&path).ok_or("couldn't read the image")?;
	Ok(Some(PickedImage {
		path: path.display().to_string(),
		data_url,
	}))
}

/// Asks for a program file. `None` if the dialog was cancelled.
#[tauri::command]
pub async fn pick_program(app: AppHandle) -> Result<Option<String>, String> {
	let picked = tauri::async_runtime::spawn_blocking(move || {
		let mut dialog = app.dialog().file().set_title("Choose the program");
		if cfg!(windows) {
			dialog = dialog
				.add_filter("Programs", &["exe", "bat", "cmd", "lnk"])
				.add_filter("All files", &["*"]);
		}
		if let Some(main) = app.get_webview_window("main") {
			dialog = dialog.set_parent(&main);
		}
		dialog.blocking_pick_file()
	})
	.await
	.map_err(|e| e.to_string())?;
	Ok(picked
		.and_then(|p| p.into_path().ok())
		.map(|p| p.display().to_string()))
}

#[tauri::command]
pub async fn save_custom_tool(app: AppHandle, tool: custom::Edit) -> Result<(), String> {
	let result = custom::save(&app, tool);
	let _ = app.emit(runner::TOOLS_CHANGED, ());
	result
}

#[tauri::command]
pub async fn remove_custom_tool(app: AppHandle, id: String) -> Result<(), String> {
	let result = custom::remove(&app, &id);
	let _ = app.emit(runner::TOOLS_CHANGED, ());
	result
}

#[tauri::command]
pub async fn open_custom_tool(app: AppHandle, id: String) -> Result<(), String> {
	custom::open(&app, &id)
}

fn suggestion(app: &AppHandle, id: &str) -> Result<SuggestedTool, String> {
	app.state::<Catalog>()
		.snapshot()
		.index
		.and_then(|i| i.custom_tools.into_iter().find(|s| s.id == id))
		.ok_or_else(|| "the suite index doesn't suggest that tool any more".to_string())
}

/// Adds a tool the suite index suggests. A program needs the file the user picked.
#[tauri::command]
pub async fn add_suggested_tool(
	app: AppHandle,
	id: String,
	program: Option<String>,
) -> Result<(), String> {
	let suggested = suggestion(&app, &id)?;
	let result = custom::add_suggested(&app, &suggested, program).await;
	let _ = app.emit(runner::TOOLS_CHANGED, ());
	result
}

/// Opens where to get a suggested program, or a suggested web tool's page, in the browser.
#[tauri::command]
pub async fn open_suggestion_page(app: AppHandle, id: String) -> Result<(), String> {
	let suggested = suggestion(&app, &id)?;
	let url = match suggested.kind {
		Kind::Web => suggested.url.as_deref(),
		Kind::Program => suggested.get_it_at(),
	}
	.ok_or("it gives no page")?;
	open_web(&app, url)
}

/// Opens a custom tool's page in the browser: a web tool's address, or where a program came from.
#[tauri::command]
pub async fn open_custom_homepage(app: AppHandle, id: String) -> Result<(), String> {
	let tool = custom::list(&app)?
		.into_iter()
		.find(|t| t.id == id)
		.ok_or("that custom tool isn't there any more")?;
	let url = match tool.kind {
		Kind::Web => tool.url,
		Kind::Program => tool.homepage,
	}
	.ok_or("it has no page")?;
	open_web(&app, &url)
}

fn open_web(app: &AppHandle, url: &str) -> Result<(), String> {
	if !reqwest::Url::parse(url).is_ok_and(|u| catalog::web_page(&u)) {
		return Err(format!("{url} isn't a web page address"));
	}
	app.opener()
		.open_url(url, None::<&str>)
		.map_err(|e| e.to_string())
}
