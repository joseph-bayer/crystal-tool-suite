//! The suite index: one JSON file listing the suite's tools, their latest release and the
//! release before it. Read on start and when asked.
//!
//! ```json
//! {
//!   "schema": 1,
//!   "tools": [{
//!     "id": "gen1-map-porter",
//!     "name": "Gen 1 Map Porter",
//!     "description": "Ports Gen 1 maps into a Crystal project",
//!     "icon": "https://.../icon.png",
//!     "repo": "https://github.com/joseph-bayer/gen1-map-porter",
//!     "latest": {
//!       "version": "1.1.0",
//!       "notes": "What's new, as text",
//!       "notesUrl": "https://github.com/.../releases/tag/v1.1.0",
//!       "downloads": {
//!         "windows": { "url": "https://.../gen1-map-porter-1.1.0-windows.zip", "sha256": "<64 hex digits>" }
//!       }
//!     },
//!     "previous": { "version": "1.0.0", "downloads": { "windows": {
//!       "url": "https://.../gen1-map-porter-1.0.0-windows.zip",
//!       "sha256sumsUrl": "https://.../v1.0.0/SHA256SUMS.txt"
//!     } } }
//!   }],
//!   "customTools": [
//!     { "id": "some-editor", "name": "Some Editor", "author": "someone", "description": "...",
//!       "kind": "web", "url": "https://.../", "icon": "https://.../icon.png" },
//!     { "id": "some-program", "name": "Some Program", "kind": "program",
//!       "homepage": "https://github.com/...", "downloadUrl": "https://.../releases" }
//!   ]
//! }
//! ```
//!
//! A download names its checksum directly (`sha256`) or the release's `SHA256SUMS.txt`
//! (`sha256sumsUrl`), where the line for the download's file name is used.
//!
//! `customTools` (optional) lists well-known tools the suite doesn't ship, offered as custom
//! tools to add in one click: a web page (`url`), or a program the user gets from its `homepage`
//! or `downloadUrl` and picks once installed. An entry that isn't usable is left out.

use crate::custom::Kind;
use crate::tools::{parse_version, valid_id};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// The index format this launcher reads.
const SCHEMA: u32 = 1;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Index {
	pub schema: u32,
	pub tools: Vec<IndexTool>,
	#[serde(default)]
	pub custom_tools: Vec<SuggestedTool>,
}

/// A tool the suite doesn't ship, suggested as a custom tool.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedTool {
	pub id: String,
	pub name: String,
	#[serde(default)]
	pub author: Option<String>,
	#[serde(default)]
	pub description: String,
	pub kind: Kind,
	/// A web page tool's address.
	#[serde(default)]
	pub url: Option<String>,
	/// Where to read about a program.
	#[serde(default)]
	pub homepage: Option<String>,
	/// Where to download a program.
	#[serde(default)]
	pub download_url: Option<String>,
	#[serde(default)]
	pub icon: Option<String>,
}

impl SuggestedTool {
	/// Where to send someone to get a program: its download page, else its homepage.
	pub fn get_it_at(&self) -> Option<&str> {
		self.download_url.as_deref().or(self.homepage.as_deref())
	}

	fn usable(&self) -> bool {
		let web_ok = |u: &Option<String>| {
			u.as_deref()
				.and_then(|u| reqwest::Url::parse(u).ok())
				.is_some_and(|u| web_page(&u))
		};
		valid_id(&self.id)
			&& !self.name.trim().is_empty()
			&& match self.kind {
				Kind::Web => web_ok(&self.url),
				Kind::Program => self
					.get_it_at()
					.and_then(|u| reqwest::Url::parse(u).ok())
					.is_some_and(|u| allowed(&u)),
			}
	}
}

