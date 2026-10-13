pub mod db;
pub mod error;
pub mod limits;
pub mod m3u;
pub mod manifest;
pub mod models;
pub mod network;
pub mod relay;
pub mod vault;
pub mod xmltv;

use db::Database;
use error::{Error, Result};
use limits::{LimitedRead, MAX_M3U_BYTES, MAX_XML_BYTES};
use models::Job;
use network::NetworkPolicy;
use relay::Relay;
use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;
use uuid::Uuid;
use vault::Vault;
use zeroize::Zeroizing;

struct TrackedJob {
    job: Job,
    cancel: Arc<AtomicBool>,
}

pub struct Core {
    pub database: Database,
    pub vault: Vault,
    pub relay: Relay,
    pub changes: tokio::sync::watch::Sender<u64>,
    jobs: Mutex<HashMap<String, TrackedJob>>,
    source_gate: Mutex<()>,
    imports: Arc<Semaphore>,
    unlocks: Arc<Semaphore>,
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
        let core = Arc::new(Self {
            database,
            vault: Vault::new(directory.join("credentials.vault")),
            relay: Relay::bind().await?,
            changes: tokio::sync::watch::channel(0).0,
            jobs: Mutex::new(HashMap::new()),
            source_gate: Mutex::new(()),
            imports: Arc::new(Semaphore::new(1)),
            unlocks: Arc::new(Semaphore::new(1)),
            _profile_lock: profile_lock,
        });
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
    pub fn require_unlocked(&self) -> Result<()> {
        if self.vault.is_unlocked() {
            Ok(())
        } else {
            Err(Error::Locked)
        }
    }
    pub async fn unlock(self: &Arc<Self>, password: String) -> Result<()> {
        let _permit = self
            .unlocks
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::Cancelled)?;
        let core = self.clone();
        let result =
            tokio::task::spawn_blocking(move || core.vault.unlock(Zeroizing::new(password)))
                .await
                .map_err(|_| Error::Vault)?;
        // Throttle repeated local attempts without blocking the UI or retaining passwords.
        if result.is_err() {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        result?;
        let core = self.clone();
        let _ = tokio::task::spawn_blocking(move || core.refresh_due()).await;
        Ok(())
    }
    pub fn lock(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.relay.close(None);
        self.vault.lock();
    }
    pub fn remove_source(&self, source: &str) -> Result<()> {
        let _gate = self.source_gate.lock().map_err(|_| Error::Storage)?;
        self.require_unlocked()?;
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
        self.require_unlocked()?;
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
        self.require_unlocked()?;
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
        self.require_unlocked()?;
        db::valid_id(id)?;
        self.jobs
            .lock()
            .map_err(|_| Error::Storage)?
            .get(id)
            .map(|j| j.job.clone())
            .ok_or(Error::NotFound)
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        self.require_unlocked()?;
        db::valid_id(id)?;
        let jobs = self.jobs.lock().map_err(|_| Error::Storage)?;
        let job = jobs.get(id).ok_or(Error::NotFound)?;
        job.cancel.store(true, Ordering::Relaxed);
        Ok(())
    }
    fn progress(&self, id: &str, phase: &str, processed: u64) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get_mut(id) {
                job.job.state = "running".into();
                job.job.phase = phase.into();
                job.job.processed = processed;
            }
        }
    }
    fn finish(&self, id: &str, result: Result<()>) {
        let success = result.is_ok();
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get_mut(id) {
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
        }
        if success {
            self.notify_changed();
        }
    }
    fn import(&self, source_id: &str, job_id: &str, cancel: Arc<AtomicBool>) -> Result<()> {
        check_cancel(&cancel)?;
        self.require_unlocked()?;
        let config = self.database.source_config(source_id, &self.vault)?;
        let policy = NetworkPolicy::new(&config.playlist_url, &config)?;
        self.progress(job_id, "playlist", 0);
        let response = policy.get_blocking(&config.playlist_url, &BTreeMap::new())?;
        let reader = decoded(response, MAX_M3U_BYTES, cancel.clone())?;
        let mut import = self.database.importer(source_id, &self.vault)?;
        let mut count = 0u64;
        let metadata = m3u::parse(reader, |mut channel| {
            check_cancel(&cancel)?;
            if channel.country.is_empty() {
                channel.country = config.country_code.to_ascii_uppercase();
            }
            import.channel(channel)?;
            count += 1;
            if count % 256 == 0 {
                self.progress(job_id, "playlist", count);
            }
            Ok(())
        })?;
        // Header-advertised EPG URLs are parsed but never followed implicitly. Only user
        // configured feeds have network authority; this prevents a playlist granting it.
        let _suggestions = metadata.suggested_epg_urls;
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
                if processed % 256 == 0 {
                    self.progress(job_id, "epg", processed);
                }
                Ok(())
            })?;
        }
        check_cancel(&cancel)?;
        self.require_unlocked()?;
        self.progress(job_id, "activating", count);
        import.activate()
    }
    pub fn refresh_due(self: &Arc<Self>) {
        if !self.vault.is_unlocked() {
            return;
        }
        if let Ok(sources) = self.database.sources() {
            let now = chrono::Utc::now().timestamp();
            for source in sources {
                if source.updated_at.is_none_or(|last| now - last >= 6 * 3600) {
                    let _ = self.start_job(source.id);
                }
            }
        }
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
