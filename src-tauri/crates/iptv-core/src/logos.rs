//! Channel artwork: bounded downloads through the image network policy and a small, private
//! on-disk cache served to the UI over the loopback relay. Artwork never carries stream authority.

use crate::{
    error::{Error, Result},
    models::SourceInput,
    network::{MediaNetwork, NetworkPolicy},
};
use futures_util::StreamExt;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    time::{Duration, SystemTime},
};
use url::Url;

pub const MAX_LOGO_BYTES: usize = 1024 * 1024;
const TYPES: [&str; 6] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/svg+xml",
];

fn is_svg(data: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&data[..data.len().min(4096)]);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    if !(head.starts_with("<svg") || head.starts_with("<?xml") || head.starts_with("<!--")) {
        return false;
    }
    let text = String::from_utf8_lossy(data).to_ascii_lowercase();
    text.contains("<svg") && !text.contains("<script")
}
/// Sniffs the image type from magic bytes; the declared header alone is never trusted.
pub fn image_type(data: &[u8], _declared: &str) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if data.len() > 12 && &data[4..12] == b"ftypavif" {
        Some("image/avif")
    } else if is_svg(data) {
        Some("image/svg+xml")
    } else {
        None
    }
}
/// Downloads one artwork file: any public host over the source's permitted transports,
/// bounded in size and time, accepted only when it sniffs as a supported image.
pub async fn fetch(url: &str, config: &SourceInput) -> Result<(Vec<u8>, &'static str)> {
    let parsed = Url::parse(url).map_err(|_| Error::NotFound)?;
    let network = MediaNetwork::new(NetworkPolicy::images(config), BTreeMap::new());
    let response = tokio::time::timeout(Duration::from_secs(20), network.get(parsed, None, false))
        .await
        .map_err(|_| Error::Network)??;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LOGO_BYTES as u64)
    {
        return Err(Error::Limit);
    }
    let declared = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut stream = response.bytes_stream();
    let mut data = Vec::with_capacity(16 * 1024);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| Error::Network)?;
        if data.len() + chunk.len() > MAX_LOGO_BYTES {
            return Err(Error::Limit);
        }
        data.extend_from_slice(&chunk);
    }
    let content_type = image_type(&data, &declared).ok_or(Error::Unsupported)?;
    Ok((data, content_type))
}
/// Cache entries are `content-type\n` followed by the image bytes.
pub fn read_cached(path: &Path) -> Option<(Vec<u8>, &'static str)> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > MAX_LOGO_BYTES + 64 {
        return None;
    }
    let newline = bytes.iter().position(|b| *b == b'\n')?;
    let declared = std::str::from_utf8(&bytes[..newline]).ok()?;
    let content_type = TYPES.iter().find(|t| **t == declared)?;
    Some((bytes[newline + 1..].to_vec(), content_type))
}
pub fn write_cached(path: &Path, content_type: &str, data: &[u8]) -> Result<()> {
    let temp = path.with_extension("part");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    file.write_all(content_type.as_bytes())?;
    file.write_all(b"\n")?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    Ok(())
}
/// Keeps the newest `keep` entries; older artwork is simply fetched again when needed.
pub fn prune(directory: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<(SystemTime, std::path::PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}
