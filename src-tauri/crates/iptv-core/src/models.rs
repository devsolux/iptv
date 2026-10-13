use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceInput {
    pub name: String,
    pub playlist_url: String,
    #[serde(default)]
    pub epg_urls: Vec<String>,
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default)]
    pub allow_lan: bool,
    #[serde(default)]
    pub cdn_hosts: Vec<String>,
    /// Explicit consent to unverified TLS certificates for this source's requests.
    #[serde(default)]
    pub accept_invalid_certs: bool,
    #[serde(default)]
    pub country_code: String,
    /// Present only for iptv-org sources; own M3U playlists leave it absent.
    #[serde(default)]
    pub iptv_org: Option<crate::iptv_org::Config>,
}
impl SourceInput {
    /// Aggregator sources list streams on arbitrary public hosts, so the exact host
    /// allowlist does not apply; every other network protection still does.
    pub fn is_aggregator(&self) -> bool {
        self.iptv_org.is_some()
    }
    pub fn kind(&self) -> &'static str {
        if self.iptv_org.is_some() {
            "iptv-org"
        } else {
            "m3u"
        }
    }
    pub fn refresh_interval_seconds(&self) -> i64 {
        self.iptv_org
            .as_ref()
            .map_or(6, |config| i64::from(config.cache.metadata_refresh_hours))
            * 3600
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub channels: i64,
    pub updated_at: Option<i64>,
    pub has_epg: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSecret {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
pub struct ParsedChannel {
    pub name: String,
    pub tvg_id: String,
    pub tvg_name: String,
    pub groups: Vec<String>,
    pub country: String,
    pub logo: String,
    pub shift_seconds: i64,
    pub catchup: BTreeMap<String, String>,
    pub stream: StreamSecret,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelQuery {
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub search: String,
    pub source_id: Option<String>,
    pub group: Option<String>,
    pub country: Option<String>,
    #[serde(default)]
    pub favorites_only: bool,
    pub revision: Option<i64>,
}
fn default_limit() -> u32 {
    80
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub source_id: String,
    pub country: String,
    pub favorite: bool,
    pub variants: i64,
    pub has_epg: bool,
    #[serde(skip)]
    pub has_logo: bool,
    /// Loopback relay address of the channel artwork, never the upstream image URL.
    pub logo_url: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPage {
    pub items: Vec<Channel>,
    pub total: i64,
    pub revision: i64,
    pub offset: u32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Programme {
    pub channel_id: String,
    pub start: i64,
    pub end: i64,
    pub title: String,
    pub description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideQuery {
    pub channel_ids: Vec<String>,
    pub start: i64,
    pub end: i64,
    pub revision: i64,
}
#[derive(Serialize)]
pub struct GuideWindow {
    pub items: Vec<Programme>,
    pub revision: i64,
}

#[derive(Clone, Default)]
pub struct EpgChannel {
    pub id: String,
    pub names: Vec<String>,
}
#[derive(Clone, Default)]
pub struct EpgProgramme {
    pub channel: String,
    pub start: i64,
    pub end: i64,
    pub title: String,
    pub description: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub source_id: String,
    pub state: String,
    pub phase: String,
    pub processed: u64,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    pub session_id: String,
    pub channel_id: String,
    pub title: String,
    pub url: String,
    pub kind: String,
    pub native: bool,
    pub variants: i64,
}

pub fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Serialize)]
pub struct Country {
    pub code: String,
    pub count: i64,
}
#[derive(Serialize)]
pub struct ChannelLocation {
    pub index: Option<i64>,
    pub revision: i64,
}
