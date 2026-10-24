mod catalog;
mod commands;
mod custom;
mod images;
mod install;
mod job;
mod jsonfile;
mod launcher;
mod projects;
mod report;
mod runner;
mod settings;
mod tools;

use catalog::Catalog;
use install::Installer;
use launcher::LauncherUpdate;
use runner::Runner;
use tauri::{
	menu::{Menu, MenuItem},
	tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
	AppHandle, Manager, RunEvent, WindowEvent,
};

const MAIN_WINDOW: &str = "main";
const MENU_OPEN: &str = "open";
const MENU_QUIT: &str = "quit";

/// Brings the launcher window back: shown, restored if minimized, and focused.
fn show_main_window(app: &AppHandle) {
	if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
		let _ = window.show();
		let _ = window.unminimize();
		let _ = window.set_focus();
	}
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
	// The HTTPS client's cryptography, shared with the updater.
	let _ = rustls::crypto::ring::default_provider().install_default();

	tauri::Builder::default()
		// Registered first, so a second launch hands over to this one before doing anything else.
		.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
			show_main_window(app);
		}))
		.plugin(tauri_plugin_opener::init())
		.plugin(tauri_plugin_dialog::init())
		.plugin(tauri_plugin_updater::Builder::new().build())
		.manage(Runner::default())
		.manage(Installer::default())
		.manage(Catalog::default())
		.manage(LauncherUpdate::default())
		.invoke_handler(tauri::generate_handler![
			commands::list_tools,
			commands::start_tool,
			commands::open_in_browser,
			commands::open_logs,
			commands::check_updates,
			commands::install_tool,
			commands::roll_back_tool,
			commands::uninstall_tool,
			commands::open_release_notes,
			commands::get_settings,
			commands::save_settings,
			commands::update_launcher,
			commands::report_bug,
			commands::pick_image,
			commands::pick_program,
			commands::save_custom_tool,
			commands::remove_custom_tool,
			commands::open_custom_tool,
			commands::add_suggested_tool,
			commands::open_suggestion_page,
			commands::open_custom_homepage,
		])
		.setup(|app| {
			let open = MenuItem::with_id(
				app,
				MENU_OPEN,
				"Open Crystal Tool Suite",
				true,
				None::<&str>,
			)?;
			let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
			let menu = Menu::with_items(app, &[&open, &quit])?;

			TrayIconBuilder::with_id("tray")
				.icon(
					app.default_window_icon()
						.expect("the app has an icon")
						.clone(),
				)
				.tooltip("Crystal Tool Suite")
				.menu(&menu)
				// Left click opens the window; the menu is on right click.
				.show_menu_on_left_click(false)
				.on_menu_event(|app, event| match event.id().as_ref() {
					MENU_OPEN => show_main_window(app),
					MENU_QUIT => app.exit(0),
					_ => {}
				})
				.on_tray_icon_event(|tray, event| {
					if let TrayIconEvent::Click {
						button: MouseButton::Left,
						button_state: MouseButtonState::Up,
						..
					} = event
					{
						show_main_window(tray.app_handle());
					}
				})
				.build(app)?;

			// Updates are looked for on every start.
			let handle = app.handle().clone();
			tauri::async_runtime::spawn(async move { commands::check_all(&handle).await });
			Ok(())
		})
		.on_window_event(|window, event| match event {
			// Closing the launcher window hides it; the app stays in the tray until Quit.
			WindowEvent::CloseRequested { api, .. } if window.label() == MAIN_WINDOW => {
				api.prevent_close();
				let _ = window.hide();
			}
			// Closing a tool's window stops the tool.
			WindowEvent::Destroyed => {
				if let Some(id) = runner::tool_of_window(window.label()) {
					let app = window.app_handle();
					app.state::<Runner>().stop(app, id);
				}
			}
			_ => {}
		})
		.build(tauri::generate_context!())
		.expect("error while building the Crystal Tool Suite")
		.run(|app, event| {
			// Quit stops every tool. (If the launcher dies instead, Windows ends them: see job.rs.)
			if let RunEvent::Exit = event {
				app.state::<Runner>().stop_all();
			}
		});
}
