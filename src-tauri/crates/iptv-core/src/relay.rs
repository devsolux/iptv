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
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, watch};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
struct Resource {
    target: String,
    placeholders: Vec<String>,
    seen: Instant,
}
struct Session {
    id: String,
    namespace: Uuid,
    network: MediaNetwork,
    resources: Mutex<HashMap<String, Resource>>,
    cancel: watch::Sender<bool>,
    requests: Arc<Semaphore>,
    root: String,
}
impl Session {
    fn register(&self, base: &str, target: &str) -> Result<String> {
        self.network
            .check(&Url::parse(target).map_err(|_| Error::Policy)?)?;
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
        let token = Uuid::new_v5(&self.namespace, target.as_bytes()).to_string();
        let mut resources = self.resources.lock().map_err(|_| Error::Storage)?;
        resources.retain(|key, r| key == &self.root || r.seen.elapsed() < Duration::from_secs(300));
        if !resources.contains_key(&token) && resources.len() >= 8192 {
            return Err(Error::Limit);
        }
        resources.insert(
            token.clone(),
            Resource {
                target: target.to_owned(),
                placeholders: placeholders.clone(),
                seen: Instant::now(),
            },
        );
        let suffix = if placeholders.is_empty() {
            String::new()
        } else {
            format!("/{}", placeholders.join("/"))
        };
        Ok(format!("{base}/media/{}/{token}{suffix}", self.id))
    }
    fn resolve(&self, token: &str, values: &str) -> Result<Url> {
        let mut resources = self.resources.lock().map_err(|_| Error::Storage)?;
        let resource = resources.get_mut(token).ok_or(Error::NotFound)?;
        resource.seen = Instant::now();
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
}
struct Inner {
    base: String,
    current: Mutex<Option<Arc<Session>>>,
    epoch: AtomicU64,
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
        let root = Url::parse(&secret.url).map_err(|_| Error::Unsupported)?;
        if !matches!(root.scheme(), "http" | "https") {
            return Err(Error::Unsupported);
        }
        let network = MediaNetwork::new(NetworkPolicy::new(&secret.url, config)?, secret.headers);
        // A bounded probe determines the engine without assuming a filename extension.
        let response = network
            .get(root.clone(), Some("bytes=0-4095"), false)
            .await?;
        let response = if response.status().as_u16() == 416 {
            drop(response);
            network.get(root.clone(), None, false).await?
        } else {
            response
        };
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let mut bytes = response.bytes_stream();
        let mut prefix = Vec::with_capacity(4096);
        while prefix.len() < 4096 {
            match bytes.next().await {
                Some(Ok(chunk)) => {
                    let n = chunk.len().min(4096 - prefix.len());
                    prefix.extend_from_slice(&chunk[..n]);
                }
                Some(Err(_)) => return Err(Error::Network),
                None => break,
            }
        }
        drop(bytes);
        let text = String::from_utf8_lossy(&prefix);
        let kind = if text
            .trim_start_matches('\u{feff}')
            .trim_start()
            .starts_with("#EXTM3U")
            || content_type.contains("mpegurl")
            || root.path().to_ascii_lowercase().ends_with(".m3u8")
        {
            "hls"
        } else if text.contains("<MPD")
            || content_type.contains("dash+xml")
            || root.path().to_ascii_lowercase().ends_with(".mpd")
        {
            "dash"
        } else {
            "progressive"
        };
        let namespace = Uuid::new_v4();
        let root_token = Uuid::new_v5(&namespace, root.as_str().as_bytes()).to_string();
        let (cancel, _) = watch::channel(false);
        let session = Arc::new(Session {
            id: namespace.to_string(),
            namespace,
            network,
            resources: Mutex::new(HashMap::new()),
            cancel,
            requests: Arc::new(Semaphore::new(6)),
            root: root_token,
        });
        let url = session.register(&self.inner.base, root.as_str())?;
        let mut current = self.inner.current.lock().map_err(|_| Error::Storage)?;
        if self.inner.epoch.load(Ordering::SeqCst) != ticket {
            return Err(Error::Cancelled);
        }
        if let Some(old) = current.replace(session.clone()) {
            old.cancel.send_replace(true);
        }
        Ok(Prepared {
            id: session.id.clone(),
            url,
            kind: kind.to_owned(),
        })
    }
    pub fn close(&self, id: Option<&str>) {
        if let Ok(mut guard) = self.inner.current.lock() {
            if id.is_none() || guard.as_ref().is_some_and(|s| Some(s.id.as_str()) == id) {
                self.inner.epoch.fetch_add(1, Ordering::SeqCst);
                if let Some(session) = guard.take() {
                    session.cancel.send_replace(true);
                }
            }
        }
    }
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
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::Policy)?;
    if host
        != state
            .inner
            .base
            .strip_prefix("http://")
            .ok_or(Error::Policy)?
    {
        return Err(Error::Policy);
    }
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
    dispatch(state, sid, rid, String::new(), headers, method)
        .await
        .unwrap_or_else(public_http_error)
}
async fn template(
    State(state): State<HttpState>,
    Path((sid, rid, values)): Path<(String, String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    dispatch(state, sid, rid, values, headers, method)
        .await
        .unwrap_or_else(public_http_error)
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
    let upstream = tokio::select! {
        _=cancellation.changed()=>return Err(Error::Cancelled),
        value=session.network.get(url,range,method==Method::HEAD)=>value?,
    };
    let status = StatusCode::from_u16(upstream.status().as_u16()).map_err(|_| Error::Network)?;
    let final_url = upstream.url().clone();
    let upstream_headers = upstream.headers().clone();
    let content_type = upstream_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream");
    let mut incoming = upstream.bytes_stream();
    let mut prefix = Vec::with_capacity(512);
    let mut remainder = None;
    if method != Method::HEAD && status.is_success() {
        while prefix.len() < 512 {
            let next = tokio::select! {_=cancellation.changed()=>return Err(Error::Cancelled),value=incoming.next()=>value};
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
    let text = String::from_utf8_lossy(&prefix);
    let hls = content_type.contains("mpegurl")
        || final_url.path().to_ascii_lowercase().ends_with(".m3u8")
        || text
            .trim_start_matches('\u{feff}')
            .trim_start()
            .starts_with("#EXTM3U");
    let dash = content_type.contains("dash+xml")
        || final_url.path().to_ascii_lowercase().ends_with(".mpd")
        || text.contains("<MPD");
    let buffered =
        std::iter::once(Ok::<Bytes, reqwest::Error>(Bytes::from(prefix))).chain(remainder.map(Ok));
    let mut stream = futures_util::stream::iter(buffered).chain(incoming);
    let mut response = if method != Method::HEAD
        && (hls || dash)
        && status != StatusCode::RANGE_NOT_SATISFIABLE
    {
        let mut data = Vec::with_capacity(16 * 1024);
        loop {
            let chunk = tokio::select! {_=cancellation.changed()=>return Err(Error::Cancelled),value=stream.next()=>value};
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
        let register = |url: &str| session.register(&state.inner.base, url);
        let rewritten = if hls {
            manifest::hls(&input, &final_url, register)?
        } else {
            manifest::dash(&input, &final_url, register)?
        };
        drop(permit);
        let mut response = (StatusCode::OK, rewritten).into_response();
        response.headers_mut().insert(
            "content-type",
            HeaderValue::from_static(if hls {
                "application/vnd.apple.mpegurl"
            } else {
                "application/dash+xml"
            }),
        );
        response
    } else {
        let stream = futures_util::stream::unfold(
            (Box::pin(stream), cancellation, permit),
            |(mut stream, mut cancel, permit)| async move {
                if *cancel.borrow() {
                    return None;
                }
                let next =
                    tokio::select! {_=cancel.changed()=>return None,value=stream.next()=>value};
                next.map(|value| {
                    (
                        value.map_err(|_| std::io::Error::other("Media transport interrupted")),
                        (stream, cancel, permit),
                    )
                })
            },
        );
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = status;
        for name in [
            "content-type",
            "content-length",
            "content-range",
            "accept-ranges",
        ] {
            if let Some(value) = upstream_headers.get(name) {
                response.headers_mut().insert(name, value.clone());
            }
        }
        response
    };
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        "access-control-expose-headers",
        HeaderValue::from_static("content-length, content-range, accept-ranges"),
    );
    response
        .headers_mut()
        .insert("vary", HeaderValue::from_static("Origin"));
    if let Some(value) = origin(&headers)? {
        response
            .headers_mut()
            .insert("access-control-allow-origin", value);
    }
    Ok(response)
}
fn public_http_error(error: Error) -> Response {
    let status = match error {
        Error::NotFound | Error::Cancelled => StatusCode::GONE,
        Error::Policy => StatusCode::FORBIDDEN,
        Error::Limit => StatusCode::PAYLOAD_TOO_LARGE,
        Error::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        Error::Busy => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, error.to_string()).into_response()
}
