use quick_xml::{Reader, Writer, events::{Event, BytesStart, BytesEnd, BytesText}};
use url::Url;
use crate::{error::{Error, Result}, limits::MAX_MANIFEST_BYTES};

pub fn hls(input: &str, base: &Url, mut register: impl FnMut(&str) -> Result<String>) -> Result<String> {
    let input = input.trim_start_matches('\u{feff}').trim_start();
    if input.len() > MAX_MANIFEST_BYTES || !input.starts_with("#EXTM3U") { return Err(Error::Invalid("Invalid HLS manifest.")); }
    let mut out = String::with_capacity(input.len()+1024);
    for line in input.lines() {
        let line = line.trim_end_matches('\r');
        if line.len() > 64*1024 { return Err(Error::Limit); }
        if line.contains("{$") { return Err(Error::Unsupported); }
        if line.starts_with("#EXT-X-KEY:") || line.starts_with("#EXT-X-SESSION-KEY:") {
            if (line.contains("KEYFORMAT=") && !line.contains("KEYFORMAT=\"identity\""))
                || !(line.contains("METHOD=AES-128") || line.contains("METHOD=NONE")) { return Err(Error::Unsupported); }
        }
        if line.starts_with('#') {
            let mut tail = line;
            while let Some(start) = tail.find("URI=\"") {
                out.push_str(&tail[..start+5]);
                let rest = &tail[start+5..];
                let end = rest.find('"').ok_or(Error::Invalid("Invalid HLS URI attribute."))?;
                let remote = base.join(&rest[..end]).map_err(|_|Error::Policy)?;
                out.push_str(&register(remote.as_str())?); out.push('"'); tail = &rest[end+1..];
            }
            out.push_str(tail);
        } else if !line.trim().is_empty() {
            let remote = base.join(line.trim()).map_err(|_|Error::Policy)?;
            out.push_str(&register(remote.as_str())?);
        }
        out.push('\n');
    }
    Ok(out)
}

#[derive(Clone)]
enum Child { Element(Element), Text(String) }
#[derive(Clone)]
struct Element { name: String, attributes: Vec<(String,String)>, children: Vec<Child> }
impl Element {
    fn local(&self) -> &str { self.name.rsplit(':').next().unwrap_or(&self.name) }
    fn text(&self) -> String { self.children.iter().filter_map(|c| match c { Child::Text(t)=>Some(t.as_str()),_=>None }).collect::<String>() }
}

