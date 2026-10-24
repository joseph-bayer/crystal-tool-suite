//! "Report a bug": a new GitHub issue on a tool's repo, prefilled with what a bug report needs
//! to say about where it happened.

/// The new-issue page for a GitHub repo, or `None` if the repo isn't one.
pub fn issue_url(repo: &str, body: &str) -> Option<String> {
	let parsed = reqwest::Url::parse(repo.trim()).ok()?;
	if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
		return None;
	}
	let mut parts = parsed.path_segments()?.filter(|s| !s.is_empty());
	let owner = parts.next()?;
	let name = parts.next()?;
	let name = name.strip_suffix(".git").unwrap_or(name);
	let mut url =
		reqwest::Url::parse(&format!("https://github.com/{owner}/{name}/issues/new")).ok()?;
	url.query_pairs_mut().append_pair("body", body);
	Some(url.to_string())
}

pub fn body(tool: &str, version: &str, launcher: &str, os: &str) -> String {
	format!(
		"**What happened**\n\n\n**What you expected**\n\n\n**Steps to make it happen**\n\n\n---\n\
		Tool: {tool} {version}\n\
		Launcher: Crystal Tool Suite {launcher}\n\
		OS: {os}\n"
	)
}

/// The operating system and its version, e.g. `Windows 10.0.19045 (x86_64)`.
pub fn os() -> String {
	format!("{} ({})", os_version(), std::env::consts::ARCH)
}

#[cfg(windows)]
fn os_version() -> String {
	use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
	use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

	// SAFETY: a zeroed OSVERSIONINFOW with its size set is what RtlGetVersion fills in.
	let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
	info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
	// Unlike GetVersionEx, this reports the real version whatever the program's manifest says.
	if unsafe { RtlGetVersion(&mut info) } != 0 {
		return "Windows".into();
	}
	format!(
		"Windows {}.{}.{}",
		info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
	)
}

#[cfg(not(windows))]
fn os_version() -> String {
	std::env::consts::OS.to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn builds_github_issue_urls() {
		let url = issue_url(
			"https://github.com/joseph-bayer/gen-1-map-porter",
			"a b\n&c",
		)
		.unwrap();
		assert_eq!(
			url,
			"https://github.com/joseph-bayer/gen-1-map-porter/issues/new?body=a+b%0A%26c"
		);
		assert_eq!(
			issue_url("https://github.com/o/r.git/", "x").as_deref(),
			Some("https://github.com/o/r/issues/new?body=x")
		);
		assert!(issue_url("https://example.com/o/r", "x").is_none());
		assert!(issue_url("http://github.com/o/r", "x").is_none());
		assert!(issue_url("https://github.com/o", "x").is_none());
	}

	#[test]
	fn names_the_os() {
		assert!(os().contains(std::env::consts::ARCH));
	}
}
