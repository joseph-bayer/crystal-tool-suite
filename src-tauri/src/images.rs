//! Icons and thumbnails, handed to the launcher page as `data:` URLs.

use base64::Engine;
use std::fs;
use std::path::Path;

/// Larger images are refused: they're shown at icon size, and sent to the page on every refresh.
pub const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;

/// The image types a thumbnail or icon may be, by extension.
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp"];

pub fn mime(ext: &str) -> Option<&'static str> {
	Some(match ext.to_ascii_lowercase().as_str() {
		"png" => "image/png",
		"jpg" | "jpeg" => "image/jpeg",
		"gif" => "image/gif",
		"webp" => "image/webp",
		"svg" => "image/svg+xml",
		"ico" => "image/x-icon",
		"bmp" => "image/bmp",
		_ => return None,
	})
}

pub fn extension(path: &Path) -> Option<String> {
	path.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase)
		.filter(|e| mime(e).is_some())
}

/// Checks a file can be used as an image: a known type, and not too large.
pub fn check(path: &Path) -> Result<String, String> {
	let ext = extension(path).ok_or_else(|| {
		format!(
			"{} isn't an image the launcher shows ({})",
			path.display(),
			EXTENSIONS.join(", ")
		)
	})?;
	let size = fs::metadata(path)
		.map_err(|e| format!("couldn't read {}: {e}", path.display()))?
		.len();
	if size > MAX_IMAGE_BYTES {
		return Err(format!(
			"{} is larger than {} MB; pick a smaller image",
			path.display(),
			MAX_IMAGE_BYTES / 1024 / 1024
		));
	}
	Ok(ext)
}

/// The image as a `data:` URL, or `None` if it can't be used.
pub fn data_url(path: &Path) -> Option<String> {
	let ext = check(path).ok()?;
	let bytes = fs::read(path).ok()?;
	Some(from_bytes(&ext, &bytes))
}

pub fn from_bytes(ext: &str, bytes: &[u8]) -> String {
	format!(
		"data:{};base64,{}",
		mime(ext).unwrap_or("application/octet-stream"),
		base64::engine::general_purpose::STANDARD.encode(bytes)
	)
}