/// Only bounded playback manifests form a tree; catalogue and XMLTV imports are streaming.
/// Rewrites URL-bearing DASH fields, including SegmentTemplate placeholders, to session resources.
pub fn dash(input: &str, base: &Url, mut register: impl FnMut(&str) -> Result<String>) -> Result<String> {
    if input.len() > MAX_MANIFEST_BYTES { return Err(Error::Limit); }
    let mut root = parse_xml(input)?;
    if root.local() != "MPD" { return Err(Error::Invalid("Invalid DASH manifest.")); }
    rewrite(&mut root,base,&mut register)?;
    let mut writer = Writer::new(Vec::with_capacity(input.len()+1024));
    write_xml(&root,&mut writer)?;
    String::from_utf8(writer.into_inner()).map_err(|_|Error::Unsupported)
}
fn parse_xml(input: &str) -> Result<Element> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Element> = vec![]; let mut root = None;
    let mut nodes = 0;
    loop {
        match reader.read_event().map_err(|_|Error::Invalid("Invalid DASH XML."))? {
            Event::Start(e) | Event::Empty(e) => {
                // Empty elements are expanded by quick-xml into matching start/end events.
                let name = std::str::from_utf8(e.name().as_ref()).map_err(|_|Error::Unsupported)?.to_owned();
                let mut attributes = Vec::new();
                for a in e.attributes() {
                    let a = a.map_err(|_|Error::Unsupported)?;
                    attributes.push((std::str::from_utf8(a.key.as_ref()).map_err(|_|Error::Unsupported)?.to_owned(),
                        a.decode_and_unescape_value(reader.decoder()).map_err(|_|Error::Unsupported)?.into_owned()));
                }
                nodes += 1;
                if nodes > 20_000 || stack.len() >= 32 { return Err(Error::Limit); }
                stack.push(Element { name,attributes,children:vec![] });
            }
            Event::End(_) => {
                let element = stack.pop().ok_or(Error::Unsupported)?;
                if let Some(parent) = stack.last_mut() { parent.children.push(Child::Element(element)); }
                else if root.replace(element).is_some() { return Err(Error::Unsupported); }
            }
            Event::Text(e) => {
                if let Some(parent) = stack.last_mut() { parent.children.push(Child::Text(e.decode().map_err(|_|Error::Unsupported)?.into_owned())); }
            }
            Event::CData(e) => {
                if let Some(parent) = stack.last_mut() { parent.children.push(Child::Text(e.decode().map_err(|_|Error::Unsupported)?.into_owned())); }
            }
            Event::GeneralRef(e) => {
                let escaped = format!("&{};", e.decode().map_err(|_|Error::Unsupported)?);
                let text = quick_xml::escape::unescape(&escaped).map_err(|_|Error::Unsupported)?.into_owned();
                if let Some(parent) = stack.last_mut() { parent.children.push(Child::Text(text)); }
            }
            Event::DocType(_) => return Err(Error::Unsupported),
            Event::Eof => break,
            _ => {}
        }
        reader.config_mut().expand_empty_elements = true;
    }
    if !stack.is_empty() { return Err(Error::Unsupported); }
    root.ok_or(Error::Unsupported)
}
fn rewrite(element: &mut Element, inherited: &Url, register: &mut impl FnMut(&str)->Result<String>) -> Result<()> {
    if element.local() == "ContentProtection" { return Err(Error::Unsupported); }
    if element.attributes.iter().any(|(k,_)| k == "xlink:href") { return Err(Error::Unsupported); }
    let chosen_base = element.children.iter().find_map(|child| match child {
        Child::Element(e) if e.local()=="BaseURL" => Some(e.text()), _=>None
    });
    let effective = match chosen_base { Some(text)=>inherited.join(text.trim()).map_err(|_|Error::Policy)?,None=>inherited.clone() };
    for (key,value) in &mut element.attributes {
        if matches!(key.as_str(),"media"|"initialization"|"sourceURL"|"index"|"href") && !value.is_empty() {
            let url = effective.join(value).map_err(|_|Error::Policy)?;
            *value = register(url.as_str())?;
        }
    }
    let mut kept_base = false;
    let mut children = Vec::with_capacity(element.children.len());
    for mut child in std::mem::take(&mut element.children) {
        if let Child::Element(e) = &mut child {
            match e.local() {
                "BaseURL" => {
                    if kept_base { continue; }
                    kept_base=true; e.attributes.clear();
                    e.children=vec![Child::Text(register(effective.as_str())?)];
                }
                "UTCTiming" | "PatchLocation" => continue,
                "Location" => {
                    let url = effective.join(e.text().trim()).map_err(|_|Error::Policy)?;
                    e.children=vec![Child::Text(register(url.as_str())?)];
                }
                _ => rewrite(e,&effective,register)?,
            }
        }
        children.push(child);
    }
    element.children=children; Ok(())
}
fn write_xml(e: &Element, writer: &mut Writer<Vec<u8>>) -> Result<()> {
    let mut start = BytesStart::new(e.name.as_str());
    for (key,value) in &e.attributes { start.push_attribute((key.as_str(),value.as_str())); }
    writer.write_event(Event::Start(start)).map_err(|_|Error::Unsupported)?;
    for child in &e.children {
        match child { Child::Element(e)=>write_xml(e,writer)?, Child::Text(t)=>writer.write_event(Event::Text(BytesText::new(t))).map_err(|_|Error::Unsupported)? }
    }
    writer.write_event(Event::End(BytesEnd::new(e.name.as_str()))).map_err(|_|Error::Unsupported)?;
    Ok(())
}
