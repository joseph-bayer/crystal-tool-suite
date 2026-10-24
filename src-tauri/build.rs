// Declaring the app's commands makes each one need a permission, so only windows whose
// capability grants it (the launcher's own) can call them.
const COMMANDS: &[&str] = &[
	"list_tools",
	"start_tool",
	"open_in_browser",
	"open_logs",
	"check_updates",
	"install_tool",
	"roll_back_tool",
	"uninstall_tool",
	"open_release_notes",
	"get_settings",
	"save_settings",
	"update_launcher",
	"report_bug",
	"pick_image",
	"pick_program",
	"save_custom_tool",
	"remove_custom_tool",
	"open_custom_tool",
	"add_suggested_tool",
	"open_suggestion_page",
	"open_custom_homepage",
];

fn main() {
	tauri_build::try_build(
		tauri_build::Attributes::new()
			.app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
	)
	.expect("failed to run the Tauri build script")
}