#[derive(Deserialize, Clone, Debug)]
pub struct IndexTool {
	pub id: String,
	pub name: String,
	#[serde(default)]
	pub description: String,
	#[serde(default)]
	pub icon: Option<String>,
	#[serde(default)]
	pub repo: Option<String>,
	pub latest: Release,
	/// The release before `latest`, for rolling back when the launcher kept no earlier copy.
	#[serde(default)]
	pub previous: Option<Release>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Release {
	pub version: String,
	#[serde(default)]
	pub notes: Option<String>,
	#[serde(default)]
	pub notes_url: Option<String>,
	/// Keyed by platform: `windows`, `macos` or `linux`.
	pub downloads: HashMap<String, Download>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Download {
	pub url: String,
	#[serde(default)]
	pub sha256: Option<String>,
	#[serde(default)]
	pub sha256sums_url: Option<String>,
}

impl Release {
	pub fn download(&self) -> Option<&Download> {
		self.downloads.get(std::env::consts::OS)
	}
}

/// The last index read, or why it couldn't be.
#[derive(Default)]
pub struct Catalog {
	state: Mutex<CatalogState>,
}

#[derive(Default, Clone)]
pub struct CatalogState {
	pub index: Option<Index>,
	pub error: Option<String>,
	pub checking: bool,
}

impl Catalog {
	pub fn snapshot(&self) -> CatalogState {
		self.state.lock().unwrap().clone()
	}

	pub fn tool(&self, id: &str) -> Option<IndexTool> {
		self.state
			.lock()
			.unwrap()
			.index
			.as_ref()
			.and_then(|i| i.tools.iter().find(|t| t.id == id).cloned())
	}

	pub fn fail(&self, error: String) {
		self.state.lock().unwrap().error = Some(error);
	}

	/// Reads the index again. A failure keeps the last good index and records the error.
	pub async fn refresh(&self, url: &str) {
		self.state.lock().unwrap().checking = true;
		let result = fetch(url).await;
		let mut state = self.state.lock().unwrap();
		state.checking = false;
		match result {
			Ok(index) => {
				state.index = Some(index);
				state.error = None;
			}
			Err(e) => state.error = Some(e),
		}
	}
}

async fn fetch(url: &str) -> Result<Index, String> {
	let text = get(url)
		.await?
		.text()
		.await
		.map_err(|e| format!("couldn't read the suite index: {e}"))?;
	parse_index(&text, url)
}

/// Reads and checks an index's text. `from` names where it came from, for the error. Custom
/// tools that aren't usable are left out.
fn parse_index(text: &str, from: &str) -> Result<Index, String> {
	let mut index: Index = serde_json::from_str(text)
		.map_err(|e| format!("the suite index at {from} isn't valid: {e}"))?;
	if index.schema != SCHEMA {
		return Err(format!(
			"the suite index uses format {}; this launcher reads format {SCHEMA}. A launcher update may be needed.",
			index.schema
		));
	}
	for tool in &index.tools {
		if !valid_id(&tool.id) {
			return Err(format!("the suite index has a bad tool id \"{}\"", tool.id));
		}
		for release in std::iter::once(&tool.latest).chain(&tool.previous) {
			if parse_version(&release.version).is_none() {
				return Err(format!(
					"the suite index lists {} version \"{}\", which isn't a version like 1.2.3",
					tool.id, release.version
				));
			}
		}
	}
	index.custom_tools.retain(SuggestedTool::usable);
	Ok(index)
}

/// Fetches a URL. Only `https` is used, except for this computer's own addresses (for testing).
pub async fn get(url: &str) -> Result<reqwest::Response, String> {
	let parsed = reqwest::Url::parse(url).map_err(|e| format!("\"{url}\" isn't a URL: {e}"))?;
	if !allowed(&parsed) {
		return Err(format!("{url} isn't https; only https addresses are used"));
	}
	let client = reqwest::Client::builder()
		.user_agent(concat!("crystal-tool-suite/", env!("CARGO_PKG_VERSION")))
		.connect_timeout(Duration::from_secs(15))
		.build()
		.map_err(|e| e.to_string())?;
	client
		.get(parsed)
		.send()
		.await
		.and_then(|r| r.error_for_status())
		.map_err(|e| format!("couldn't fetch {url}: {e}"))
}

pub fn allowed(url: &reqwest::Url) -> bool {
	match url.scheme() {
		"https" => true,
		"http" => is_loopback(url),
		_ => false,
	}
}

/// A page a launcher window may show: http or https.
pub fn web_page(url: &reqwest::Url) -> bool {
	matches!(url.scheme(), "https" | "http") && url.host_str().is_some()
}

pub fn is_loopback(url: &reqwest::Url) -> bool {
	matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The index this repo publishes, which every launcher reads.
	const PUBLISHED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../index.json"));

	#[test]
	fn published_index_parses() {
		let index = parse_index(PUBLISHED, "index.json").unwrap();
		let ids: Vec<&str> = index.custom_tools.iter().map(|t| t.id.as_str()).collect();
		assert_eq!(
			ids,
			["gen2-attack-animation-editor", "siren"],
			"no custom tool left out"
		);
	}

	#[test]
	fn other_schemas_and_unusable_tools_are_refused() {
		let err = parse_index(r#"{ "schema": 2, "tools": [] }"#, "x").unwrap_err();
		assert!(err.contains("format 2"), "{err}");

		let index = parse_index(
			r#"{ "schema": 1, "tools": [], "customTools": [
				{ "id": "ok", "name": "Ok", "kind": "web", "url": "https://example.com/" },
				{ "id": "no-url", "name": "No URL", "kind": "web" },
				{ "id": "plain-http", "name": "Plain HTTP", "kind": "program",
					"homepage": "http://example.com/" },
				{ "id": "Bad Id", "name": "Bad", "kind": "web", "url": "https://example.com/" }
			] }"#,
			"x",
		)
		.unwrap();
		let ids: Vec<&str> = index.custom_tools.iter().map(|t| t.id.as_str()).collect();
		assert_eq!(ids, ["ok"]);
	}
}
