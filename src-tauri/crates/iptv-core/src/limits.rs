use std::io::{self, BufRead, Read};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use crate::error::{Error, Result};

pub const MAX_CHANNELS: usize = 250_000;
pub const MAX_PROGRAMMES: usize = 2_000_000;
pub const MAX_LINE: usize = 64 * 1024;
pub const MAX_M3U_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_XML_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;

pub struct LimitedRead<R> { inner: R, remaining: u64, cancel: Arc<AtomicBool> }
impl<R> LimitedRead<R> {
    pub fn new(inner: R, limit: u64, cancel: Arc<AtomicBool>) -> Self { Self { inner, remaining: limit, cancel } }
}
impl<R: Read> Read for LimitedRead<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() { return Ok(0); }
        if self.cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled")); }
        if self.remaining == 0 {
            let mut probe = [0u8;1];
            return match self.inner.read(&mut probe)? { 0 => Ok(0), _ => Err(io::Error::other("input limit exceeded")) };
        }
        let take = out.len().min(self.remaining as usize);
        let count = self.inner.read(&mut out[..take])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

pub fn bounded_line<R: BufRead>(reader: &mut R, line: &mut String) -> Result<bool> {
    let mut bytes = Vec::with_capacity(256);
    loop {
        let buf = reader.fill_buf().map_err(|_| Error::Invalid("Playlist input was interrupted or too large."))?;
        if buf.is_empty() { break; }
        let take = buf.iter().position(|b| *b == b'\n').map_or(buf.len(), |i| i + 1);
        if bytes.len() + take > MAX_LINE { return Err(Error::Limit); }
        let ends = buf[take-1] == b'\n';
        bytes.extend_from_slice(&buf[..take]); reader.consume(take);
        if ends { break; }
    }
    let present = !bytes.is_empty();
    *line = String::from_utf8(bytes).map_err(|_| Error::Invalid("Playlists must be UTF-8."))?;
    Ok(present)
}
