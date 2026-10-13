use crate::{
    error::{Error, Result},
    limits::{MAX_CHANNELS, bounded_line},
    models::ParsedChannel,
};
use std::{collections::BTreeMap, io::BufRead};

#[derive(Default)]
pub struct PlaylistMetadata {
    pub suggested_epg_urls: Vec<String>,
    pub count: usize,
}

/// Streaming catalogue parser. HLS media manifests belong to the player, not this parser.
pub fn parse<R: BufRead>(
    reader: R,
    emit: impl FnMut(ParsedChannel) -> Result<()>,
) -> Result<PlaylistMetadata> {
    parse_with(reader, false, emit)
}
/// `allow_empty` accepts a well-formed playlist without entries: an official aggregated
/// playlist can legitimately be empty while other selected playlists are not.
pub fn parse_with<R: BufRead>(
    mut reader: R,
    allow_empty: bool,
    mut emit: impl FnMut(ParsedChannel) -> Result<()>,
) -> Result<PlaylistMetadata> {
    let mut line = String::new();
    let mut started = false;
    let mut pending: Option<ParsedChannel> = None;
    let mut meta = PlaylistMetadata::default();
    while bounded_line(&mut reader, &mut line)? {
        let value = line.trim().trim_start_matches('\u{feff}');
        if value.is_empty() {
            continue;
        }
        if !started {
            if value.split_whitespace().next() != Some("#EXTM3U") {
                return Err(Error::Invalid("A playlist must start with #EXTM3U."));
            }
            started = true;
            let attrs = attributes(value.strip_prefix("#EXTM3U").unwrap_or_default())?;
            for key in ["x-tvg-url", "url-tvg"] {
                if let Some(urls) = attrs.get(key) {
                    meta.suggested_epg_urls.extend(
                        urls.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                    );
                }
            }
            continue;
        }
        if value.starts_with("#EXT-X-") {
            return Err(Error::Invalid(
                "This is a media manifest, not a channel catalogue.",
            ));
        }
        if let Some(info) = value.strip_prefix("#EXTINF:") {
            if pending.is_some() {
                return Err(Error::Invalid("A playlist entry has no stream URL."));
            }
            let comma = split_unquoted(info, ',')
                .ok_or(Error::Invalid("An EXTINF entry has no channel name."))?;
            let attrs = attributes(&info[..comma])?;
            let name = info[comma + 1..].trim();
            if name.is_empty() || name.len() > 512 {
                return Err(Error::Invalid("A channel name is empty or too long."));
            }
            let get = |key: &str| attrs.get(key).cloned().unwrap_or_default();
            let shift = get("tvg-shift");
            let shift_seconds = if shift.is_empty() {
                0
            } else {
                let hours: f64 = shift
                    .parse()
                    .map_err(|_| Error::Invalid("Invalid tvg-shift."))?;
                if !hours.is_finite() || hours.abs() > 24.0 {
                    return Err(Error::Invalid("tvg-shift must be within 24 hours."));
                }
                (hours * 3600.0).round() as i64
            };
            let group = get("group-title");
            let mut catchup = BTreeMap::new();
            for key in [
                "catchup",
                "catchup-days",
                "catchup-source",
                "catchup-correction",
                "timeshift",
            ] {
                if let Some(v) = attrs.get(key) {
                    catchup.insert(key.to_owned(), v.clone());
                }
            }
            pending = Some(ParsedChannel {
                name: name.to_owned(),
                tvg_id: get("tvg-id"),
                tvg_name: get("tvg-name"),
                country: get("tvg-country"),
                logo: get("tvg-logo"),
                groups: if group.is_empty() {
                    vec![]
                } else {
                    vec![group]
                },
                shift_seconds,
                catchup,
                ..Default::default()
            });
        } else if let Some(group) = value.strip_prefix("#EXTGRP:") {
            if let Some(entry) = pending.as_mut() {
                let group = group.trim();
                if !group.is_empty() && !entry.groups.iter().any(|g| g == group) {
                    entry.groups.push(group.to_owned());
                }
            }
        } else if let Some(option) = value.strip_prefix("#EXTVLCOPT:") {
            if let Some(entry) = pending.as_mut()
                && let Some((name, value)) = option.split_once('=')
            {
                header(&mut entry.stream.headers, name, value)?;
            }
        } else if !value.starts_with('#') {
            let mut entry = pending
                .take()
                .ok_or(Error::Invalid("A stream URL has no EXTINF metadata."))?;
            let (uri, options) = value.split_once('|').unwrap_or((value, ""));
            let parsed = url::Url::parse(uri.trim())
                .map_err(|_| Error::Invalid("A channel has an invalid stream URL."))?;
            if parsed.as_str().len() > 16_384 {
                return Err(Error::Limit);
            }
            entry.stream.url = parsed.to_string();
            for (name, value) in url::form_urlencoded::parse(options.as_bytes()) {
                header(&mut entry.stream.headers, &name, &value)?;
            }
            meta.count += 1;
            if meta.count > MAX_CHANNELS {
                return Err(Error::Limit);
            }
            emit(entry)?;
        }
    }
    if !started || pending.is_some() || (meta.count == 0 && !allow_empty) {
        return Err(Error::Invalid(
            "The playlist is empty or incomplete; the previous catalogue was preserved.",
        ));
    }
    Ok(meta)
}

fn header(headers: &mut BTreeMap<String, String>, name: &str, value: &str) -> Result<()> {
    let canonical = match name.trim().to_ascii_lowercase().as_str() {
        "http-user-agent" | "user-agent" | "useragent" => "user-agent",
        "http-referrer" | "http-referer" | "referer" | "referrer" => "referer",
        _ => return Ok(()),
    };
    if value.len() > 4096 || value.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
        return Err(Error::Invalid("Unsafe provider header."));
    }
    headers.insert(canonical.to_owned(), value.trim().to_owned());
    Ok(())
}

pub fn split_unquoted(value: &str, needle: char) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in value.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == needle && !quoted {
            return Some(i);
        }
    }
    None
}

pub fn attributes(value: &str) -> Result<BTreeMap<String, String>> {
    let bytes = value.as_bytes();
    let mut i = 0;
    let mut result = BTreeMap::new();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_end = i;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key = value[start..key_end].to_ascii_lowercase();
        let text = if i < bytes.len() && bytes[i] == b'"' {
            i += 1;
            let begin = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i == bytes.len() {
                return Err(Error::Invalid("Unterminated playlist attribute."));
            }
            let text = value[begin..i].to_owned();
            i += 1;
            text
        } else {
            let begin = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            value[begin..i].to_owned()
        };
        if key.len() > 128 || text.len() > 16_384 || result.len() > 64 {
            return Err(Error::Limit);
        }
        result.insert(key, text);
    }
    Ok(result)
}
