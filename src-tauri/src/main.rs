// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
	crystal_tool_suite_lib::run()
}
