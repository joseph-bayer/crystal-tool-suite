//! Small JSON files other programs may write too (the suite's shared folder list): read the
//! latest, change it, and write it back only if nobody wrote it meanwhile. The same way the
//! suite's Node tools write them, so either side's changes survive the other's.

use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Tab-indented JSON with a final newline, as the suite's tools write it.
pub fn to_text(value: &impl Serialize) -> Result<String, String> {
	let mut out = Vec::new();
	let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
	let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
	value.serialize(&mut ser).map_err(|e| e.to_string())?;
	out.push(b'\n');
	String::from_utf8(out).map_err(|e| e.to_string())
}

fn parse(bytes: &[u8], path: &Path) -> Result<Value, String> {
	// A byte-order mark (Notepad, PowerShell 5's utf8) would make parsing fail.
	let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
	serde_json::from_slice(bytes)
		.map_err(|_| format!("{} isn't valid JSON; fix or delete it", path.display()))
}

/// A JSON file's value, or `None` when it doesn't exist.
pub fn read(path: &Path) -> Result<Option<Value>, String> {
	match fs::read(path) {
		Ok(bytes) => parse(&bytes, path).map(Some),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
	}
}

/// Re-reads a JSON file, applies `change`, and writes the result. The new text goes to a
/// temporary file first and replaces the old one only if the old one still holds what was read;
/// otherwise it starts over from the newer content. With `replace_bad`, an unreadable file is
/// treated as `fallback` instead of refused.
pub fn update(
	path: &Path,
	fallback: &Value,
	mut change: impl FnMut(Value) -> Value,
	replace_bad: bool,
) -> Result<(), String> {
	if let Some(dir) = path.parent() {
		fs::create_dir_all(dir).map_err(|e| e.to_string())?;
	}
	let read_bytes = |p: &Path| match fs::read(p) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(e) => Err(format!("couldn't read {}: {e}", p.display())),
	};
	let mut temp = path.as_os_str().to_owned();
	temp.push(format!(".{}.tmp", std::process::id()));
	let temp = Path::new(&temp);
	for _ in 0..5 {
		let before = read_bytes(path)?;
		let value = match &before {
			None => fallback.clone(),
			Some(bytes) => match parse(bytes, path) {
				Ok(v) => v,
				Err(_) if replace_bad => fallback.clone(),
				Err(e) => return Err(e),
			},
		};
		let text = to_text(&change(value))?;
		fs::write(temp, text).map_err(|e| format!("couldn't write {}: {e}", temp.display()))?;
		if read_bytes(path)? == before {
			return fs::rename(temp, path).map_err(|e| {
				let _ = fs::remove_file(temp);
				format!("couldn't write {}: {e}", path.display())
			});
		}
		let _ = fs::remove_file(temp);
	}
	Err(format!(
		"{} kept changing while the launcher wrote it; try again",
		path.display()
	))
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn writes_tabs_and_merges() {
		let dir = std::env::temp_dir().join(format!("cts-json-test-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let path = dir.join("x.json");
		update(
			&path,
			&json!({ "n": 0 }),
			|mut v| {
				v["n"] = json!(v["n"].as_i64().unwrap() + 1);
				v
			},
			false,
		)
		.unwrap();
		assert_eq!(fs::read_to_string(&path).unwrap(), "{\n\t\"n\": 1\n}\n");

		// A byte-order mark is read past; bad JSON is refused unless replacing is allowed.
		fs::write(&path, "\u{FEFF}{\"n\": 5}").unwrap();
		assert_eq!(read(&path).unwrap(), Some(json!({ "n": 5 })));
		fs::write(&path, "{oops").unwrap();
		assert!(update(&path, &json!({}), |v| v, false).is_err());
		update(&path, &json!({ "n": 9 }), |v| v, true).unwrap();
		assert_eq!(read(&path).unwrap(), Some(json!({ "n": 9 })));
		assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no temp file left");
		let _ = fs::remove_dir_all(&dir);
	}
}
