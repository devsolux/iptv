//! iptv-org integration: official playlist URL construction, bounded JSON dataset ingestion,
//! one deterministic selection policy and the extended-M3U serializer whose output feeds the
//! ordinary playlist importer. Sources without an iptv-org configuration never use this module.

use crate::error::{Error, Result};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeOwned, SeqAccess, Visitor},
};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    io::Read,
    marker::PhantomData,
};
use url::Url;

pub const DEFAULT_API_BASE: &str = "https://iptv-org.github.io/api";
pub const DEFAULT_PLAYLIST_BASE: &str = "https://iptv-org.github.io/iptv";
pub const MAX_PLAYLISTS: usize = 32;
const MAX_CODES: usize = 64;
const MAX_ITEMS: usize = 400_000;
const MAX_VARIANTS: usize = 32;
const MAX_REFRESH_HOURS: u32 = 168;

// ---------------------------------------------------------------------------------------------
// Application-owned configuration contract (mirrors the documented JSON contract; the playlist
// base lives in `SourceInput::playlist_url` and the XMLTV URL in `SourceInput::epg_urls`).
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    /// Fetch official prebuilt playlists and hand them to the ordinary M3U importer.
    Playlist,
    /// Join the JSON datasets locally and apply the country/category/language policy.
    Catalogue,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CountryMode {
    Origin,
    #[default]
    CoverageStrict,
    CoverageWithRegions,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupBy {
    Country,
    Category,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaylistKind {
    Master,
    IndexCountry,
    IndexCategory,
    IndexLanguage,
    Country,
    Category,
    Language,
    Region,
    Subdivision,
    City,
    Source,
    Raw,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfficialPlaylist {
    pub kind: PlaylistKind,
    #[serde(default)]
    pub code: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Geography {
    #[serde(default)]
    pub countries: Vec<String>,
    #[serde(default)]
    pub mode: CountryMode,
    #[serde(default)]
    pub include_international: bool,
    #[serde(default = "default_aliases")]
    pub country_aliases: BTreeMap<String, String>,
}
impl Default for Geography {
    fn default() -> Self {
        Self {
            countries: vec![],
            mode: CountryMode::default(),
            include_international: false,
            country_aliases: default_aliases(),
        }
    }
}
fn default_aliases() -> BTreeMap<String, String> {
    BTreeMap::from([("GB".to_owned(), "UK".to_owned())])
}
fn yes() -> bool {
    true
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default = "yes")]
    pub exclude_nsfw: bool,
    #[serde(default = "yes")]
    pub exclude_blocked: bool,
    #[serde(default = "yes")]
    pub exclude_closed: bool,
    #[serde(default = "yes")]
    pub exclude_geo_blocked_label: bool,
    #[serde(default = "yes")]
    pub exclude_not247_label: bool,
}
impl Default for Filters {
    fn default() -> Self {
        Self {
            categories: vec![],
            languages: vec![],
            exclude_nsfw: true,
            exclude_blocked: true,
            exclude_closed: true,
            exclude_geo_blocked_label: true,
            exclude_not247_label: true,
        }
    }
}
impl Filters {
    /// Official playlists carry stream labels as name markers such as `[Not 24/7]`.
    pub fn excludes_official_name(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        (self.exclude_geo_blocked_label && lower.contains("[geo-blocked]"))
            || (self.exclude_not247_label && lower.contains("[not 24/7]"))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaylistOptions {
    #[serde(default = "default_group_by")]
    pub group_by: Vec<GroupBy>,
    #[serde(default = "yes")]
    pub include_every_alternative_stream: bool,
    #[serde(default = "yes")]
    pub emit_vlc_http_options: bool,
}
impl Default for PlaylistOptions {
    fn default() -> Self {
        Self {
            group_by: default_group_by(),
            include_every_alternative_stream: true,
            emit_vlc_http_options: true,
        }
    }
}
fn default_group_by() -> Vec<GroupBy> {
    vec![GroupBy::Country, GroupBy::Category]
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cache {
    #[serde(default = "default_refresh")]
    pub metadata_refresh_hours: u32,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            metadata_refresh_hours: default_refresh(),
        }
    }
}
fn default_refresh() -> u32 {
    24
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    /// Empty selects the official API base.
    #[serde(default)]
    pub api_base: String,
    pub method: Method,
    #[serde(default)]
    pub playlists: Vec<OfficialPlaylist>,
    #[serde(default)]
    pub geography: Geography,
    #[serde(default)]
    pub filters: Filters,
    #[serde(default)]
    pub playlist: PlaylistOptions,
    #[serde(default)]
    pub cache: Cache,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        base_url(&self.api_base, DEFAULT_API_BASE)?;
        if self.cache.metadata_refresh_hours == 0
            || self.cache.metadata_refresh_hours > MAX_REFRESH_HOURS
        {
            return Err(Error::Invalid(
                "Refresh iptv-org metadata between every hour and every week.",
            ));
        }
        if self.geography.countries.len() > MAX_CODES
            || self.geography.country_aliases.len() > MAX_CODES
            || self.filters.categories.len() > MAX_CODES
            || self.filters.languages.len() > MAX_CODES
            || self.playlists.len() > MAX_PLAYLISTS
            || self.playlist.group_by.len() > 2
        {
            return Err(Error::Limit);
        }
        for code in &self.geography.countries {
            if !is_code(code, 2, 2, b"") {
                return Err(Error::Invalid(
                    "Country selections must be two-letter country codes.",
                ));
            }
        }
        for (from, to) in &self.geography.country_aliases {
            if !is_code(from, 2, 2, b"") || !is_code(to, 2, 2, b"") {
                return Err(Error::Invalid(
                    "Country aliases map one two-letter code to another.",
                ));
            }
        }
        for id in &self.filters.categories {
            if !is_category(id) {
                return Err(Error::Invalid(
                    "Category IDs are lowercase slugs such as news.",
                ));
            }
        }
        for code in &self.filters.languages {
            if !is_code(code, 3, 3, b"") {
                return Err(Error::Invalid(
                    "Languages use three-letter ISO 639-3 codes such as deu.",
                ));
            }
        }
        if self.playlist.group_by.len() == 2
            && self.playlist.group_by[0] == self.playlist.group_by[1]
        {
            return Err(Error::Invalid("Group the playlist by distinct dimensions."));
        }
        match self.method {
            Method::Playlist => {
                if self.playlists.is_empty() {
                    return Err(Error::Invalid(
                        "Select at least one official iptv-org playlist.",
                    ));
                }
                for pick in &self.playlists {
                    official_path(pick, &self.geography.country_aliases)?;
                }
            }
            Method::Catalogue => {
                if self.geography.countries.is_empty() {
                    return Err(Error::Invalid(
                        "Select at least one country for an iptv-org catalogue selection.",
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn api_url(&self) -> Result<Url> {
        base_url(&self.api_base, DEFAULT_API_BASE)
    }
}

/// The playlist base of an iptv-org source; an empty `playlist_url` selects the official base.
pub fn playlist_base(source: &crate::models::SourceInput) -> Result<Url> {
    base_url(&source.playlist_url, DEFAULT_PLAYLIST_BASE)
}
/// A public HTTPS base without credentials, query or fragment; empty selects the default.
pub fn base_url(value: &str, default: &str) -> Result<Url> {
    let value = value.trim();
    let url = Url::parse(if value.is_empty() { default } else { value })
        .map_err(|_| Error::Invalid("The iptv-org base URL is invalid."))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str().len() > 512
    {
        return Err(Error::Invalid(
            "iptv-org base URLs must be plain HTTPS origins, optionally with a path.",
        ));
    }
    Ok(url)
}
fn is_code(value: &str, min: usize, max: usize, extra: &[u8]) -> bool {
    value.len() >= min
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}
fn is_category(value: &str) -> bool {
    is_code(value, 1, 64, b"-")
        && value.as_bytes()[0].is_ascii_lowercase()
        && !value.bytes().any(|b| b.is_ascii_uppercase())
}
fn canonical(aliases: &BTreeMap<String, String>, code: &str) -> String {
    let code = code.trim().to_ascii_uppercase();
    aliases
        .iter()
        .find(|(from, _)| from.eq_ignore_ascii_case(&code))
        .map_or(code, |(_, to)| to.to_ascii_uppercase())
}

/// Relative path of an official playlist, validated per kind. Country codes are canonicalized
/// (`GB` → `UK`); subdivision and city identifiers are never rewritten.
pub fn official_path(
    pick: &OfficialPlaylist,
    aliases: &BTreeMap<String, String>,
) -> Result<String> {
    let code = pick.code.trim();
    let fixed = |path: &str| {
        if code.is_empty() {
            Ok(path.to_owned())
        } else {
            Err(Error::Invalid("Master playlists do not take a code."))
        }
    };
    match pick.kind {
        PlaylistKind::Master => fixed("index.m3u"),
        PlaylistKind::IndexCountry => fixed("index.country.m3u"),
        PlaylistKind::IndexCategory => fixed("index.category.m3u"),
        PlaylistKind::IndexLanguage => fixed("index.language.m3u"),
        PlaylistKind::Country => {
            let country = canonical(aliases, code);
            if is_code(&country, 2, 2, b"") || country == "INT" || country == "UNDEFINED" {
                Ok(format!("countries/{}.m3u", country.to_ascii_lowercase()))
            } else {
                Err(Error::Invalid(
                    "Use a two-letter country code, INT or UNDEFINED for country playlists.",
                ))
            }
        }
        PlaylistKind::Category => {
            let id = code.to_ascii_lowercase();
            if is_category(&id) {
                Ok(format!("categories/{id}.m3u"))
            } else {
                Err(Error::Invalid(
                    "Category IDs are lowercase slugs such as news.",
                ))
            }
        }
        PlaylistKind::Language => {
            let id = code.to_ascii_lowercase();
            if is_code(&id, 3, 3, b"") || id == "undefined" {
                Ok(format!("languages/{id}.m3u"))
            } else {
                Err(Error::Invalid(
                    "Languages use three-letter ISO 639-3 codes such as deu.",
                ))
            }
        }
        PlaylistKind::Region => {
            if is_code(code, 2, 8, b"") {
                Ok(format!("regions/{}.m3u", code.to_ascii_lowercase()))
            } else {
                Err(Error::Invalid(
                    "Region codes are short identifiers such as EUR.",
                ))
            }
        }
        PlaylistKind::Subdivision => {
            let (country, part) = code.split_once('-').unwrap_or_default();
            if is_code(country, 2, 2, b"") && is_code(part, 1, 3, b"") {
                Ok(format!("subdivisions/{}.m3u", code.to_ascii_lowercase()))
            } else {
                Err(Error::Invalid(
                    "Subdivisions use ISO 3166-2 codes such as DE-NW.",
                ))
            }
        }
        PlaylistKind::City => {
            if is_code(code, 3, 12, b"") {
                Ok(format!("cities/{}.m3u", code.to_ascii_lowercase()))
            } else {
                Err(Error::Invalid("City codes are identifiers such as USNYC."))
            }
        }
        PlaylistKind::Source | PlaylistKind::Raw => {
            let file = code.to_ascii_lowercase();
            if is_code(&file, 1, 64, b"_.-") && !file.contains("..") {
                let directory = if pick.kind == PlaylistKind::Source {
                    "sources"
                } else {
                    "raw"
                };
                Ok(format!("{directory}/{file}.m3u"))
            } else {
                Err(Error::Invalid("Source playlists are named like us_pluto."))
            }
        }
    }
}
/// Absolute official playlist URL plus the country the entries belong to, when the playlist
/// itself defines one (official playlists do not carry `tvg-country`).
pub fn official_url(
    base: &Url,
    pick: &OfficialPlaylist,
    aliases: &BTreeMap<String, String>,
) -> Result<(String, Option<String>)> {
    let path = official_path(pick, aliases)?;
    let url = format!("{}/{path}", base.as_str().trim_end_matches('/'));
    Url::parse(&url).map_err(|_| Error::Invalid("The iptv-org base URL is invalid."))?;
    let country = match pick.kind {
        PlaylistKind::Country => {
            let country = canonical(aliases, &pick.code);
            (country.len() == 2).then_some(country)
        }
        _ => None,
    };
    Ok((url, country))
}

// ---------------------------------------------------------------------------------------------
// Published dataset records. Only the fields the policy needs are retained; unknown fields are
// skipped, nulls become empty values, and missing required identifiers reject the snapshot.
// ---------------------------------------------------------------------------------------------

fn nullable<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub country: String,
    #[serde(default, deserialize_with = "nullable")]
    pub categories: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub is_nsfw: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub closed: String,
}
#[derive(Deserialize)]
pub struct Feed {
    pub channel: String,
    pub id: String,
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub is_main: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub broadcast_area: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub languages: Vec<String>,
}
#[derive(Deserialize)]
pub struct Stream {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub feed: Option<String>,
    pub url: String,
    #[serde(default)]
    pub referrer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub labels: Vec<String>,
}
#[derive(Deserialize)]
pub struct Logo {
    pub channel: String,
    #[serde(default)]
    pub feed: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub in_use: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub width: u64,
    #[serde(default, deserialize_with = "nullable")]
    pub height: u64,
    pub url: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Category {
    pub id: String,
    pub name: String,
}
#[derive(Deserialize)]
pub struct Country {
    pub code: String,
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub languages: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub flag: String,
}
#[derive(Deserialize)]
pub struct Region {
    pub code: String,
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub countries: Vec<String>,
}
#[derive(Deserialize)]
pub struct Subdivision {
    pub country: String,
    pub code: String,
}
#[derive(Deserialize)]
pub struct City {
    pub country: String,
    pub code: String,
}
#[derive(Deserialize)]
pub struct Blocked {
    pub channel: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Language {
    pub code: String,
    pub name: String,
}

#[derive(Default)]
pub struct Dataset {
    pub channels: Vec<Channel>,
    pub feeds: Vec<Feed>,
    pub streams: Vec<Stream>,
    pub logos: Vec<Logo>,
    pub categories: Vec<Category>,
    pub countries: Vec<Country>,
    pub regions: Vec<Region>,
    pub subdivisions: Vec<Subdivision>,
    pub cities: Vec<City>,
    pub blocklist: Vec<Blocked>,
}

struct ArrayVisitor<'a, T, F> {
    each: F,
    limit: usize,
    failure: &'a mut Option<Error>,
    item: PhantomData<T>,
}
impl<'de, T: Deserialize<'de>, F: FnMut(T) -> Result<()>> Visitor<'de> for ArrayVisitor<'_, T, F> {
    type Value = usize;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON array")
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> std::result::Result<usize, A::Error> {
        let mut count = 0usize;
        while let Some(item) = seq.next_element::<T>()? {
            count += 1;
            if count > self.limit {
                *self.failure = Some(Error::Limit);
                return Err(serde::de::Error::custom("array limit exceeded"));
            }
            if let Err(error) = (self.each)(item) {
                *self.failure = Some(error);
                return Err(serde::de::Error::custom("item rejected"));
            }
        }
        Ok(count)
    }
}
/// Streams one top-level JSON array item by item, so only retained records occupy memory.
pub fn read_array<T: DeserializeOwned>(
    reader: impl Read,
    limit: usize,
    each: impl FnMut(T) -> Result<()>,
) -> Result<usize> {
    let mut failure = None;
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let count = (&mut deserializer).deserialize_seq(ArrayVisitor {
        each,
        limit,
        failure: &mut failure,
        item: PhantomData,
    });
    if let Some(error) = failure {
        return Err(error);
    }
    let malformed = || Error::Invalid("The iptv-org data is malformed or exceeds a limit.");
    let count = count.map_err(|_| malformed())?;
    deserializer.end().map_err(|_| malformed())?;
    Ok(count)
}
fn read_all<T: DeserializeOwned, F: FnMut(&str) -> Result<Box<dyn Read>>>(
    fetch: &mut F,
    file: &str,
) -> Result<Vec<T>> {
    let reader = fetch(file)?;
    let mut items = Vec::new();
    read_array(reader, MAX_ITEMS, |item| {
        items.push(item);
        Ok(())
    })?;
    Ok(items)
}
/// Loads the ten datasets the selection policy needs as one coherent snapshot.
pub fn load<F: FnMut(&str) -> Result<Box<dyn Read>>>(mut fetch: F) -> Result<Dataset> {
    let mut data = Dataset {
        channels: read_all(&mut fetch, "channels.json")?,
        feeds: read_all(&mut fetch, "feeds.json")?,
        streams: read_all(&mut fetch, "streams.json")?,
        logos: read_all(&mut fetch, "logos.json")?,
        categories: read_all(&mut fetch, "categories.json")?,
        countries: read_all(&mut fetch, "countries.json")?,
        regions: read_all(&mut fetch, "regions.json")?,
        subdivisions: read_all(&mut fetch, "subdivisions.json")?,
        cities: read_all(&mut fetch, "cities.json")?,
        blocklist: read_all(&mut fetch, "blocklist.json")?,
    };
    // Only in-use HTTPS artwork is ever selected; drop the rest before indexing.
    data.logos
        .retain(|logo| logo.in_use && logo.url.starts_with("https://"));
    Ok(data)
}

// ---------------------------------------------------------------------------------------------
// Selection: the single policy behind the catalogue and the serialized playlist.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub tvg_id: String,
    pub tvg_name: String,
    pub logo: String,
    pub group: String,
    pub country: String,
    pub name: String,
    pub url: String,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
}

fn quality_rank(value: Option<&str>) -> u32 {
    let Some(value) = value.map(str::trim) else {
        return 0;
    };
    let digits = value.trim_end_matches(['p', 'i', 'P', 'I']);
    if (3..=4).contains(&digits.len())
        && digits.len() + 1 == value.len()
        && digits.bytes().all(|b| b.is_ascii_digit())
    {
        digits.parse().unwrap_or(0)
    } else {
        0
    }
}
struct Geo<'a> {
    config: &'a Config,
    subdivisions: HashMap<String, &'a str>,
    cities: HashMap<String, &'a str>,
    regions: HashMap<String, &'a Region>,
}
impl Geo<'_> {
    fn covers(&self, raw: &str, country: &str) -> bool {
        let aliases = &self.config.geography.country_aliases;
        let Some((kind, code)) = raw.split_once('/') else {
            return false;
        };
        let code = code.to_ascii_uppercase();
        match kind.to_ascii_lowercase().as_str() {
            "c" => canonical(aliases, &code) == country,
            "s" => self
                .subdivisions
                .get(&code)
                .is_some_and(|owner| canonical(aliases, owner) == country),
            "ct" => self
                .cities
                .get(&code)
                .is_some_and(|owner| canonical(aliases, owner) == country),
            "r" => {
                if code == "WW" || code == "INT" {
                    return self.config.geography.include_international;
                }
                self.config.geography.mode == CountryMode::CoverageWithRegions
                    && self.regions.get(&code).is_some_and(|region| {
                        region
                            .countries
                            .iter()
                            .any(|member| canonical(aliases, member) == country)
                    })
            }
            _ => false,
        }
    }
}

/// Applies the configured country mode, filters, exclusions and deduplication. `accept`
/// is the source network policy: streams it rejects are not selected at all. `today` is an
/// ISO date used for the `closed` exclusion.
pub fn select(
    data: &Dataset,
    config: &Config,
    today: &str,
    accept: impl Fn(&Url) -> bool,
) -> Result<Vec<Entry>> {
    let aliases = &config.geography.country_aliases;
    let mut requested: Vec<String> = Vec::new();
    for code in &config.geography.countries {
        let code = canonical(aliases, code);
        if !requested.contains(&code) {
            requested.push(code);
        }
    }
    let countries_by_code: HashMap<String, &Country> = data
        .countries
        .iter()
        .map(|country| (canonical(aliases, &country.code), country))
        .collect();
    if requested.is_empty()
        || requested
            .iter()
            .any(|code| !countries_by_code.contains_key(code))
    {
        return Err(Error::Invalid(
            "Unknown or missing country selection; check the iptv-org country list.",
        ));
    }
    let channels_by_id: HashMap<&str, &Channel> =
        data.channels.iter().map(|c| (c.id.as_str(), c)).collect();
    let categories_by_id: HashMap<&str, &Category> =
        data.categories.iter().map(|c| (c.id.as_str(), c)).collect();
    let geo = Geo {
        config,
        regions: data
            .regions
            .iter()
            .map(|r| (r.code.to_ascii_uppercase(), r))
            .collect(),
        subdivisions: data
            .subdivisions
            .iter()
            .map(|s| (s.code.to_ascii_uppercase(), s.country.as_str()))
            .collect(),
        cities: data
            .cities
            .iter()
            .map(|c| (c.code.to_ascii_uppercase(), c.country.as_str()))
            .collect(),
    };
    let mut feed_by_key: HashMap<(&str, &str), &Feed> = HashMap::new();
    let mut feeds_by_channel: HashMap<&str, Vec<&Feed>> = HashMap::new();
    for feed in &data.feeds {
        feed_by_key.insert((feed.channel.as_str(), feed.id.as_str()), feed);
        feeds_by_channel
            .entry(feed.channel.as_str())
            .or_default()
            .push(feed);
    }
    let mut logos_by_channel: HashMap<&str, Vec<&Logo>> = HashMap::new();
    for logo in &data.logos {
        logos_by_channel
            .entry(logo.channel.as_str())
            .or_default()
            .push(logo);
    }
    let denied: HashSet<&str> = data.blocklist.iter().map(|b| b.channel.as_str()).collect();

    let resolve_feed = |stream: &Stream| -> Option<&Feed> {
        let channel = stream.channel.as_deref()?;
        match stream.feed.as_deref() {
            Some(feed) => feed_by_key.get(&(channel, feed)).copied(),
            // A single feed is an unambiguous fallback; several feeds stay unresolved.
            None => match feeds_by_channel.get(channel) {
                Some(feeds) if feeds.len() == 1 => feeds.first().copied(),
                _ => None,
            },
        }
    };
    let choose_logo = |channel: &str, stream_feed: Option<&str>| -> String {
        logos_by_channel
            .get(channel)
            .and_then(|logos| {
                logos
                    .iter()
                    .max_by_key(|logo| {
                        let tier: u64 = match (&logo.feed, stream_feed) {
                            (Some(feed), Some(wanted)) if feed == wanted => 2_000_000_000,
                            (None, _) => 1_000_000_000,
                            _ => 0,
                        };
                        (
                            tier + logo.width.saturating_mul(logo.height),
                            Reverse(logo.url.as_str()),
                        )
                    })
                    .map(|logo| logo.url.clone())
            })
            .unwrap_or_default()
    };

    let mut order: Vec<&Stream> = data.streams.iter().collect();
    order.sort_by(|a, b| {
        a.channel
            .as_deref()
            .unwrap_or_default()
            .cmp(b.channel.as_deref().unwrap_or_default())
            .then_with(|| {
                a.feed
                    .as_deref()
                    .unwrap_or_default()
                    .cmp(b.feed.as_deref().unwrap_or_default())
            })
            .then_with(|| {
                quality_rank(b.quality.as_deref()).cmp(&quality_rank(a.quality.as_deref()))
            })
            .then_with(|| a.url.cmp(&b.url))
    });

    let filters = &config.filters;
    let mut entries = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut feed_counts: HashMap<String, usize> = HashMap::new();
    for stream in order {
        let Some(channel_id) = stream.channel.as_deref() else {
            continue;
        };
        let Some(channel) = channels_by_id.get(channel_id) else {
            continue;
        };
        let Ok(url) = Url::parse(stream.url.trim()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") || url.as_str().len() > 16_384 || !accept(&url)
        {
            continue;
        }
        let feed = resolve_feed(stream);
        if filters.exclude_nsfw && channel.is_nsfw {
            continue;
        }
        if filters.exclude_blocked && denied.contains(channel_id) {
            continue;
        }
        if filters.exclude_closed && !channel.closed.is_empty() && channel.closed.as_str() <= today
        {
            continue;
        }
        if filters.exclude_geo_blocked_label && stream.labels.iter().any(|l| l == "Geo-blocked") {
            continue;
        }
        if filters.exclude_not247_label && stream.labels.iter().any(|l| l == "Not 24/7") {
            continue;
        }
        if !filters.categories.is_empty()
            && !filters
                .categories
                .iter()
                .any(|wanted| channel.categories.iter().any(|c| c == wanted))
        {
            continue;
        }
        if !filters.languages.is_empty()
            && !feed.is_some_and(|feed| {
                filters
                    .languages
                    .iter()
                    .any(|wanted| feed.languages.iter().any(|l| l == wanted))
            })
        {
            continue;
        }
        let matching: Vec<&String> = requested
            .iter()
            .filter(|country| match config.geography.mode {
                CountryMode::Origin => canonical(aliases, &channel.country) == **country,
                _ => feed.is_some_and(|feed| {
                    feed.broadcast_area
                        .iter()
                        .any(|raw| geo.covers(raw, country))
                }),
            })
            .collect();
        if matching.is_empty() {
            continue;
        }
        let category_id = filters
            .categories
            .iter()
            .find(|wanted| channel.categories.contains(wanted))
            .or(channel.categories.first())
            .map_or("undefined", String::as_str);
        let category_name = categories_by_id
            .get(category_id)
            .map_or(category_id, |c| c.name.as_str());
        let logo = choose_logo(channel_id, stream.feed.as_deref());
        let tvg_id = match stream.feed.as_deref() {
            Some(feed) => format!("{channel_id}@{feed}"),
            None => channel_id.to_owned(),
        };
        // Alternative streams of one channel feed share a name so they become stream
        // variants of a single logical channel rather than duplicate cards.
        let channel_name = channel.name.trim();
        let name = match feed
            .filter(|f| !f.is_main && !f.name.trim().is_empty() && f.name.trim() != channel_name)
        {
            Some(feed) => format!("{channel_name} ({})", feed.name.trim()),
            None => channel_name.to_owned(),
        };
        for country in matching {
            let count_key = format!("{country}\u{1f}{channel_id}\u{1f}{tvg_id}");
            let count = feed_counts.get(&count_key).copied().unwrap_or(0);
            if (!config.playlist.include_every_alternative_stream && count >= 1)
                || count >= MAX_VARIANTS
            {
                continue;
            }
            let dedupe_key = format!("{count_key}\u{1f}{}", stream.url);
            if !seen.insert(dedupe_key) {
                continue;
            }
            feed_counts.insert(count_key, count + 1);
            let country_name = countries_by_code
                .get(country)
                .map_or(country.as_str(), |c| c.name.as_str());
            let group = config
                .playlist
                .group_by
                .iter()
                .map(|dimension| match dimension {
                    GroupBy::Country => country_name,
                    GroupBy::Category => category_name,
                })
                .collect::<Vec<_>>()
                .join(" / ");
            entries.push(Entry {
                tvg_id: tvg_id.clone(),
                tvg_name: channel_name.to_owned(),
                logo: logo.clone(),
                group,
                country: country.clone(),
                name: name.clone(),
                url: stream.url.trim().to_owned(),
                referrer: stream.referrer.clone().filter(|v| !v.trim().is_empty()),
                user_agent: stream.user_agent.clone().filter(|v| !v.trim().is_empty()),
            });
        }
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------------------------
// Extended-M3U serializer. Its output is parsed by the ordinary playlist importer, so every
// entry obeys the same limits as a user-supplied playlist.
// ---------------------------------------------------------------------------------------------

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\r' | '\n' | '\0'))
        .collect::<String>()
        .trim()
        .to_owned()
}
fn attr(text: &str) -> String {
    clean(text).replace('"', "'")
}
fn clip(text: &str, max_bytes: usize) -> String {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
pub fn serialize(entries: &[Entry], epg_urls: &[String], vlc_options: bool) -> String {
    let mut out = String::with_capacity(entries.len() * 256 + 64);
    if epg_urls.is_empty() {
        out.push_str("#EXTM3U\n");
    } else {
        out.push_str(&format!(
            "#EXTM3U x-tvg-url=\"{}\"\n",
            attr(&epg_urls.join(","))
        ));
    }
    for entry in entries {
        let mut name = clip(&clean(&entry.name), 512);
        if name.is_empty() {
            name = clip(&clean(&entry.tvg_id), 512);
        }
        out.push_str(&format!(
            "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" tvg-country=\"{}\" group-title=\"{}\",{name}\n",
            attr(&entry.tvg_id),
            clip(&attr(&entry.tvg_name), 512),
            clip(&attr(&entry.logo), 4096),
            attr(&entry.country),
            clip(&attr(&entry.group), 512),
        ));
        if vlc_options {
            if let Some(referrer) = &entry.referrer {
                out.push_str(&format!(
                    "#EXTVLCOPT:http-referrer={}\n",
                    clip(&clean(referrer), 4096)
                ));
            }
            if let Some(agent) = &entry.user_agent {
                out.push_str(&format!(
                    "#EXTVLCOPT:http-user-agent={}\n",
                    clip(&clean(agent), 4096)
                ));
            }
        }
        // Stream URLs stay verbatim, including query strings and pipe-delimited hints.
        out.push_str(&clean(&entry.url));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Normalized catalogue for UI pickers: stable codes plus presentation-only names.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCountry {
    pub code: String,
    pub name: String,
    pub flag: String,
    pub official_language_codes: Vec<String>,
    /// A constructed candidate, not proof that the playlist is published.
    pub playlist_candidate_url: String,
}
#[derive(Clone, Serialize)]
pub struct CatalogRegion {
    pub code: String,
    pub name: String,
    pub countries: Vec<String>,
}
#[derive(Clone, Serialize)]
pub struct Catalog {
    pub countries: Vec<CatalogCountry>,
    pub categories: Vec<Category>,
    pub languages: Vec<Language>,
    pub regions: Vec<CatalogRegion>,
}
fn text(value: &str, max_chars: usize) -> String {
    clean(value).chars().take(max_chars).collect()
}
/// Loads the four presentation catalogues; bounded for the UI contract.
pub fn load_catalog<F: FnMut(&str) -> Result<Box<dyn Read>>>(
    mut fetch: F,
    playlist_base: &Url,
) -> Result<Catalog> {
    let countries: Vec<Country> = read_all(&mut fetch, "countries.json")?;
    let categories: Vec<Category> = read_all(&mut fetch, "categories.json")?;
    let languages: Vec<Language> = read_all(&mut fetch, "languages.json")?;
    let regions: Vec<Region> = read_all(&mut fetch, "regions.json")?;
    let aliases = default_aliases();
    // languages.json lists every ISO 639-3 code; the picker offers the ones countries use.
    let spoken: HashSet<String> = countries
        .iter()
        .flat_map(|c| c.languages.iter().map(|l| l.to_ascii_lowercase()))
        .collect();
    Ok(Catalog {
        countries: countries
            .iter()
            .filter(|c| is_code(&c.code, 2, 2, b""))
            .take(400)
            .map(|c| CatalogCountry {
                code: c.code.to_ascii_uppercase(),
                name: text(&c.name, 128),
                flag: text(&c.flag, 8),
                official_language_codes: c.languages.iter().take(32).map(|l| text(l, 8)).collect(),
                playlist_candidate_url: official_url(
                    playlist_base,
                    &OfficialPlaylist {
                        kind: PlaylistKind::Country,
                        code: c.code.clone(),
                    },
                    &aliases,
                )
                .map(|(url, _)| url)
                .unwrap_or_default(),
            })
            .collect(),
        categories: categories
            .iter()
            .filter(|c| is_category(&c.id))
            .take(128)
            .map(|c| Category {
                id: c.id.clone(),
                name: text(&c.name, 128),
            })
            .collect(),
        languages: languages
            .iter()
            .filter(|l| {
                is_code(&l.code, 3, 3, b"")
                    && (spoken.is_empty() || spoken.contains(&l.code.to_ascii_lowercase()))
            })
            .take(1024)
            .map(|l| Language {
                code: l.code.to_ascii_lowercase(),
                name: text(&l.name, 128),
            })
            .collect(),
        regions: regions
            .iter()
            .filter(|r| is_code(&r.code, 2, 8, b""))
            .take(128)
            .map(|r| CatalogRegion {
                code: r.code.to_ascii_uppercase(),
                name: text(&r.name, 128),
                countries: r
                    .countries
                    .iter()
                    .filter(|c| is_code(c, 2, 2, b""))
                    .take(400)
                    .map(|c| c.to_ascii_uppercase())
                    .collect(),
            })
            .collect(),
    })
}
