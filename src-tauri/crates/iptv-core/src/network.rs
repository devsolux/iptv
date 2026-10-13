use crate::{
    error::{Error, Result},
    models::SourceInput,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, LOCATION},
    redirect::Policy,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use url::Url;

#[derive(Clone)]
pub struct NetworkPolicy {
    pub allow_http: bool,
    pub allow_lan: bool,
    hosts: HashSet<String>,
}
impl NetworkPolicy {
    pub fn new(initial: &str, source: &SourceInput) -> Result<Self> {
        let url = Url::parse(initial).map_err(|_| Error::Invalid("A source URL is invalid."))?;
        let host = url.host_str().ok_or(Error::Policy)?.to_ascii_lowercase();
        let mut hosts = HashSet::from([host]);
        if source.cdn_hosts.len() > 32 {
            return Err(Error::Limit);
        }
        for host in &source.cdn_hosts {
            let parsed = Url::parse(&format!("https://{host}/")).map_err(|_| {
                Error::Invalid("CDN hosts must be exact hostnames, without paths or ports.")
            })?;
            if parsed.host_str() != Some(host.to_ascii_lowercase().as_str())
                || parsed.port().is_some()
                || parsed.path() != "/"
                || !parsed.username().is_empty()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(Error::Invalid(
                    "CDN hosts must be exact hostnames, without paths or ports.",
                ));
            }
            hosts.insert(host.to_ascii_lowercase());
        }
        let policy = Self {
            allow_http: source.allow_http,
            allow_lan: source.allow_lan,
            hosts,
        };
        policy.check_url(&url)?;
        Ok(policy)
    }
    pub fn check_url(&self, url: &Url) -> Result<()> {
        if url.as_str().len() > 16_384 || url.fragment().is_some() {
            return Err(Error::Policy);
        }
        if url.scheme() != "https" && !(self.allow_http && url.scheme() == "http") {
            return Err(Error::Policy);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Error::Invalid(
                "Use token-based URLs rather than URL user-info credentials.",
            ));
        }
        let host = url.host_str().ok_or(Error::Policy)?.to_ascii_lowercase();
        if !self.hosts.contains(&host) {
            return Err(Error::Policy);
        }
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
            self.check_ip(ip)?;
        }
        Ok(())
    }
    pub fn check_ip(&self, address: IpAddr) -> Result<()> {
        let forbidden = match address {
            IpAddr::V4(ip) => {
                let [a, b, c, _] = ip.octets();
                ip.is_loopback() || ip.is_unspecified() || ip.is_link_local() || ip.is_multicast()
                    || ip.is_broadcast() || a == 0 || a >= 240
                    || (a == 100 && (64..=127).contains(&b)) // carrier NAT / infrastructure
                    || (a == 192 && b == 0 && c == 0)
                    || (!self.allow_lan && ip.is_private())
            }
            IpAddr::V6(ip) => {
                if let Some(v4) = ip.to_ipv4_mapped() {
                    return self.check_ip(IpAddr::V4(v4));
                }
                let segments = ip.segments();
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || (segments[0] & 0xffc0) == 0xfe80
                    || (!self.allow_lan && (segments[0] & 0xfe00) == 0xfc00)
                    || !((segments[0] & 0xe000) == 0x2000
                        || (self.allow_lan && (segments[0] & 0xfe00) == 0xfc00))
                    || segments[0] == 0x2002
                    || (segments[0] == 0x2001 && segments[1] == 0)
                    || (segments[0] == 0x0064 && segments[1] == 0xff9b)
            }
        };
        if forbidden {
            Err(Error::Policy)
        } else {
            Ok(())
        }
    }
    fn check_addresses(&self, addresses: Vec<SocketAddr>) -> Result<Vec<SocketAddr>> {
        if addresses.is_empty() || addresses.len() > 64 {
            return Err(Error::Network);
        }
        // Reject a mixed public/private answer rather than picking the convenient address.
        for address in &addresses {
            self.check_ip(address.ip())?;
        }
        Ok(addresses)
    }
    fn resolve_blocking(&self, url: &Url) -> Result<Vec<SocketAddr>> {
        self.check_url(url)?;
        let host = url
            .host_str()
            .ok_or(Error::Policy)?
            .trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or(Error::Policy)?;
        self.check_addresses(
            (host, port)
                .to_socket_addrs()
                .map_err(|_| Error::Network)?
                .collect(),
        )
    }
    async fn resolve(&self, url: &Url) -> Result<Vec<SocketAddr>> {
        self.check_url(url)?;
        let host = url
            .host_str()
            .ok_or(Error::Policy)?
            .trim_matches(['[', ']']);
        let port = url.port_or_known_default().ok_or(Error::Policy)?;
        let addresses = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| Error::Network)?
        .map_err(|_| Error::Network)?
        .collect();
        self.check_addresses(addresses)
    }
    pub fn get_blocking(
        &self,
        initial: &str,
        headers: &BTreeMap<String, String>,
    ) -> Result<reqwest::blocking::Response> {
        let mut url = Url::parse(initial).map_err(|_| Error::Policy)?;
        for _ in 0..=5 {
            let addresses = self.resolve_blocking(&url)?;
            let host = url.host_str().ok_or(Error::Policy)?;
            let client = reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(180))
                .resolve_to_addrs(host, &addresses)
                .build()
                .map_err(|_| Error::Network)?;
            let response = client
                .get(url.clone())
                .headers(provider_headers(headers)?)
                .header("accept-encoding", "identity")
                .send()
                .map_err(|_| Error::Network)?;
            if response.status().is_redirection() {
                url = redirect(&url, response.headers())?;
                continue;
            }
            if !response.status().is_success() {
                return Err(Error::Network);
            }
            return Ok(response);
        }
        Err(Error::Policy)
    }
}

