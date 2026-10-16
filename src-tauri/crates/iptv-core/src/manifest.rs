use crate::{
    error::{Error, Result},
    limits::MAX_MANIFEST_BYTES,
};
use quick_xml::{
    Reader, Writer, XmlVersion,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use url::Url;

/// Rewrites every URI in an HLS playlist to a session resource. In a multivariant playlist a
/// variant, rendition or hint whose address the source policy rejects is left out instead of
/// failing the whole stream; the playlist fails only when no variant remains.
pub fn hls(
    input: &str,
    base: &Url,
    mut register: impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let input = input.trim_start_matches('\u{feff}').trim_start();
    if input.len() > MAX_MANIFEST_BYTES || !input.starts_with("#EXTM3U") {
        return Err(Error::Invalid("Invalid HLS manifest."));
    }
    let mut out = String::with_capacity(input.len() + 1024);
    // An EXT-X-STREAM-INF tag waits for its URI line so a rejected variant drops as a pair.
    let mut variant: Option<String> = None;
    let mut kept = 0;
    let mut dropped = 0;
    for line in input.lines() {
        let line = line.trim_end_matches('\r');
        if line.len() > 64 * 1024 {
            return Err(Error::Limit);
        }
        if line.contains("{$") {
            return Err(Error::Unsupported);
        }
        if line.starts_with("#EXT-X-KEY:") || line.starts_with("#EXT-X-SESSION-KEY:") {
            check_key(line)?;
        }
        if line.starts_with('#') {
            let rewritten = match tag(line, base, &mut register) {
                Ok(rewritten) => rewritten,
                Err(error) if optional(line) && droppable(&error) => continue,
                Err(error) => return Err(error),
            };
            if line.starts_with("#EXT-X-STREAM-INF:") {
                if let Some(orphan) = variant.take() {
                    out.push_str(&orphan);
                }
                variant = Some(format!("{rewritten}\n"));
            } else {
                let target = variant.as_mut().unwrap_or(&mut out);
                target.push_str(&rewritten);
                target.push('\n');
            }
        } else if line.trim().is_empty() {
            variant.as_mut().unwrap_or(&mut out).push('\n');
        } else {
            let local = base
                .join(line.trim())
                .map_err(|_| Error::Policy)
                .and_then(|remote| register(remote.as_str()));
            match (variant.take(), local) {
                (Some(info), Ok(local)) => {
                    out.push_str(&info);
                    out.push_str(&local);
                    out.push('\n');
                    kept += 1;
                }
                (Some(_), Err(error)) if droppable(&error) => dropped += 1,
                (_, Err(error)) => return Err(error),
                (None, Ok(local)) => {
                    out.push_str(&local);
                    out.push('\n');
                }
            }
        }
    }
    if let Some(orphan) = variant {
        out.push_str(&orphan);
    }
    if dropped > 0 && kept == 0 {
        return Err(Error::Policy);
    }
    Ok(out)
}
/// Rewrites the quoted URI attributes of one tag line.
fn tag(
    line: &str,
    base: &Url,
    register: &mut impl FnMut(&str) -> Result<String>,
) -> Result<String> {
    let mut out = String::with_capacity(line.len() + 64);
    let mut tail = line;
    while let Some(start) = tail.find("URI=\"") {
        out.push_str(&tail[..start + 5]);
        let rest = &tail[start + 5..];
        let end = rest
            .find('"')
            .ok_or(Error::Invalid("Invalid HLS URI attribute."))?;
        let remote = base.join(&rest[..end]).map_err(|_| Error::Policy)?;
        out.push_str(&register(remote.as_str())?);
        out.push('"');
        tail = &rest[end + 1..];
    }
    out.push_str(tail);
    Ok(out)
}
/// Tags a player can do without: alternatives, hints and session metadata.
fn optional(line: &str) -> bool {
    [
        "#EXT-X-MEDIA:",
        "#EXT-X-I-FRAME-STREAM-INF:",
        "#EXT-X-IMAGE-STREAM-INF:",
        "#EXT-X-SESSION-DATA:",
        "#EXT-X-SESSION-KEY:",
        "#EXT-X-CONTENT-STEERING:",
        "#EXT-X-RENDITION-REPORT:",
        "#EXT-X-PRELOAD-HINT:",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}
/// Address rejections, as opposed to limits or storage failures that must surface.
fn droppable(error: &Error) -> bool {
    matches!(error, Error::Policy | Error::Invalid(_))
}
/// Clear AES-128 and identity SAMPLE-AES keys can be relayed; DRM key systems cannot.
fn check_key(line: &str) -> Result<()> {
    let method = attribute(line, "METHOD").unwrap_or_default();
    let format = attribute(line, "KEYFORMAT").unwrap_or("identity");
    if format != "identity" || !matches!(method, "NONE" | "AES-128" | "SAMPLE-AES") {
        return Err(Error::Unsupported);
    }
    Ok(())
}
/// One attribute of an HLS tag's attribute list, without quotes.
fn attribute<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = line.split_once(':')?.1;
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=')?;
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], quoted[end + 1..].trim_start_matches(','))
        } else {
            after.split_once(',').unwrap_or((after, ""))
        };
        if key.trim() == name {
            return Some(value);
        }
        rest = next;
    }
    None
}

