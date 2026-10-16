use crate::{
    error::{Error, Result},
    limits::MAX_MANIFEST_BYTES,
    manifest,
    models::{SourceInput, StreamSecret},
    network::{MediaNetwork, NetworkPolicy},
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, watch};
use url::Url;
use uuid::Uuid;

/// Registered upstream addresses per session. Entries live as long as the session; when the
/// bound is reached the least recently used quarter is evicted, so a long live session stays
/// bounded while an on-demand playlist keeps every segment it announced.
const MAX_RESOURCES: usize = 16_384;
/// How long the probe's open response waits for the player's first request of the root.
const PARK_TTL: Duration = Duration::from_secs(20);
const PROBE_BYTES: usize = 4096;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

struct Resource {
    target: String,
    placeholders: Vec<String>,
    seen: Instant,
}
#[derive(Default)]
struct Registry {
    entries: HashMap<String, Resource>,
}
impl Registry {
    fn insert(&mut self, root: &str, token: &str, target: &str, placeholders: Vec<String>) {
        let now = Instant::now();
        if let Some(existing) = self.entries.get_mut(token) {
            existing.seen = now;
            return;
        }
        if self.entries.len() >= MAX_RESOURCES {
            let mut ages: Vec<(Instant, String)> = self
                .entries
                .iter()
                .filter(|(key, _)| key.as_str() != root)
                .map(|(key, resource)| (resource.seen, key.clone()))
                .collect();
            ages.sort_unstable();
            for (_, key) in ages.into_iter().take(MAX_RESOURCES / 4) {
                self.entries.remove(&key);
            }
        }
        self.entries.insert(
            token.to_owned(),
            Resource {
                target: target.to_owned(),
                placeholders,
                seen: now,
            },
        );
    }
    fn touch(&mut self, token: &str) -> Option<&Resource> {
        let resource = self.entries.get_mut(token)?;
        resource.seen = Instant::now();
        Some(resource)
    }
}
/// Session resource IDs are derived from the address, so re-announced segments keep their URL.
fn token(namespace: &Uuid, target: &str, template: bool) -> String {
    let kind = if template { "t" } else { "u" };
    Uuid::new_v5(namespace, format!("{kind}:{target}").as_bytes()).to_string()
}
/// DASH SegmentTemplate identifiers the player substitutes into a template address.
fn placeholders(target: &str) -> Result<Vec<String>> {
    let mut placeholders = vec![];
    let mut position = 0;
    while let Some(start) = target[position..].find('$') {
        let start = position + start;
        let end = start + 1 + target[start + 1..].find('$').ok_or(Error::Unsupported)?;
        let full = &target[start..=end];
        let name = &target[start + 1..end];
        let bare = name.split('%').next().unwrap_or_default();
        if !matches!(bare, "RepresentationID" | "Bandwidth" | "Number" | "Time")
            || placeholders.len() >= 8
        {
            return Err(Error::Unsupported);
        }
        placeholders.push(full.to_owned());
        position = end + 1;
    }
    Ok(placeholders)
}
type ByteStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<Bytes>> + Send>>;
/// The probe's upstream response, handed to the player's first plain request for the root.
/// The stream then starts without a second upstream connection, which single-connection
/// providers and one-time addresses refuse, and without a second handshake.
struct Parked {
    status: u16,
    final_url: Url,
    headers: reqwest::header::HeaderMap,
    prefix: Vec<u8>,
    stream: ByteStream,
    at: Instant,
}
struct Session {
    id: String,
    namespace: Uuid,
    network: MediaNetwork,
    resources: Mutex<Registry>,
    cancel: watch::Sender<bool>,
    requests: Arc<Semaphore>,
    root: String,
    kind: String,
    parked: Mutex<Option<Parked>>,
}
impl Session {
    fn register(&self, base: &str, target: &str, template: bool) -> Result<String> {
        let mut url = Url::parse(target).map_err(|_| Error::Policy)?;
        // Fragments never reach the server; dropping them keeps the address policy exact.
        url.set_fragment(None);
        self.network.check(&url)?;
        let target = url.as_str();
        let placeholders = if template {
            placeholders(target)?
        } else {
            vec![]
        };
        let token = token(&self.namespace, target, template);
        let suffix = if placeholders.is_empty() {
            String::new()
        } else {
            format!("/{}", placeholders.join("/"))
        };
        self.resources.lock().map_err(|_| Error::Storage)?.insert(
            &self.root,
            &token,
            target,
            placeholders,
        );
        Ok(format!("{base}/media/{}/{token}{suffix}", self.id))
    }
    fn resolve(&self, token: &str, values: &str) -> Result<Url> {
        let mut resources = self.resources.lock().map_err(|_| Error::Storage)?;
        let resource = resources.touch(token).ok_or(Error::NotFound)?;
        let values: Vec<&str> = if values.is_empty() {
            vec![]
        } else {
            values.split('/').collect()
        };
        if values.len() != resource.placeholders.len() {
            return Err(Error::Policy);
        }
        let mut target = resource.target.clone();
        for (placeholder, value) in resource.placeholders.iter().zip(values) {
            if value.is_empty()
                || value.len() > 256
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || value == ".."
            {
                return Err(Error::Policy);
            }
            target = target.replacen(placeholder, value, 1);
        }
        let url = Url::parse(&target).map_err(|_| Error::Policy)?;
        self.network.check(&url)?;
        Ok(url)
    }
    fn take_parked(&self) -> Option<Parked> {
        let parked = self.parked.lock().ok()?.take()?;
        (parked.at.elapsed() < PARK_TTL).then_some(parked)
    }
    /// Cancels in-flight transfers and releases a probe connection nobody claimed.
    fn end(&self) {
        self.cancel.send_replace(true);
        drop(self.parked.lock().ok().and_then(|mut parked| parked.take()));
    }
}
struct Inner {
    base: String,
    current: Mutex<Option<Arc<Session>>>,
    epoch: AtomicU64,
    /// Unguessable path segment for artwork requests; the UI learns it only through DTOs.
    logo_token: String,
    logos: Mutex<Option<Weak<crate::Core>>>,
}
#[derive(Clone)]
pub struct Relay {
    inner: Arc<Inner>,
}
#[derive(Clone)]
struct HttpState {
    inner: Arc<Inner>,
}

