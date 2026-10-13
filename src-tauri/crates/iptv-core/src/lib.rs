pub mod db;
pub mod error;
pub mod iptv_org;
pub mod limits;
pub mod logos;
pub mod m3u;
pub mod manifest;
pub mod models;
pub mod network;
pub mod relay;
pub mod vault;
pub mod xmltv;

use db::Database;
use error::{Error, Result};
use limits::{LimitedRead, MAX_JSON_BYTES, MAX_M3U_BYTES, MAX_XML_BYTES};
use models::{ChannelPage, ChannelQuery, Job, SourceInput};
use network::NetworkPolicy;
use relay::Relay;
use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Cursor, Read},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use uuid::Uuid;
use vault::Vault;

struct TrackedJob {
    job: Job,
    cancel: Arc<AtomicBool>,
}
struct CatalogCache {
    fetched: Instant,
    key: String,
    catalog: iptv_org::Catalog,
}

pub struct Core {
    pub database: Database,
    pub vault: Vault,
    pub relay: Relay,
    pub changes: tokio::sync::watch::Sender<u64>,
    jobs: Mutex<HashMap<String, TrackedJob>>,
    source_gate: Mutex<()>,
    imports: Arc<Semaphore>,
    catalog: Mutex<Option<CatalogCache>>,
    logo_dir: PathBuf,
    logo_permits: Arc<Semaphore>,
    logo_failures: Mutex<HashMap<String, Instant>>,
    logo_writes: AtomicU64,
    _profile_lock: File,
}
impl Core {
    pub async fn open(directory: PathBuf) -> Result<Arc<Self>> {
        db::private_directory(&directory)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let profile_lock = options.open(directory.join("profile.lock"))?;
        profile_lock.try_lock().map_err(|_| Error::Busy)?;
        let database = Database::open(directory.join("catalogue.db"))?;
        // Records sealed under the former passphrase vault cannot be opened without it; such a
        // profile is reset once so the application starts without any prompt.
        let legacy = directory.join("credentials.vault");
        if legacy.exists() {
            database.reset_sources()?;
            std::fs::remove_file(&legacy)?;
        }
        let vault = Vault::new(directory.join("credentials.key"));
        vault.initialize()?;
        let logo_dir = directory.join("logos");
        db::private_directory(&logo_dir)?;
        let core = Arc::new(Self {
            database,
            vault,
            relay: Relay::bind().await?,
            changes: tokio::sync::watch::channel(0).0,
            jobs: Mutex::new(HashMap::new()),
            source_gate: Mutex::new(()),
            imports: Arc::new(Semaphore::new(1)),
            catalog: Mutex::new(None),
            logo_dir,
            logo_permits: Arc::new(Semaphore::new(4)),
            logo_failures: Mutex::new(HashMap::new()),
            logo_writes: AtomicU64::new(0),
            _profile_lock: profile_lock,
        });
        core.relay.attach(&core);
        let weak = Arc::downgrade(&core);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15 * 60));
            loop {
                interval.tick().await;
                let Some(core) = weak.upgrade() else { break };
                let cloned = core.clone();
                let _ = tokio::task::spawn_blocking(move || cloned.refresh_due()).await;
            }
        });
        Ok(core)
    }
    /// Catalogue page with artwork addresses pointing at the loopback relay.
    pub fn channels(&self, query: &ChannelQuery) -> Result<ChannelPage> {
        let mut page = self.database.channels(query)?;
        for item in &mut page.items {
            if item.has_logo {
                item.logo_url = Some(self.relay.logo_url(&item.id));
            }
        }
        Ok(page)
    }
    /// Artwork for one catalogued channel: served from the private disk cache, otherwise
    /// fetched once through the image policy with bounded concurrency. Failures are
    /// remembered for an hour so a dead image host is not retried on every scroll.
    pub async fn logo(self: &Arc<Self>, channel: &str) -> Result<(Vec<u8>, &'static str)> {
        if let Ok(failures) = self.logo_failures.lock()
            && failures
                .get(channel)
                .is_some_and(|at| at.elapsed() < Duration::from_secs(3600))
        {
            return Err(Error::NotFound);
        }
        let core = self.clone();
        let id = channel.to_owned();
        let (url, config) =
            tokio::task::spawn_blocking(move || core.database.channel_logo(&id, &core.vault))
                .await
                .map_err(|_| Error::Storage)??;
        let path = self
            .logo_dir
            .join(Uuid::new_v5(&Uuid::NAMESPACE_URL, url.as_bytes()).to_string());
        let cached = path.clone();
        if let Ok(Some(entry)) =
            tokio::task::spawn_blocking(move || logos::read_cached(&cached)).await
        {
            return Ok(entry);
        }
        let _permit = self
            .logo_permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Cancelled)?;
        match logos::fetch(&url, &config).await {
            Ok((data, content_type)) => {
                let bytes = data.clone();
                let directory = self.logo_dir.clone();
                let prune = self
                    .logo_writes
                    .fetch_add(1, Ordering::Relaxed)
                    .is_multiple_of(200);
                let _ = tokio::task::spawn_blocking(move || {
                    let _ = logos::write_cached(&path, content_type, &bytes);
                    if prune {
                        logos::prune(&directory, 3000);
                    }
                })
                .await;
                Ok((data, content_type))
            }
            Err(error) => {
                if let Ok(mut failures) = self.logo_failures.lock() {
                    if failures.len() >= 4096 {
                        failures.clear();
                    }
                    failures.insert(channel.to_owned(), Instant::now());
                }
                Err(error)
            }
        }
    }
    /// Cancels imports, ends playback and drops the key from memory; used at exit.
    pub fn shutdown(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.relay.close(None);
        self.vault.clear();
    }
    pub fn remove_source(&self, source: &str) -> Result<()> {
        let _gate = self.source_gate.lock().map_err(|_| Error::Storage)?;
        if self.source_busy(source) {
            return Err(Error::Busy);
        }
        self.database.remove_source(source)?;
        self.relay.close(None);
        self.notify_changed();
        Ok(())
    }
    pub fn source_busy(&self, source: &str) -> bool {
        self.jobs.lock().map_or(true, |jobs| {
            jobs.values().any(|j| {
                j.job.source_id == source && matches!(j.job.state.as_str(), "queued" | "running")
            })
        })
    }
    pub fn start_job(self: &Arc<Self>, source_id: String) -> Result<String> {
        let _gate = self.source_gate.lock().map_err(|_| Error::Storage)?;
        db::valid_id(&source_id)?;
        // Read/validate source existence before enqueuing; caller executes this on a worker.
        self.database.source_config(&source_id, &self.vault)?;
        let id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().map_err(|_| Error::Storage)?;
            if jobs.values().any(|j| {
                j.job.source_id == source_id && matches!(j.job.state.as_str(), "queued" | "running")
            }) {
                return Err(Error::Busy);
            }
            if jobs
                .values()
                .filter(|j| matches!(j.job.state.as_str(), "queued" | "running"))
                .count()
                >= 4
            {
                return Err(Error::Busy);
            }
            if jobs.len() >= 64 {
                jobs.retain(|_, j| matches!(j.job.state.as_str(), "queued" | "running"));
            }
            jobs.insert(
                id.clone(),
                TrackedJob {
                    job: Job {
                        id: id.clone(),
                        source_id: source_id.clone(),
                        state: "queued".into(),
                        phase: "waiting".into(),
                        processed: 0,
                        error: None,
                        created_at: chrono::Utc::now().timestamp_millis(),
                    },
                    cancel: cancel.clone(),
                },
            );
        }
        let core = self.clone();
        let job_id = id.clone();
        // A Tokio runtime handle must be entered by the outer Tauri command worker.
        tokio::spawn(async move {
            let permit = core.imports.clone().acquire_owned().await;
            if permit.is_err() {
                core.finish(&job_id, Err(Error::Cancelled));
                return;
            }
            let _permit = permit.expect("checked semaphore result");
            let work = core.clone();
            let job = job_id.clone();
            let result = tokio::task::spawn_blocking(move || work.import(&source_id, &job, cancel))
                .await
                .unwrap_or(Err(Error::Storage));
            core.finish(&job_id, result);
        });
        Ok(id)
    }
    pub fn jobs(&self) -> Result<Vec<Job>> {
        let jobs = self.jobs.lock().map_err(|_| Error::Storage)?;
        let mut result = jobs.values().map(|j| j.job.clone()).collect::<Vec<_>>();
        result.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(result)
    }
    pub fn notify_changed(&self) {
        self.changes.send_modify(|revision| *revision += 1);
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        db::valid_id(id)?;
        self.jobs
            .lock()
            .map_err(|_| Error::Storage)?
            .get(id)
            .map(|j| j.job.clone())
            .ok_or(Error::NotFound)
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        db::valid_id(id)?;
        let jobs = self.jobs.lock().map_err(|_| Error::Storage)?;
        let job = jobs.get(id).ok_or(Error::NotFound)?;
        job.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }
    fn progress(&self, id: &str, phase: &str, processed: u64) {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.get_mut(id)
        {
            job.job.state = "running".into();
            job.job.phase = phase.into();
            job.job.processed = processed;
        }
    }
    fn finish(&self, id: &str, result: Result<()>) {
        let success = result.is_ok();
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.get_mut(id)
        {
            match result {
                Ok(()) => {
                    job.job.state = "completed".into();
                    job.job.phase = "ready".into();
                }
                Err(error) => {
                    let cancelled =
                        job.cancel.load(Ordering::Relaxed) || matches!(error, Error::Cancelled);
                    job.job.state = if cancelled { "cancelled" } else { "failed" }.into();
                    job.job.error = if cancelled {
                        None
                    } else {
                        Some(error.to_string())
                    };
                }
            }
        }
        if success {
            self.notify_changed();
        }
    }
    fn import(&self, source_id: &str, job_id: &str, cancel: Arc<AtomicBool>) -> Result<()> {
        check_cancel(&cancel)?;
        let config = self.database.source_config(source_id, &self.vault)?;
        let mut import = self.database.importer(source_id, &self.vault)?;
        let count = match &config.iptv_org {
            None => self.import_playlist(&config, job_id, &cancel, &mut import)?,
            Some(org) => self.import_iptv_org(&config, org, job_id, &cancel, &mut import)?,
        };
        for (priority, epg_url) in config.epg_urls.iter().enumerate() {
            check_cancel(&cancel)?;
            self.progress(job_id, "epg", 0);
            let policy = NetworkPolicy::new(epg_url, &config)?;
            let response = policy.get_blocking(epg_url, &BTreeMap::new())?;
            let reader = decoded(response, MAX_XML_BYTES, cancel.clone())?;
            // Feed identity is slot-based so rotating an access token does not change it.
            let source = Uuid::parse_str(source_id).map_err(|_| Error::Storage)?;
            let feed = Uuid::new_v5(&source, format!("epg:{priority}").as_bytes()).to_string();
            let now = chrono::Utc::now().timestamp();
            let mut processed = 0u64;
            xmltv::parse(reader, |record| {
                check_cancel(&cancel)?;
                match record {
                    xmltv::Record::Channel(channel) => {
                        import.epg_channel(&feed, priority, channel)?
                    }
                    xmltv::Record::Programme(programme) => {
                        import.programme(&feed, programme, now)?
                    }
                }
                processed += 1;
                if processed.is_multiple_of(256) {
                    self.progress(job_id, "epg", processed);
                }
                Ok(())
            })?;
        }
        check_cancel(&cancel)?;
        self.progress(job_id, "activating", count);
        import.activate()
    }
    /// Own M3U playlists: one bounded download parsed straight into the staging generation.
    fn import_playlist(
        &self,
        config: &SourceInput,
        job_id: &str,
        cancel: &Arc<AtomicBool>,
        import: &mut db::Import<'_>,
    ) -> Result<u64> {
        let policy = NetworkPolicy::new(&config.playlist_url, config)?;
        self.progress(job_id, "playlist", 0);
        let response = policy.get_blocking(&config.playlist_url, &BTreeMap::new())?;
        let reader = decoded(response, MAX_M3U_BYTES, cancel.clone())?;
        let mut count = 0u64;
        let metadata = m3u::parse(reader, |mut channel| {
            check_cancel(cancel)?;
            if channel.country.is_empty() {
                channel.country = config.country_code.to_ascii_uppercase();
            }
            import.channel(channel)?;
            count += 1;
            if count.is_multiple_of(256) {
                self.progress(job_id, "playlist", count);
            }
            Ok(())
        })?;
        // Header-advertised EPG URLs are parsed but never followed implicitly. Only user
        // configured feeds have network authority; this prevents a playlist granting it.
        let _suggestions = metadata.suggested_epg_urls;
        Ok(count)
    }
    /// iptv-org sources: official prebuilt playlists, or a local JSON selection serialized to
    /// M3U and parsed by the same importer as user playlists.
    fn import_iptv_org(
        &self,
        config: &SourceInput,
        org: &iptv_org::Config,
        job_id: &str,
        cancel: &Arc<AtomicBool>,
        import: &mut db::Import<'_>,
    ) -> Result<u64> {
        let base = iptv_org::playlist_base(config)?;
        let policy = NetworkPolicy::new(base.as_str(), config)?;
        let mut count = 0u64;
        let mut skipped = 0u64;
        match org.method {
            iptv_org::Method::Playlist => {
                for pick in &org.playlists {
                    check_cancel(cancel)?;
                    let (url, country) =
                        iptv_org::official_url(&base, pick, &org.geography.country_aliases)?;
                    self.progress(job_id, "playlist", count);
                    let response = policy.get_blocking(&url, &BTreeMap::new())?;
                    let reader = decoded(response, MAX_M3U_BYTES, cancel.clone())?;
                    m3u::parse_with(reader, true, |mut channel| {
                        check_cancel(cancel)?;
                        // Only streams this source's policy can fetch are catalogued, exactly
                        // as the catalogue method does; HTTP needs the source's consent.
                        if url::Url::parse(&channel.stream.url)
                            .is_ok_and(|url| policy.check_url(&url).is_err())
                            || org.filters.excludes_official_name(&channel.name)
                        {
                            skipped += 1;
                            return Ok(());
                        }
                        if channel.country.is_empty() {
                            channel.country = country
                                .clone()
                                .unwrap_or_else(|| config.country_code.to_ascii_uppercase());
                        }
                        import.channel(channel)?;
                        count += 1;
                        if count.is_multiple_of(256) {
                            self.progress(job_id, "playlist", count);
                        }
                        Ok(())
                    })?;
                }
                if count == 0 {
                    return Err(Error::Invalid(if skipped > 0 {
                        "Every stream in the selected playlists was excluded by this source's address policy or label filters; allow HTTP or relax the filters and refresh."
                    } else {
                        "The selected iptv-org playlists contain no channels; the previous catalogue was preserved."
                    }));
                }
            }
            iptv_org::Method::Catalogue => {
                let api = org.api_url()?.as_str().trim_end_matches('/').to_owned();
                let mut loaded = 0u64;
                self.progress(job_id, "metadata", 0);
                let dataset = iptv_org::load(|file| {
                    check_cancel(cancel)?;
                    let response =
                        policy.get_blocking(&format!("{api}/{file}"), &BTreeMap::new())?;
                    let reader = decoded(response, MAX_JSON_BYTES, cancel.clone())?;
                    loaded += 1;
                    self.progress(job_id, "metadata", loaded);
                    Ok(Box::new(reader) as Box<dyn Read>)
                })?;
                check_cancel(cancel)?;
                self.progress(job_id, "selection", 0);
                let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
                let entries =
                    iptv_org::select(&dataset, org, &today, |url| policy.check_url(url).is_ok())?;
                drop(dataset);
                if entries.is_empty() {
                    return Err(Error::Invalid(
                        "No iptv-org streams match this selection; the previous catalogue was preserved.",
                    ));
                }
                self.progress(job_id, "selection", entries.len() as u64);
                let text = iptv_org::serialize(
                    &entries,
                    &config.epg_urls,
                    org.playlist.emit_vlc_http_options,
                );
                drop(entries);
                m3u::parse(Cursor::new(text), |channel| {
                    check_cancel(cancel)?;
                    import.channel(channel)?;
                    count += 1;
                    if count.is_multiple_of(256) {
                        self.progress(job_id, "playlist", count);
                    }
                    Ok(())
                })?;
            }
        }
        Ok(count)
    }
    pub fn refresh_due(self: &Arc<Self>) {
        if let Ok(sources) = self.database.sources() {
            let now = chrono::Utc::now().timestamp();
            for source in sources {
                // Own playlists keep the six-hour cadence; iptv-org sources use their own.
                let interval = self
                    .database
                    .source_config(&source.id, &self.vault)
                    .map_or(6 * 3600, |config| config.refresh_interval_seconds());
                if source.updated_at.is_none_or(|last| now - last >= interval) {
                    let _ = self.start_job(source.id);
                }
            }
        }
    }
    /// Presentation catalogue (countries, categories, languages, regions) for the source
    /// form, fetched read-only from the API host and cached for a day.
    pub fn iptv_org_catalog(
        &self,
        api_base: Option<&str>,
        playlist_base: Option<&str>,
    ) -> Result<iptv_org::Catalog> {
        let api = iptv_org::base_url(api_base.unwrap_or_default(), iptv_org::DEFAULT_API_BASE)?;
        let playlists = iptv_org::base_url(
            playlist_base.unwrap_or_default(),
            iptv_org::DEFAULT_PLAYLIST_BASE,
        )?;
        let key = format!("{api}\u{1f}{playlists}");
        if let Ok(cache) = self.catalog.lock()
            && let Some(entry) = cache.as_ref()
            && entry.key == key
            && entry.fetched.elapsed() < Duration::from_secs(24 * 3600)
        {
            return Ok(entry.catalog.clone());
        }
        let policy = NetworkPolicy::public(api.as_str())?;
        let api = api.as_str().trim_end_matches('/').to_owned();
        let catalog = iptv_org::load_catalog(
            |file| {
                let response = policy.get_blocking(&format!("{api}/{file}"), &BTreeMap::new())?;
                let reader = decoded(response, MAX_JSON_BYTES, Arc::new(AtomicBool::new(false)))?;
                Ok(Box::new(reader) as Box<dyn Read>)
            },
            &playlists,
        )?;
        if let Ok(mut cache) = self.catalog.lock() {
            *cache = Some(CatalogCache {
                fetched: Instant::now(),
                key,
                catalog: catalog.clone(),
            });
        }
        Ok(catalog)
    }
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn decoded(
    response: reqwest::blocking::Response,
    limit: u64,
    cancel: Arc<AtomicBool>,
) -> Result<BufReader<LimitedRead<Box<dyn Read + Send>>>> {
    let mut input =
        BufReader::with_capacity(16 * 1024, LimitedRead::new(response, limit, cancel.clone()));
    let magic = input.fill_buf().map_err(|_| Error::Network)?;
    let gzip = magic.starts_with(&[0x1f, 0x8b]);
    let reader: Box<dyn Read + Send> = if gzip {
        Box::new(flate2::read::GzDecoder::new(input))
    } else {
        Box::new(input)
    };
    // Both compressed transfer and decompressed output have independent absolute limits.
    Ok(BufReader::with_capacity(
        16 * 1024,
        LimitedRead::new(reader, limit, cancel),
    ))
}
