//! Recent projects: the folders the suite's tools have checked and remembered, in the list
//! every tool shares, `%APPDATA%\Crystal Tool Suite\known-paths.json`:
//!
//! ```json
//! { "version": 1, "paths": [
//!   { "kind": "cse-project", "path": "C:\\...", "lastUsed": "2026-10-20T03:11:39.059Z", "tool": "gen1-map-porter" }
//! ] }
//! ```
//!
//! The newest entry comes first. `%APPDATA%` is read from the environment, as the tools do.

use crate::jsonfile;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SHARED_DIR: &str = "Crystal Tool Suite";
const KNOWN_PATHS_FILE: &str = "known-paths.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KnownPath {
	pub kind: String,
	pub path: String,
	#[serde(default)]
	pub last_used: String,
	#[serde(default)]
	pub tool: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Project {
	#[serde(flatten)]
	pub known: KnownPath,
	/// Whether the folder is still there.
	pub exists: bool,
}

fn app_data() -> PathBuf {
	std::env::var_os("APPDATA")
		.map(PathBuf::from)
		.or_else(|| {
			std::env::var_os("USERPROFILE")
				.or_else(|| std::env::var_os("HOME"))
				.map(|h| PathBuf::from(h).join("AppData").join("Roaming"))
		})
		.unwrap_or_default()
}

pub fn known_paths_file() -> PathBuf {
	app_data().join(SHARED_DIR).join(KNOWN_PATHS_FILE)
}

fn entries(value: &Value) -> Vec<KnownPath> {
	value
		.get("paths")
		.and_then(Value::as_array)
		.map(|paths| {
			paths
				.iter()
				.filter_map(|p| serde_json::from_value(p.clone()).ok())
				.collect()
		})
		.unwrap_or_default()
}

/// Every remembered folder, most recently used first.
pub fn list() -> Result<Vec<Project>, String> {
	let Some(value) = jsonfile::read(&known_paths_file())? else {
		return Ok(Vec::new());
	};
	let mut known = entries(&value);
	// ISO times sort as text.
	known.sort_by(|a, b| b.last_used.cmp(&a.last_used));
	Ok(known
		.into_iter()
		.map(|k| Project {
			exists: Path::new(&k.path).is_dir(),
			known: k,
		})
		.collect())
}

/// Marks a remembered folder as just used by a tool, moving it to the top. Other entries, and
/// anything else in the file, are kept as they are.
pub fn touch(kind: &str, path: &str, tool: &str) -> Result<(), String> {
	touch_in(&known_paths_file(), kind, path, tool, &iso_now())
}

fn touch_in(file: &Path, kind: &str, path: &str, tool: &str, now: &str) -> Result<(), String> {
	let same = |p: &Value| {
		p.get("kind").and_then(Value::as_str) == Some(kind)
			&& p.get("path")
				.and_then(Value::as_str)
				.is_some_and(|q| q.to_lowercase() == path.to_lowercase())
	};
	jsonfile::update(
		file,
		&json!({ "version": 1, "paths": [] }),
		|mut value| {
			let mut paths = value
				.get("paths")
				.and_then(Value::as_array)
				.cloned()
				.unwrap_or_default();
			// The entry keeps any fields another tool added to it.
			let mut entry = paths
				.iter()
				.find(|p| same(p))
				.cloned()
				.unwrap_or_else(|| json!({ "kind": kind, "path": path }));
			entry["lastUsed"] = json!(now);
			entry["tool"] = json!(tool);
			paths.retain(|p| !same(p));
			paths.insert(0, entry);
			if !value.is_object() {
				value = json!({ "version": 1 });
			}
			value["paths"] = Value::Array(paths);
			value
		},
		true,
	)
}

/// The time now as `2026-10-20T03:11:39.059Z`, as JavaScript's `toISOString` writes it.
fn iso_now() -> String {
	let since = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default();
	iso_from_millis(since.as_millis() as i64)
}

fn iso_from_millis(ms: i64) -> String {
	let secs = ms.div_euclid(1000);
	let days = secs.div_euclid(86_400);
	let rem = secs.rem_euclid(86_400);
	// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z - era * 146_097;
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	let mp = (5 * doy + 2) / 153;
	let day = doy - (153 * mp + 2) / 5 + 1;
	let month = if mp < 10 { mp + 3 } else { mp - 9 };
	let year = yoe + era * 400 + i64::from(month <= 2);
	format!(
		"{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
		rem / 3600,
		rem / 60 % 60,
		rem % 60,
		ms.rem_euclid(1000)
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_times_like_javascript() {
		assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
		// new Date(1792466499059).toISOString()
		assert_eq!(
			iso_from_millis(1_792_466_499_059),
			"2026-10-20T03:21:39.059Z"
		);
		assert_eq!(iso_from_millis(951_782_400_000), "2000-02-29T00:00:00.000Z");
	}

	#[test]
	fn touching_moves_an_entry_to_the_top_and_keeps_the_rest() {
		let dir = std::env::temp_dir().join(format!("cts-known-test-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		let file = dir.join("known-paths.json");
		std::fs::create_dir_all(&dir).unwrap();
		std::fs::write(
			&file,
			r#"{"version":1,"other":true,"paths":[
				{"kind":"pokered","path":"C:/red","lastUsed":"2026-01-02T00:00:00.000Z","tool":"a"},
				{"kind":"cse-project","path":"C:/Hack","lastUsed":"2026-01-01T00:00:00.000Z","tool":"a","extra":1}
			]}"#,
		)
		.unwrap();
		touch_in(
			&file,
			"cse-project",
			"c:/hack",
			"b",
			"2026-02-01T00:00:00.000Z",
		)
		.unwrap();
		let value = jsonfile::read(&file).unwrap().unwrap();
		assert_eq!(value["other"], json!(true));
		// Keys stay in the order the tools write them.
		let text = std::fs::read_to_string(&file).unwrap();
		assert!(text.starts_with(
			"{\n\t\"version\": 1,\n\t\"other\": true,\n\t\"paths\": [\n\t\t{\n\t\t\t\"kind\""
		));
		assert!(text.contains("\"path\": \"C:/Hack\",\n\t\t\t\"lastUsed\""));
		let paths = value["paths"].as_array().unwrap();
		assert_eq!(paths.len(), 2);
		assert_eq!(
			paths[0],
			json!({ "kind": "cse-project", "path": "C:/Hack", "lastUsed": "2026-02-01T00:00:00.000Z", "tool": "b", "extra": 1 })
		);
		assert_eq!(paths[1]["path"], json!("C:/red"));

		// A folder not in the list yet is added at the top.
		touch_in(
			&file,
			"pokeyellow",
			"C:/yellow",
			"b",
			"2026-03-01T00:00:00.000Z",
		)
		.unwrap();
		let value = jsonfile::read(&file).unwrap().unwrap();
		assert_eq!(value["paths"][0]["path"], json!("C:/yellow"));
		assert_eq!(value["paths"].as_array().unwrap().len(), 3);
		let _ = std::fs::remove_dir_all(&dir);
	}
}
