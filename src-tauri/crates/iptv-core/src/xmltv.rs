use std::io::{self, BufRead, Read};
use chrono::{DateTime, NaiveDateTime};
use quick_xml::{Reader, events::Event};
use crate::{error::{Error, Result}, limits::{MAX_LINE, MAX_PROGRAMMES}, models::{EpgChannel, EpgProgramme}};

pub enum Record { Channel(EpgChannel), Programme(EpgProgramme) }

/// Applies an allocation budget to each XML event, before quick-xml can grow its buffer.
struct EventBudget<R> { inner: R, remaining: usize }
impl<R: BufRead> Read for EventBudget<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?; let n = available.len().min(output.len());
        output[..n].copy_from_slice(&available[..n]); self.consume(n); Ok(n)
    }
}
impl<R: BufRead> BufRead for EventBudget<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.remaining == 0 { return Err(io::Error::other("XML event limit exceeded")); }
        let buffer = self.inner.fill_buf()?;
        Ok(&buffer[..buffer.len().min(self.remaining)])
    }
    fn consume(&mut self, amount: usize) { self.remaining -= amount; self.inner.consume(amount); }
}

pub fn parse<R: BufRead>(input: R, mut emit: impl FnMut(Record) -> Result<()>) -> Result<usize> {
    let mut reader = Reader::from_reader(EventBudget { inner: input, remaining: MAX_LINE });
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::with_capacity(4096);
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut channel: Option<EpgChannel> = None;
    let mut programme: Option<EpgProgramme> = None;
    let mut text = String::new();
    let mut programmes = 0; let mut channels = 0; let mut root_seen = false;
    loop {
        reader.get_mut().remaining = MAX_LINE;
        let event = reader.read_event_into(&mut buffer).map_err(|_| Error::Invalid("The XMLTV document is malformed or exceeds a limit."))?;
        match event {
            Event::Decl(decl) => {
                if let Some(encoding) = decl.encoding() {
                    if !encoding.map_err(|_| Error::Invalid("Invalid XML encoding."))?.eq_ignore_ascii_case(b"UTF-8") {
                        return Err(Error::Invalid("XMLTV must use UTF-8."));
                    }
                }
            }
            Event::DocType(_) => return Err(Error::Invalid("XML document types and external entities are not accepted.")),
            Event::Start(ref e) | Event::Empty(ref e) => {
                let tag = e.local_name().as_ref().to_vec();
                let empty = matches!(event, Event::Empty(_));
                if stack.is_empty() {
                    if root_seen || tag != b"tv" { return Err(Error::Invalid("XMLTV needs one tv root element.")); }
                    root_seen = true;
                }
                if stack.len() >= 32 { return Err(Error::Limit); }
                if stack.len() == 1 && (tag == b"channel" || tag == b"programme") {
                    let mut attrs = std::collections::BTreeMap::new();
                    for attr in e.attributes() {
                        let attr = attr.map_err(|_| Error::Invalid("Invalid XMLTV attributes."))?;
                        let value = attr.decode_and_unescape_value(reader.decoder()).map_err(|_| Error::Invalid("Invalid XMLTV attribute text."))?.into_owned();
                        attrs.insert(attr.key.as_ref().to_vec(), value);
                    }
                    if tag == b"channel" {
                        let id = attrs.remove(b"id".as_slice()).ok_or(Error::Invalid("An EPG channel has no ID."))?;
                        if id.is_empty() || id.len() > 512 { return Err(Error::Limit); }
                        channel = Some(EpgChannel { id, names: Vec::new() });
                    } else {
                        let channel_id = attrs.remove(b"channel".as_slice()).ok_or(Error::Invalid("A programme has no channel ID."))?;
                        if channel_id.is_empty() || channel_id.len()>512 {return Err(Error::Limit);}
                        let start = timestamp(attrs.get(b"start".as_slice()).ok_or(Error::Invalid("A programme has no start time."))?)?;
                        let end = timestamp(attrs.get(b"stop".as_slice()).ok_or(Error::Invalid("A programme has no stop time; an explicit end is required."))?)?;
                        if end <= start || end-start > 7*86400 { return Err(Error::Invalid("Invalid programme interval.")); }
                        programme = Some(EpgProgramme { channel: channel_id, start, end, ..Default::default() });
                    }
                }
                text.clear();
                if empty {
                    finish(&tag, stack.len()+1, &text, &mut channel, &mut programme, &mut emit, &mut programmes, &mut channels)?;
                } else { stack.push(tag); }
            }
            Event::Text(e) => {
                let decoded = e.decode().map_err(|_| Error::Invalid("XMLTV must use UTF-8."))?;
                append(&mut text, &decoded)?;
            }
            Event::CData(e) => {
                append(&mut text, &e.decode().map_err(|_| Error::Invalid("Invalid XMLTV text."))?)?;
            }
            Event::GeneralRef(e) => {
                let reference = e.decode().map_err(|_| Error::Invalid("Invalid XML entity."))?;
                let escaped = format!("&{reference};");
                let decoded = quick_xml::escape::unescape(&escaped).map_err(|_| Error::Invalid("Unknown XML entities are not allowed."))?;
                append(&mut text, &decoded)?;
            }
            Event::End(_) => {
                let depth = stack.len();
                let tag = stack.pop().ok_or(Error::Invalid("Invalid XMLTV nesting."))?;
                finish(&tag, depth, &text, &mut channel, &mut programme, &mut emit, &mut programmes, &mut channels)?;
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen || !stack.is_empty() || channels == 0 { return Err(Error::Invalid("The XMLTV feed contains no valid channels.")); }
    Ok(programmes)
}
fn append(text: &mut String, part: &str) -> Result<()> {
    if text.len() + part.len() > MAX_LINE { return Err(Error::Limit); }
    text.push_str(part); Ok(())
}
#[allow(clippy::too_many_arguments)]
fn finish(tag: &[u8], depth: usize, text: &str, channel: &mut Option<EpgChannel>, programme: &mut Option<EpgProgramme>,
    emit: &mut impl FnMut(Record) -> Result<()>, programmes: &mut usize, channels: &mut usize) -> Result<()> {
    if depth == 3 && tag == b"display-name" {
        if let Some(c) = channel.as_mut() {
            if c.names.len() < 16 && !text.trim().is_empty() { c.names.push(text.trim().chars().take(512).collect()); }
        }
    } else if depth == 3 && tag == b"title" {
        if let Some(p) = programme.as_mut() { if p.title.is_empty() { p.title = text.trim().chars().take(512).collect(); } }
    } else if depth == 3 && tag == b"desc" {
        if let Some(p) = programme.as_mut() { if p.description.is_empty() { p.description = text.trim().chars().take(8192).collect(); } }
    } else if depth == 2 && tag == b"channel" {
        if let Some(c) = channel.take() {
            *channels += 1;
            if *channels > 250_000 { return Err(Error::Limit); }
            emit(Record::Channel(c))?;
        }
    } else if depth == 2 && tag == b"programme" {
        if let Some(p) = programme.take() {
            *programmes += 1;
            if *programmes > MAX_PROGRAMMES { return Err(Error::Limit); }
            emit(Record::Programme(p))?;
        }
    }
    Ok(())
}

pub fn timestamp(value: &str) -> Result<i64> {
    let value = value.trim();
    if value.len() == 14 {
        // Explicit application policy: a missing XMLTV offset is UTC, never the device timezone.
        return NaiveDateTime::parse_from_str(value, "%Y%m%d%H%M%S").map(|t| t.and_utc().timestamp())
            .map_err(|_| Error::Invalid("Invalid XMLTV timestamp."));
    }
    DateTime::parse_from_str(value, "%Y%m%d%H%M%S %z").map(|t| t.timestamp())
        .map_err(|_| Error::Invalid("XMLTV timestamps require second precision and a numeric UTC offset."))
}