pub fn validate_source(source: &SourceInput) -> Result<()> {
    if source.name.trim().is_empty() || source.name.len() > 128 || source.epg_urls.len() > 4 {
        return Err(Error::Invalid(
            "Use a source name and at most four EPG URLs.",
        ));
    }
    if !source.country_code.is_empty()
        && (source.country_code.len() != 2
            || !source.country_code.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return Err(Error::Invalid(
            "The default country must be a two-letter country code.",
        ));
    }
    NetworkPolicy::new(&source.playlist_url, source)?;
    for url in &source.epg_urls {
        NetworkPolicy::new(url, source)?;
    }
    Ok(())
}
fn provider_headers(input: &BTreeMap<String, String>) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    for (name, value) in input {
        if !matches!(name.as_str(), "referer" | "user-agent") {
            return Err(Error::Policy);
        }
        if value.len() > 4096 {
            return Err(Error::Limit);
        }
        headers.insert(
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::Policy)?,
            HeaderValue::from_str(value).map_err(|_| Error::Policy)?,
        );
    }
    Ok(headers)
}
fn redirect(url: &Url, headers: &HeaderMap) -> Result<Url> {
    let location = headers
        .get(LOCATION)
        .ok_or(Error::Network)?
        .to_str()
        .map_err(|_| Error::Network)?;
    url.join(location).map_err(|_| Error::Policy)
}

type ClientCache = HashMap<String, (std::time::Instant, reqwest::Client)>;
#[derive(Clone)]
pub struct MediaNetwork {
    policy: NetworkPolicy,
    headers: BTreeMap<String, String>,
    clients: Arc<Mutex<ClientCache>>,
}
impl MediaNetwork {
    pub fn new(policy: NetworkPolicy, headers: BTreeMap<String, String>) -> Self {
        Self {
            policy,
            headers,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn check(&self, url: &Url) -> Result<()> {
        self.policy.check_url(url)
    }
    async fn client(&self, url: &Url) -> Result<reqwest::Client> {
        self.policy.check_url(url)?;
        let key = url.origin().ascii_serialization();
        let mut cache = self.clients.lock().await;
        if let Some((created, client)) = cache.get(&key) {
            if created.elapsed() < Duration::from_secs(60) {
                return Ok(client.clone());
            }
        }
        let addresses = self.policy.resolve(url).await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(2)
            .resolve_to_addrs(url.host_str().ok_or(Error::Policy)?, &addresses)
            .build()
            .map_err(|_| Error::Network)?;
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(key, (std::time::Instant::now(), client.clone()));
        Ok(client)
    }
    pub async fn get(
        &self,
        initial: Url,
        range: Option<&str>,
        head: bool,
    ) -> Result<reqwest::Response> {
        let mut url = initial;
        for _ in 0..=5 {
            let client = self.client(&url).await?;
            let method = if head {
                reqwest::Method::HEAD
            } else {
                reqwest::Method::GET
            };
            let mut request = client
                .request(method, url.clone())
                .headers(provider_headers(&self.headers)?)
                .header("accept-encoding", "identity");
            if let Some(range) = range {
                if range.len() > 128 || !range.starts_with("bytes=") || range.contains(',') {
                    return Err(Error::Policy);
                }
                request = request.header("range", range);
            }
            let response = request.send().await.map_err(|_| Error::Network)?;
            if response.status().is_redirection() {
                url = redirect(&url, response.headers())?;
                continue;
            }
            if !(response.status().is_success() || response.status().as_u16() == 416) {
                return Err(Error::Network);
            }
            return Ok(response);
        }
        Err(Error::Policy)
    }
}