pub struct Prepared {
    pub id: String,
    pub url: String,
    pub kind: String,
}
impl Relay {
    pub async fn bind() -> Result<Self> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Error::Network)?;
        let port = listener.local_addr().map_err(|_| Error::Network)?.port();
        let inner = Arc::new(Inner {
            base: format!("http://127.0.0.1:{port}"),
            current: Mutex::new(None),
            epoch: AtomicU64::new(0),
            logo_token: Uuid::new_v4().to_string(),
            logos: Mutex::new(None),
        });
        let router = Router::new()
            .route(
                "/media/{session}/{resource}",
                get(resource).options(preflight),
            )
            .route(
                "/media/{session}/{resource}/{*values}",
                get(template).options(preflight),
            )
            .route("/logo/{token}/{channel}", get(logo))
            .with_state(HttpState {
                inner: inner.clone(),
            });
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self { inner })
    }
    pub async fn start(&self, secret: StreamSecret, config: &SourceInput) -> Result<Prepared> {
        let ticket = self.inner.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let mut root = Url::parse(&secret.url).map_err(|_| Error::Unsupported)?;
        if !matches!(root.scheme(), "http" | "https") {
            return Err(Error::Unsupported);
        }
        root.set_fragment(None);
        let network = MediaNetwork::new(
            NetworkPolicy::new(root.as_str(), config).map_err(stream_error)?,
            secret.headers,
        );
        // A bounded probe determines the engine without assuming a filename extension. It is a
        // plain GET: some stream hosts reject Range requests on playlists. The open response is
        // parked for the player instead of being discarded.
        let response = network
            .get(root.clone(), None, false)
            .await
            .map_err(stream_error)?;
        let status = response.status().as_u16();
        let final_url = response.url().clone();
        let headers = response.headers().clone();
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let mut stream: ByteStream = Box::pin(response.bytes_stream());
        let mut prefix = Vec::with_capacity(PROBE_BYTES + 16 * 1024);
        let read = async {
            while prefix.len() < PROBE_BYTES {
                match stream.next().await {
                    Some(Ok(chunk)) => prefix.extend_from_slice(&chunk),
                    Some(Err(_)) => return Err(Error::Stream),
                    None => break,
                }
            }
            Ok(())
        };
        match tokio::time::timeout(PROBE_TIMEOUT, read).await {
            Ok(result) => result?,
            // A slow stream that already sent something is still identified from that prefix.
            Err(_) if !prefix.is_empty() => {}
            Err(_) => return Err(Error::Stream),
        }
        let kind = detect_kind(&prefix, &content_type, &[root.path(), final_url.path()]);
        let namespace = Uuid::new_v4();
        let (cancel, _) = watch::channel(false);
        let parked = (200..300).contains(&status).then(|| Parked {
            status,
            final_url,
            headers,
            prefix,
            stream,
            at: Instant::now(),
        });
        let session = Arc::new(Session {
            id: namespace.to_string(),
            namespace,
            network,
            resources: Mutex::new(Registry::default()),
            cancel,
            requests: Arc::new(Semaphore::new(6)),
            root: token(&namespace, root.as_str(), false),
            kind: kind.to_owned(),
            parked: Mutex::new(parked),
        });
        let url = session.register(&self.inner.base, root.as_str(), false)?;
        {
            let mut current = self.inner.current.lock().map_err(|_| Error::Storage)?;
            if self.inner.epoch.load(Ordering::SeqCst) != ticket {
                return Err(Error::Cancelled);
            }
            if let Some(old) = current.replace(session.clone()) {
                old.end();
            }
        }
        let unclaimed = Arc::downgrade(&session);
        tokio::spawn(async move {
            tokio::time::sleep(PARK_TTL).await;
            if let Some(session) = unclaimed.upgrade() {
                drop(session.take_parked());
            }
        });
        Ok(Prepared {
            id: session.id.clone(),
            url,
            kind: kind.to_owned(),
        })
    }
    pub fn close(&self, id: Option<&str>) {
        if let Ok(mut guard) = self.inner.current.lock() {
            if id.is_none() {
                // Locking or removing a source also invalidates a session still being prepared.
                self.inner.epoch.fetch_add(1, Ordering::SeqCst);
            }
            // Closing one finished session must not cancel the next session being prepared.
            if (id.is_none() || guard.as_ref().is_some_and(|s| Some(s.id.as_str()) == id))
                && let Some(session) = guard.take()
            {
                session.end();
            }
        }
    }
    /// Gives artwork requests access to the catalogue; called once after `Core` exists.
    pub fn attach(&self, core: &Arc<crate::Core>) {
        if let Ok(mut logos) = self.inner.logos.lock() {
            *logos = Some(Arc::downgrade(core));
        }
    }
    pub fn logo_url(&self, channel: &str) -> String {
        format!(
            "{}/logo/{}/{channel}",
            self.inner.base, self.inner.logo_token
        )
    }
}
fn stream_error(error: Error) -> Error {
    match error {
        Error::Network => Error::Stream,
        Error::Policy => Error::StreamPolicy,
        other => other,
    }
}
fn check_host(state: &HttpState, headers: &HeaderMap) -> Result<()> {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Policy)?;
    if Some(host) != state.inner.base.strip_prefix("http://") {
        return Err(Error::Policy);
    }
    Ok(())
}
async fn logo(
    State(state): State<HttpState>,
    Path((token, channel)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let result = async {
        check_host(&state, &headers)?;
        if token != state.inner.logo_token {
            return Err(Error::NotFound);
        }
        crate::db::valid_id(&channel)?;
        let core = {
            let logos = state.inner.logos.lock().map_err(|_| Error::Storage)?;
            logos.as_ref().and_then(Weak::upgrade)
        }
        .ok_or(Error::NotFound)?;
        let (bytes, content_type) = core.logo(&channel).await?;
        let mut response = (StatusCode::OK, bytes).into_response();
        let headers = response.headers_mut();
        headers.insert("content-type", HeaderValue::from_static(content_type));
        headers.insert(
            "cache-control",
            HeaderValue::from_static("private, max-age=86400"),
        );
        headers.insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(
            "content-security-policy",
            HeaderValue::from_static("default-src 'none'"),
        );
        Ok(response)
    }
    .await;
    result.unwrap_or_else(public_http_error)
}
fn origin(headers: &HeaderMap) -> Result<Option<HeaderValue>> {
    let Some(value) = headers.get("origin") else {
        return Ok(None);
    };
    let value_str = value.to_str().map_err(|_| Error::Policy)?;
    if !matches!(
        value_str,
        "http://tauri.localhost"
            | "https://tauri.localhost"
            | "tauri://localhost"
            | "http://localhost:1420"
            | "http://127.0.0.1:1420"
    ) {
        return Err(Error::Policy);
    }
    Ok(Some(value.clone()))
}
fn session(state: &HttpState, id: &str, headers: &HeaderMap) -> Result<Arc<Session>> {
    check_host(state, headers)?;
    origin(headers)?;
    let guard = state.inner.current.lock().map_err(|_| Error::Storage)?;
    let current = guard.as_ref().ok_or(Error::NotFound)?;
    if current.id != id || *current.cancel.borrow() {
        return Err(Error::NotFound);
    }
    Ok(current.clone())
}
async fn preflight(State(state): State<HttpState>, headers: HeaderMap) -> Response {
    // Preflight does not expose data or authorize a session; GET still validates tokens.
    let result = (|| -> Result<Response> {
        let host = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .ok_or(Error::Policy)?;
        if Some(host) != state.inner.base.strip_prefix("http://") {
            return Err(Error::Policy);
        }
        let mut response = StatusCode::NO_CONTENT.into_response();
        if let Some(value) = origin(&headers)? {
            response
                .headers_mut()
                .insert("access-control-allow-origin", value);
        }
        response.headers_mut().insert(
            "access-control-allow-methods",
            HeaderValue::from_static("GET, HEAD, OPTIONS"),
        );
        response.headers_mut().insert(
            "access-control-allow-headers",
            HeaderValue::from_static("range"),
        );
        response
            .headers_mut()
            .insert("access-control-max-age", HeaderValue::from_static("600"));
        Ok(response)
    })();
    result.unwrap_or_else(public_http_error)
}
async fn resource(
    State(state): State<HttpState>,
    Path((sid, rid)): Path<(String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let allowed = origin(&headers).ok().flatten();
    let response = dispatch(state, sid, rid, String::new(), headers, method)
        .await
        .unwrap_or_else(public_http_error);
    finish(response, allowed)
}
async fn template(
    State(state): State<HttpState>,
    Path((sid, rid, values)): Path<(String, String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let allowed = origin(&headers).ok().flatten();
    let response = dispatch(state, sid, rid, values, headers, method)
        .await
        .unwrap_or_else(public_http_error);
    finish(response, allowed)
}
/// Media responses, errors included, carry CORS headers for the WebView: without them a
/// script engine sees a refused relay answer as a lost connection and cannot name the cause.
fn finish(mut response: Response, allowed: Option<HeaderValue>) -> Response {
    let headers = response.headers_mut();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    // DASH players read the manifest's Date to synchronise their clock with the packager.
    headers.insert(
        "access-control-expose-headers",
        HeaderValue::from_static("content-length, content-range, accept-ranges, date"),
    );
    headers.insert("vary", HeaderValue::from_static("Origin"));
    if let Some(value) = allowed {
        headers.insert("access-control-allow-origin", value);
    }
    response
}
/// Resources whose names mark them as playlists. Upstream playlist requests never carry a
/// Range header and never use HEAD: many stream hosts answer those probes with a two-byte
/// 206, 405 or 5xx, and native players report any failure of the main resource as an
/// unsupported format.
pub fn playlist_like(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".m3u") || path.ends_with(".mpd")
}
/// The manifest family a response body starts with, if any.
pub fn manifest_prefix(text: &str) -> Option<&'static str> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("#EXTM3U") {
        Some("hls")
    } else if text.contains("<MPD") {
        Some("dash")
    } else {
        None
    }
}
/// The playback engine for a probed stream: the body's own signature first, then the
/// declared content type, then the address. Live MPEG-TS may start mid-packet, so the sync
/// pattern is searched across one packet length.
pub fn detect_kind(prefix: &[u8], content_type: &str, paths: &[&str]) -> &'static str {
    let content_type = content_type.to_ascii_lowercase();
    let ends = |suffix: &str| {
        paths
            .iter()
            .any(|path| path.to_ascii_lowercase().ends_with(suffix))
    };
    let head = &prefix[..prefix.len().min(PROBE_BYTES)];
    if let Some(kind) = manifest_prefix(&String::from_utf8_lossy(head)) {
        return kind;
    }
    let ts = (0..188).any(|offset| (0..4).all(|k| head.get(offset + k * 188) == Some(&0x47)));
    if ts {
        "mpegts"
    } else if head.starts_with(b"FLV\x01") {
        "flv"
    } else if content_type.contains("mpegurl") || ends(".m3u8") {
        "hls"
    } else if content_type.contains("dash+xml") || ends(".mpd") {
        "dash"
    } else if content_type.contains("mp2t") || ends(".ts") {
        "mpegts"
    } else if content_type.contains("x-flv") || ends(".flv") {
        "flv"
    } else {
        "progressive"
    }
}
/// The content type passed to the player for a media resource. Text and JSON labels on media
/// would let the WebView's cross-origin read blocking empty a `<video>` response.
fn media_type(upstream: Option<&HeaderValue>, container: Option<&str>) -> HeaderValue {
    match container {
        Some("mpegts") => return HeaderValue::from_static("video/mp2t"),
        Some("flv") => return HeaderValue::from_static("video/x-flv"),
        _ => {}
    }
    let Some(value) = upstream else {
        return HeaderValue::from_static("application/octet-stream");
    };
    let lower = value.to_str().unwrap_or_default().to_ascii_lowercase();
    if lower.starts_with("text/html") || lower.starts_with("text/plain") || lower.contains("json") {
        HeaderValue::from_static("application/octet-stream")
    } else {
        value.clone()
    }
}
async fn dispatch(
    state: HttpState,
    sid: String,
    rid: String,
    values: String,
    headers: HeaderMap,
    method: Method,
) -> Result<Response> {
    let session = session(&state, &sid, &headers)?;
    let url = session.resolve(&rid, &values)?;
    let mut cancellation = session.cancel.subscribe();
    let permit = tokio::time::timeout(
        Duration::from_secs(3),
        session.requests.clone().acquire_owned(),
    )
    .await
    .map_err(|_| Error::Busy)?
    .map_err(|_| Error::Cancelled)?;
    let range = headers
        .get("range")
        .map(|v| v.to_str().map_err(|_| Error::Policy))
        .transpose()?;
    let root = rid == session.root;
    let manifest_role =
        (root && matches!(session.kind.as_str(), "hls" | "dash")) || playlist_like(&url);
    // HEAD is answered from a GET; a byte range is only forwarded for media resources.
    let mut attempt_range = if manifest_role { None } else { range };
    let mut parked = if root && attempt_range.is_none() && method == Method::GET {
        session.take_parked()
    } else {
        None
    };
    let (status, final_url, upstream_headers, prefix, remainder, incoming) = loop {
        let (status, final_url, upstream_headers, mut prefix, mut incoming) = match parked.take() {
            Some(parked) => (
                StatusCode::from_u16(parked.status).map_err(|_| Error::Network)?,
                parked.final_url,
                parked.headers,
                parked.prefix,
                parked.stream,
            ),
            None => {
                let upstream = tokio::select! {
                    _ = cancellation.changed() => return Err(Error::Cancelled),
                    value = session.network.get(url.clone(), attempt_range, false) => value?,
                };
                let status =
                    StatusCode::from_u16(upstream.status().as_u16()).map_err(|_| Error::Network)?;
                let final_url = upstream.url().clone();
                let upstream_headers = upstream.headers().clone();
                let incoming: ByteStream = Box::pin(upstream.bytes_stream());
                (
                    status,
                    final_url,
                    upstream_headers,
                    Vec::with_capacity(512),
                    incoming,
                )
            }
        };
        let mut remainder = None;
        if status.is_success() {
            while prefix.len() < 512 {
                let next = tokio::select! {
                    _ = cancellation.changed() => return Err(Error::Cancelled),
                    value = incoming.next() => value,
                };
                match next {
                    Some(Ok(bytes)) => {
                        let n = bytes.len().min(512 - prefix.len());
                        prefix.extend_from_slice(&bytes[..n]);
                        if n < bytes.len() {
                            remainder = Some(bytes.slice(n..));
                            break;
                        }
                    }
                    Some(Err(_)) => return Err(Error::Network),
                    None => break,
                }
            }
        }
        let content_type = upstream_headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let looks_like_manifest = content_type.contains("mpegurl")
            || content_type.contains("dash+xml")
            || playlist_like(&final_url)
            || manifest_prefix(&String::from_utf8_lossy(&prefix)).is_some();
        if attempt_range.is_some() && looks_like_manifest && status == StatusCode::PARTIAL_CONTENT {
            // The host honoured a byte range on a playlist; fetch the whole document instead.
            attempt_range = None;
            continue;
        }
        break (
            status,
            final_url,
            upstream_headers,
            prefix,
            remainder,
            incoming,
        );
    };
    let content_type = upstream_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    // The same rule as the probe: a `.m3u8` address that serves MPEG-TS is media, not a playlist.
    let kind = detect_kind(&prefix, content_type, &[final_url.path()]);
    let (hls, dash) = (kind == "hls", kind == "dash");
    let buffered =
        std::iter::once(Ok::<Bytes, reqwest::Error>(Bytes::from(prefix))).chain(remainder.map(Ok));
    let mut stream = futures_util::stream::iter(buffered).chain(incoming);
    let mut response = if (hls || dash) && status.is_success() {
        let manifest_type = HeaderValue::from_static(if hls {
            "application/vnd.apple.mpegurl"
        } else {
            "application/dash+xml"
        });
        if method == Method::HEAD {
            drop(permit);
            let mut response = StatusCode::OK.into_response();
            response.headers_mut().insert("content-type", manifest_type);
            response
        } else {
            let mut data = Vec::with_capacity(16 * 1024);
            loop {
                let chunk = tokio::select! {
                    _ = cancellation.changed() => return Err(Error::Cancelled),
                    value = stream.next() => value,
                };
                match chunk {
                    Some(Ok(bytes)) => {
                        if data.len() + bytes.len() > MAX_MANIFEST_BYTES {
                            return Err(Error::Limit);
                        }
                        data.extend_from_slice(&bytes);
                    }
                    Some(Err(_)) => return Err(Error::Network),
                    None => break,
                }
            }
            let input = String::from_utf8(data).map_err(|_| Error::Unsupported)?;
            let rewritten = if hls {
                manifest::hls(&input, &final_url, |url| {
                    session.register(&state.inner.base, url, false)
                })?
            } else {
                manifest::dash(&input, &final_url, |url, template| {
                    session.register(&state.inner.base, url, template)
                })?
            };
            drop(permit);
            let mut response = (StatusCode::OK, rewritten).into_response();
            response.headers_mut().insert("content-type", manifest_type);
            response
        }
    } else {
        let container = root.then_some(session.kind.as_str());
        let body = if method == Method::HEAD {
            drop(permit);
            Body::empty()
        } else {
            Body::from_stream(futures_util::stream::unfold(
                (Box::pin(stream), cancellation, permit),
                |(mut stream, mut cancel, permit)| async move {
                    if *cancel.borrow() {
                        return None;
                    }
                    let next = tokio::select! {
                        _ = cancel.changed() => return None,
                        value = stream.next() => value,
                    };
                    next.map(|value| {
                        (
                            value.map_err(|_| std::io::Error::other("Media transport interrupted")),
                            (stream, cancel, permit),
                        )
                    })
                },
            ))
        };
        let mut response = Response::new(body);
        *response.status_mut() = status;
        let headers = response.headers_mut();
        headers.insert(
            "content-type",
            media_type(upstream_headers.get("content-type"), container),
        );
        for name in ["content-length", "content-range", "accept-ranges"] {
            if let Some(value) = upstream_headers.get(name) {
                headers.insert(name, value.clone());
            }
        }
        response
    };
    if let Some(date) = upstream_headers.get("date") {
        response.headers_mut().insert("date", date.clone());
    }
    Ok(response)
}
fn public_http_error(error: Error) -> Response {
    let status = match error {
        Error::NotFound | Error::Cancelled => StatusCode::GONE,
        Error::Policy | Error::StreamPolicy => StatusCode::FORBIDDEN,
        Error::Limit => StatusCode::PAYLOAD_TOO_LARGE,
        Error::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        Error::Busy => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, error.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_evicts_least_recently_used_entries_and_keeps_the_root() {
        let mut registry = Registry::default();
        registry.insert("root", "root", "https://host.invalid/live.m3u8", vec![]);
        registry.insert("root", "old", "https://host.invalid/old.ts", vec![]);
        for index in 0..MAX_RESOURCES {
            if index == MAX_RESOURCES / 2 {
                // A resource the player keeps using is not the one evicted.
                assert!(registry.touch("old").is_some());
            }
            let key = format!("segment-{index}");
            registry.insert("root", &key, "https://host.invalid/s.ts", vec![]);
        }
        assert!(registry.entries.len() <= MAX_RESOURCES);
        assert!(registry.touch("root").is_some());
        assert!(registry.touch("old").is_some());
        assert!(registry.touch("segment-0").is_none());
        assert!(
            registry
                .touch(&format!("segment-{}", MAX_RESOURCES - 1))
                .is_some()
        );
        // Re-announcing a known address refreshes it instead of adding an entry.
        let before = registry.entries.len();
        registry.insert("root", "old", "https://host.invalid/old.ts", vec![]);
        assert_eq!(registry.entries.len(), before);
    }

    #[test]
    fn only_template_addresses_carry_placeholders() {
        assert_eq!(
            placeholders("https://h.invalid/$RepresentationID$/$Number%05d$.m4s").unwrap(),
            vec!["$RepresentationID$", "$Number%05d$"]
        );
        assert!(placeholders("https://h.invalid/a$b.ts").is_err());
        let namespace = Uuid::new_v4();
        assert_ne!(
            token(&namespace, "https://h.invalid/a$b$.ts", true),
            token(&namespace, "https://h.invalid/a$b$.ts", false)
        );
    }
}