#[derive(Clone)]
enum Child {
    Element(Element),
    Text(String),
}
#[derive(Clone)]
struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Child>,
}
impl Element {
    fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
    fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|c| match c {
                Child::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<String>()
    }
}

/// Only bounded playback manifests form a tree; catalogue and XMLTV imports are streaming.
/// Rewrites URL-bearing DASH fields to session resources. `register` receives each absolute
/// address and whether it is a SegmentTemplate string whose `$Identifier$` placeholders the
/// player fills in.
pub fn dash(
    input: &str,
    base: &Url,
    mut register: impl FnMut(&str, bool) -> Result<String>,
) -> Result<String> {
    if input.len() > MAX_MANIFEST_BYTES {
        return Err(Error::Limit);
    }
    let mut root = parse_xml(input)?;
    if root.local() != "MPD" {
        return Err(Error::Invalid("Invalid DASH manifest."));
    }
    rewrite(&mut root, base, &mut register)?;
    let mut writer = Writer::new(Vec::with_capacity(input.len() + 1024));
    write_xml(&root, &mut writer)?;
    String::from_utf8(writer.into_inner()).map_err(|_| Error::Unsupported)
}
fn parse_xml(input: &str) -> Result<Element> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Element> = vec![];
    let mut root = None;
    let mut nodes = 0;
    loop {
        match reader
            .read_event()
            .map_err(|_| Error::Invalid("Invalid DASH XML."))?
        {
            Event::Start(e) | Event::Empty(e) => {
                // Empty elements are expanded by quick-xml into matching start/end events.
                let name = e.name().as_ref().to_owned();
                let mut attributes = Vec::new();
                for a in e.attributes() {
                    let a = a.map_err(|_| Error::Unsupported)?;
                    attributes.push((
                        a.key.as_ref().to_owned(),
                        a.normalized_value(XmlVersion::Implicit1_0)
                            .map_err(|_| Error::Unsupported)?
                            .into_owned(),
                    ));
                }
                nodes += 1;
                if nodes > 20_000 || stack.len() >= 32 {
                    return Err(Error::Limit);
                }
                stack.push(Element {
                    name,
                    attributes,
                    children: vec![],
                });
            }
            Event::End(_) => {
                let element = stack.pop().ok_or(Error::Unsupported)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Child::Element(element));
                } else if root.replace(element).is_some() {
                    return Err(Error::Unsupported);
                }
            }
            Event::Text(e) => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .children
                        .push(Child::Text(e.xml10_content().into_owned()));
                }
            }
            Event::CData(e) => {
                if let Some(parent) = stack.last_mut() {
                    parent
                        .children
                        .push(Child::Text(e.xml10_content().into_owned()));
                }
            }
            Event::GeneralRef(e) => {
                let escaped = format!("&{};", &*e);
                let text = quick_xml::escape::unescape(&escaped)
                    .map_err(|_| Error::Unsupported)?
                    .into_owned();
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Child::Text(text));
                }
            }
            Event::DocType(_) => return Err(Error::Unsupported),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(Error::Unsupported);
    }
    root.ok_or(Error::Unsupported)
}
fn rewrite(
    element: &mut Element,
    inherited: &Url,
    register: &mut impl FnMut(&str, bool) -> Result<String>,
) -> Result<()> {
    if element.local() == "ContentProtection" {
        return Err(Error::Unsupported);
    }
    if element.attributes.iter().any(|(k, _)| k == "xlink:href") {
        return Err(Error::Unsupported);
    }
    let chosen_base = element.children.iter().find_map(|child| match child {
        Child::Element(e) if e.local() == "BaseURL" => Some(e.text()),
        _ => None,
    });
    let effective = match chosen_base {
        Some(text) => inherited.join(text.trim()).map_err(|_| Error::Policy)?,
        None => inherited.clone(),
    };
    let template = element.local() == "SegmentTemplate";
    for (key, value) in &mut element.attributes {
        if matches!(
            key.as_str(),
            "media" | "initialization" | "sourceURL" | "index" | "href"
        ) && !value.is_empty()
        {
            let url = effective.join(value).map_err(|_| Error::Policy)?;
            *value = register(url.as_str(), template)?;
        }
    }
    let mut kept_base = false;
    let mut children = Vec::with_capacity(element.children.len());
    for mut child in std::mem::take(&mut element.children) {
        if let Child::Element(e) = &mut child {
            match e.local() {
                "BaseURL" => {
                    if kept_base {
                        continue;
                    }
                    kept_base = true;
                    e.attributes.clear();
                    e.children = vec![Child::Text(register(effective.as_str(), false)?)];
                }
                "UTCTiming" => {
                    if !timing(e, &effective, register) {
                        continue;
                    }
                }
                "PatchLocation" => continue,
                "Location" => {
                    let url = effective.join(e.text().trim()).map_err(|_| Error::Policy)?;
                    e.children = vec![Child::Text(register(url.as_str(), false)?)];
                }
                _ => rewrite(e, &effective, register)?,
            }
        }
        children.push(child);
    }
    element.children = children;
    Ok(())
}
/// Keeps a UTCTiming source the player can use. Live DASH players fail when no clock source
/// answers, and the WebView may reach only the relay: HTTP sources are relayed when the source
/// policy permits them, direct values stay, and anything else is dropped.
fn timing(
    element: &mut Element,
    base: &Url,
    register: &mut impl FnMut(&str, bool) -> Result<String>,
) -> bool {
    let scheme = element
        .attributes
        .iter()
        .find(|(k, _)| k == "schemeIdUri")
        .map(|(_, v)| v.as_str())
        .unwrap_or_default();
    if scheme.starts_with("urn:mpeg:dash:utc:direct:") {
        return true;
    }
    if !scheme.starts_with("urn:mpeg:dash:utc:http-") {
        return false;
    }
    let Some((_, value)) = element.attributes.iter_mut().find(|(k, _)| k == "value") else {
        return false;
    };
    let local: Vec<String> = value
        .split_whitespace()
        .filter_map(|candidate| base.join(candidate).ok())
        .filter_map(|url| register(url.as_str(), false).ok())
        .collect();
    if local.is_empty() {
        return false;
    }
    *value = local.join(" ");
    true
}
fn write_xml(e: &Element, writer: &mut Writer<Vec<u8>>) -> Result<()> {
    let mut start = BytesStart::new(e.name.as_str());
    for (key, value) in &e.attributes {
        start.push_attribute((key.as_str(), value.as_str()));
    }
    writer
        .write_event(Event::Start(start))
        .map_err(|_| Error::Unsupported)?;
    for child in &e.children {
        match child {
            Child::Element(e) => write_xml(e, writer)?,
            Child::Text(t) => writer
                .write_event(Event::Text(BytesText::new(t)))
                .map_err(|_| Error::Unsupported)?,
        }
    }
    writer
        .write_event(Event::End(BytesEnd::new(e.name.as_str())))
        .map_err(|_| Error::Unsupported)?;
    Ok(())
}
