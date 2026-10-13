use crate::{
    error::{Error, Result},
    models::*,
    vault::Vault,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;

const CHANNEL_FILTER: &str = "FROM channel_entries c JOIN sources s ON s.id=c.source_id AND s.active_generation=c.generation
            WHERE (?1='' OR c.name LIKE ?2 ESCAPE '\\') AND (?3 IS NULL OR c.source_id=?3)
            AND (?4 IS NULL OR EXISTS(SELECT 1 FROM channel_groups g WHERE g.generation=c.generation AND g.channel_id=c.id AND g.name=?4))
            AND (?5=0 OR EXISTS(SELECT 1 FROM favorites f WHERE f.channel_id=c.id)) AND (?6 IS NULL OR EXISTS(SELECT 1 FROM channel_countries cc WHERE cc.generation=c.generation AND cc.channel_id=c.id AND cc.code=?6))";

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}
impl Database {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            private_directory(parent)?;
        }
        let db = Self { path };
        let mut connection = db.connect()?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            tx.commit()?;
        } else if version != 1 {
            return Err(Error::Invalid(
                "The database was created by an unsupported application version.",
            ));
        }
        // Only called while the process holds the exclusive profile lock.
        connection.execute("DELETE FROM generations WHERE state='staging'", [])?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&db.path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(db)
    }
    pub fn connect(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-4096; PRAGMA temp_store=FILE;")?;
        Ok(connection)
    }
    pub fn save_source(&self, input: &SourceInput, vault: &Vault) -> Result<String> {
        crate::network::validate_source(input)?;
        let mut connection = self.connect()?;
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))?;
        if count >= 32 {
            return Err(Error::Limit);
        }
        let id = Uuid::new_v4().to_string();
        let plain = Zeroizing::new(serde_json::to_vec(input)?);
        let encrypted = vault.seal(&format!("source:{id}"), &plain)?;
        tx.execute(
            "INSERT INTO sources(id,name,config,has_epg) VALUES(?1,?2,?3,?4)",
            params![id, input.name.trim(), encrypted, !input.epg_urls.is_empty()],
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn source_config(&self, id: &str, vault: &Vault) -> Result<SourceInput> {
        valid_id(id)?;
        let bytes: Vec<u8> = self
            .connect()?
            .query_row("SELECT config FROM sources WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok(serde_json::from_slice(
            &vault.open(&format!("source:{id}"), &bytes)?,
        )?)
    }
    pub fn sources(&self) -> Result<Vec<Source>> {
        let connection = self.connect()?;
        let mut statement = connection.prepare("SELECT s.id,s.name,s.updated_at,s.has_epg,(SELECT COUNT(*) FROM channel_entries c WHERE c.generation=s.active_generation) FROM sources s ORDER BY s.name COLLATE NOCASE,s.id")?;
        let rows = statement.query_map([], |r| {
            Ok(Source {
                id: r.get(0)?,
                name: r.get(1)?,
                updated_at: r.get(2)?,
                has_epg: r.get(3)?,
                channels: r.get(4)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    pub fn remove_source(&self, id: &str) -> Result<()> {
        valid_id(id)?;
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM favorites WHERE channel_id IN (SELECT id FROM channel_entries WHERE source_id=?1)", [id])?;
        tx.execute("DELETE FROM epg_overrides WHERE channel_id IN (SELECT id FROM channel_entries WHERE source_id=?1)", [id])?;
        if tx.execute("DELETE FROM sources WHERE id=?1", [id])? == 0 {
            return Err(Error::NotFound);
        }
        bump_revision(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn channels(&self, query: &ChannelQuery) -> Result<ChannelPage> {
        if query.limit == 0
            || query.limit > 120
            || query.offset > 2_000_000
            || query.search.len() > 256
            || query.group.as_ref().is_some_and(|v| v.len() > 512)
        {
            return Err(Error::Limit);
        }
        if let Some(id) = &query.source_id {
            valid_id(id)?;
        }
        if query
            .country
            .as_ref()
            .is_some_and(|c| c.len() != 2 || !c.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            return Err(Error::Invalid("Invalid country filter."));
        }
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;
        let revision = revision(&tx)?;
        if query.revision.is_some_and(|v| v != revision) {
            return Err(Error::Stale);
        }
        let pattern = format!(
            "%{}%",
            query
                .search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let filter = CHANNEL_FILTER;
        let total = tx.query_row(
            &format!("SELECT COUNT(*) {filter}"),
            params![
                query.search,
                pattern,
                query.source_id,
                query.group,
                query.favorites_only,
                query.country
            ],
            |r| r.get(0),
        )?;
        let sql = format!("SELECT c.id,c.name,c.source_id,c.country,
            EXISTS(SELECT 1 FROM favorites f WHERE f.channel_id=c.id),
            (SELECT COUNT(*) FROM stream_variants v WHERE v.generation=c.generation AND v.channel_id=c.id),
            EXISTS(SELECT 1 FROM epg_links e WHERE e.generation=c.generation AND e.channel_id=c.id)
            {filter} ORDER BY c.name COLLATE NOCASE,c.id LIMIT ?7 OFFSET ?8");
        let items = {
            let mut statement = tx.prepare(&sql)?;
            let rows = statement.query_map(
                params![
                    query.search,
                    pattern,
                    query.source_id,
                    query.group,
                    query.favorites_only,
                    query.country,
                    query.limit,
                    query.offset
                ],
                |r| {
                    Ok(Channel {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        source_id: r.get(2)?,
                        country: r.get(3)?,
                        favorite: r.get(4)?,
                        variants: r.get(5)?,
                        has_epg: r.get(6)?,
                    })
                },
            )?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        tx.commit()?;
        Ok(ChannelPage {
            items,
            total,
            revision,
            offset: query.offset,
        })
    }
    pub fn countries(&self, source: Option<&str>) -> Result<Vec<Country>> {
        if let Some(source) = source {
            valid_id(source)?;
        }
        let connection = self.connect()?;
        let mut statement=connection.prepare("SELECT cc.code,COUNT(DISTINCT cc.channel_id) FROM channel_countries cc JOIN sources s ON s.active_generation=cc.generation WHERE (?1 IS NULL OR s.id=?1) GROUP BY cc.code ORDER BY cc.code LIMIT 300")?;
        let rows = statement.query_map([source], |r| {
            Ok(Country {
                code: r.get(0)?,
                count: r.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    pub fn locate(&self, query: &ChannelQuery, id: &str) -> Result<ChannelLocation> {
        valid_id(id)?;
        if query.search.len() > 256 || query.group.as_ref().is_some_and(|v| v.len() > 512) {
            return Err(Error::Limit);
        }
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;
        let rev = revision(&tx)?;
        if query.revision.is_some_and(|r| r != rev) {
            return Err(Error::Stale);
        }
        let pattern = format!(
            "%{}%",
            query
                .search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let sql = format!(
            "WITH ranked AS (SELECT c.id,ROW_NUMBER() OVER(ORDER BY c.name COLLATE NOCASE,c.id)-1 AS position {CHANNEL_FILTER}) SELECT position FROM ranked WHERE id=?7"
        );
        let index = tx
            .query_row(
                &sql,
                params![
                    query.search,
                    pattern,
                    query.source_id,
                    query.group,
                    query.favorites_only,
                    query.country,
                    id
                ],
                |r| r.get(0),
            )
            .optional()?;
        tx.commit()?;
        Ok(ChannelLocation {
            index,
            revision: rev,
        })
    }
    pub fn groups(&self, source: Option<&str>) -> Result<Vec<String>> {
        if let Some(source) = source {
            valid_id(source)?;
        }
        let connection = self.connect()?;
        let mut statement = connection.prepare("SELECT DISTINCT g.name FROM channel_groups g JOIN generations v ON v.id=g.generation JOIN sources s ON s.active_generation=v.id WHERE (?1 IS NULL OR s.id=?1) ORDER BY g.name COLLATE NOCASE LIMIT 512")?;
        let rows = statement.query_map([source], |r| r.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    pub fn guide(&self, query: &GuideQuery) -> Result<GuideWindow> {
        if query.channel_ids.is_empty() {
            return Ok(GuideWindow {
                items: vec![],
                revision: query.revision,
            });
        }
        if query.channel_ids.len() > 40
            || query.end <= query.start
            || query
                .end
                .checked_sub(query.start)
                .is_none_or(|span| span > 6 * 3600)
            || query.start < -62135596800
            || query.end > 253402300799
        {
            return Err(Error::Limit);
        }
        for id in &query.channel_ids {
            valid_id(id)?;
        }
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;
        let rev = revision(&tx)?;
        if rev != query.revision {
            return Err(Error::Stale);
        }
        let ids = serde_json::to_string(&query.channel_ids)?;
        let items = {
            let mut statement = tx.prepare("SELECT c.id,p.start+c.shift_seconds,p.end+c.shift_seconds,p.title,p.description
                FROM channel_entries c JOIN sources s ON s.active_generation=c.generation
                JOIN epg_links l ON l.generation=c.generation AND l.channel_id=c.id
                JOIN programmes p ON p.generation=l.generation AND p.feed_id=l.feed_id AND p.channel_id=l.epg_id
                WHERE c.id IN (SELECT value FROM json_each(?1)) AND p.end>?2-c.shift_seconds AND p.start<?3-c.shift_seconds
                ORDER BY c.id,p.start,p.end,p.title LIMIT 4001")?;
            let rows = statement.query_map(params![ids, query.start, query.end], |r| {
                Ok(Programme {
                    channel_id: r.get(0)?,
                    start: r.get(1)?,
                    end: r.get(2)?,
                    title: r.get(3)?,
                    description: r.get(4)?,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        if items.len() > 4000 {
            return Err(Error::Limit);
        }
        tx.commit()?;
        Ok(GuideWindow {
            items,
            revision: rev,
        })
    }
    pub fn favorite(&self, id: &str, enabled: bool) -> Result<()> {
        valid_id(id)?;
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM channel_entries c JOIN sources s ON s.active_generation=c.generation WHERE c.id=?1)", [id], |r| r.get::<_,bool>(0))? { return Err(Error::NotFound); }
        if enabled {
            tx.execute("INSERT OR IGNORE INTO favorites VALUES(?1)", [id])?;
        } else {
            tx.execute("DELETE FROM favorites WHERE channel_id=?1", [id])?;
        }
        bump_revision(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn mapping(&self, id: &str, feed_id: Option<&str>, epg_id: Option<&str>) -> Result<()> {
        valid_id(id)?;
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;
        let generation: String = tx.query_row("SELECT c.generation FROM channel_entries c JOIN sources s ON s.active_generation=c.generation WHERE c.id=?1",[id],|r|r.get(0)).optional()?.ok_or(Error::NotFound)?;
        match (feed_id, epg_id) {
            (Some(feed), Some(epg)) => {
                valid_id(feed)?;
                if epg.len() > 512 || !tx.query_row("SELECT EXISTS(SELECT 1 FROM epg_channels WHERE generation=?1 AND feed_id=?2 AND id=?3)", params![generation,feed,epg],|r|r.get::<_,bool>(0))? { return Err(Error::NotFound); }
                tx.execute("INSERT INTO epg_overrides VALUES(?1,?2,?3) ON CONFLICT(channel_id) DO UPDATE SET feed_id=excluded.feed_id,epg_id=excluded.epg_id", params![id,feed,epg])?;
            }
            (None, None) => {
                tx.execute("DELETE FROM epg_overrides WHERE channel_id=?1", [id])?;
            }
            _ => {
                return Err(Error::Invalid(
                    "An EPG override requires both feed and channel IDs.",
                ));
            }
        }
        match_epg(&tx, &generation)?;
        bump_revision(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn stream(
        &self,
        id: &str,
        variant: u32,
        vault: &Vault,
    ) -> Result<(String, String, StreamSecret, SourceInput, i64)> {
        valid_id(id)?;
        if variant >= 32 {
            return Err(Error::Limit);
        }
        let connection = self.connect()?;
        let record = connection.query_row("SELECT c.source_id,c.name,v.id,v.secret,
            (SELECT COUNT(*) FROM stream_variants v2 WHERE v2.generation=c.generation AND v2.channel_id=c.id)
            FROM channel_entries c JOIN sources s ON s.active_generation=c.generation
            JOIN stream_variants v ON v.generation=c.generation AND v.channel_id=c.id
            WHERE c.id=?1 ORDER BY v.ordinal,v.id LIMIT 1 OFFSET ?2", params![id,variant], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Vec<u8>>(3)?,r.get::<_,i64>(4)?))).optional()?.ok_or(Error::NotFound)?;
        let config = self.source_config(&record.0, vault)?;
        let secret =
            serde_json::from_slice(&vault.open(&format!("stream:{}", record.2), &record.3)?)?;
        Ok((record.0, record.1, secret, config, record.4))
    }
    pub fn importer<'a>(&self, source_id: &str, vault: &'a Vault) -> Result<Import<'a>> {
        valid_id(source_id)?;
        let connection = self.connect()?;
        let generation = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO generations VALUES(?1,?2,'staging',?3)",
            params![generation, source_id, chrono::Utc::now().timestamp()],
        )?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        Ok(Import {
            connection,
            vault,
            source_id: source_id.to_owned(),
            generation,
            batch: 0,
            ordinal: 0,
            activated: false,
        })
    }
}
pub fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn valid_id(id: &str) -> Result<()> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| Error::Invalid("Invalid identifier."))
}
fn revision(c: &Connection) -> Result<i64> {
    Ok(
        c.query_row("SELECT value FROM app_meta WHERE key='revision'", [], |r| {
            r.get(0)
        })?,
    )
}
fn bump_revision(c: &Connection) -> Result<()> {
    c.execute("UPDATE app_meta SET value=value+1 WHERE key='revision'", [])?;
    Ok(())
}

pub struct Import<'a> {
    connection: Connection,
    vault: &'a Vault,
    source_id: String,
    generation: String,
    batch: usize,
    ordinal: i64,
    activated: bool,
}
impl Import<'_> {
    fn advance(&mut self) -> Result<()> {
        self.batch += 1;
        if self.batch >= 256 {
            self.connection.execute_batch("COMMIT; BEGIN IMMEDIATE;")?;
            self.batch = 0;
        }
        Ok(())
    }
    pub fn channel(&mut self, channel: ParsedChannel) -> Result<()> {
        self.ordinal += 1;
        let namespace = Uuid::parse_str(&self.source_id).map_err(|_| Error::Storage)?;
        // The channel is stable across token/URL changes. Stream variants remain separate.
        let identity = if channel.tvg_id.is_empty() {
            format!("name:{}", normalized(&channel.name))
        } else {
            format!("tvg:{}|{}", channel.tvg_id, normalized(&channel.name))
        };
        let id = Uuid::new_v5(&namespace, identity.as_bytes());
        let stream = Zeroizing::new(serde_json::to_vec(&channel.stream)?);
        let variant = Uuid::new_v5(&id, &stream).to_string();
        let secret = self.vault.seal(&format!("stream:{variant}"), &stream)?;
        let metadata = Zeroizing::new(serde_json::to_vec(
            &serde_json::json!({"logo":channel.logo,"catchup":channel.catchup}),
        )?);
        let metadata = self.vault.seal(&format!("metadata:{id}"), &metadata)?;
        let id = id.to_string();
        let epg_name = if channel.tvg_name.is_empty() {
            normalized(&channel.name)
        } else {
            normalized(&channel.tvg_name)
        };
        self.connection.execute(
            "INSERT OR IGNORE INTO channel_entries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                self.source_id,
                self.generation,
                id,
                channel.name,
                normalized(&channel.name),
                channel.tvg_id,
                epg_name,
                channel.country,
                channel.shift_seconds,
                metadata
            ],
        )?;
        self.connection.execute(
            "INSERT OR IGNORE INTO stream_variants VALUES(?1,?2,?3,?4,?5)",
            params![self.generation, id, variant, secret, self.ordinal],
        )?;
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM stream_variants WHERE generation=?1 AND channel_id=?2",
            params![self.generation, id],
            |r| r.get(0),
        )?;
        if count > 32 {
            return Err(Error::Invalid(
                "A channel has more than 32 stream variants; use unique names or TVG IDs.",
            ));
        }
        let groups = if channel.groups.is_empty() {
            vec!["Ungrouped".to_owned()]
        } else {
            channel.groups
        };
        for group in groups {
            if group.len() > 512 {
                return Err(Error::Limit);
            }
            self.connection.execute(
                "INSERT OR IGNORE INTO channel_groups VALUES(?1,?2,?3)",
                params![self.generation, id, group.trim()],
            )?;
        }
        for code in channel
            .country
            .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        {
            let code = code.trim().to_ascii_uppercase();
            if code.len() == 2 && code.bytes().all(|b| b.is_ascii_alphabetic()) {
                self.connection.execute(
                    "INSERT OR IGNORE INTO channel_countries VALUES(?1,?2,?3)",
                    params![self.generation, id, code],
                )?;
            }
        }
        self.advance()
    }
    pub fn epg_channel(&mut self, feed: &str, priority: usize, channel: EpgChannel) -> Result<()> {
        let names = if channel.names.is_empty() {
            vec![String::new()]
        } else {
            channel.names
        };
        for name in names {
            self.connection.execute(
                "INSERT OR IGNORE INTO epg_channels VALUES(?1,?2,?3,?4,?5)",
                params![
                    self.generation,
                    feed,
                    channel.id,
                    normalized(&name),
                    priority as i64
                ],
            )?;
        }
        self.advance()
    }
    pub fn programme(&mut self, feed: &str, programme: EpgProgramme, now: i64) -> Result<()> {
        if programme.end > now - 86400 && programme.start < now + 14 * 86400 {
            self.connection.execute(
                "INSERT OR IGNORE INTO programmes VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    self.generation,
                    feed,
                    programme.channel,
                    programme.start,
                    programme.end,
                    programme.title,
                    programme.description
                ],
            )?;
            self.advance()?;
        }
        Ok(())
    }
    pub fn activate(mut self) -> Result<()> {
        match_epg(&self.connection, &self.generation)?;
        self.connection.execute(
            "UPDATE sources SET active_generation=?1,updated_at=?2 WHERE id=?3",
            params![
                self.generation,
                chrono::Utc::now().timestamp(),
                self.source_id
            ],
        )?;
        self.connection.execute(
            "UPDATE generations SET state='active' WHERE id=?1",
            [&self.generation],
        )?;
        // Delete old rows after the new reference is changed, inside the same transaction.
        self.connection.execute(
            "DELETE FROM generations WHERE source_id=?1 AND id<>?2",
            params![self.source_id, self.generation],
        )?;
        bump_revision(&self.connection)?;
        self.connection.execute_batch("COMMIT")?;
        self.activated = true;
        Ok(())
    }
}
impl Drop for Import<'_> {
    fn drop(&mut self) {
        if !self.activated {
            let _ = self.connection.execute_batch("ROLLBACK");
            let _ = self.connection.execute(
                "DELETE FROM generations WHERE id=?1 AND state='staging'",
                [&self.generation],
            );
        }
    }
}
fn match_epg(c: &Connection, generation: &str) -> Result<()> {
    c.execute("DELETE FROM epg_links WHERE generation=?1", [generation])?;
    c.execute(
        "INSERT OR IGNORE INTO epg_links SELECT c.generation,c.id,o.feed_id,o.epg_id,'manual'
        FROM channel_entries c JOIN epg_overrides o ON o.channel_id=c.id
        JOIN epg_channels e ON e.generation=c.generation AND e.feed_id=o.feed_id AND e.id=o.epg_id
        WHERE c.generation=?1",
        [generation],
    )?;
    c.execute(
        "INSERT OR IGNORE INTO epg_links SELECT c.generation,c.id,e.feed_id,e.id,'id'
        FROM channel_entries c JOIN epg_channels e ON e.generation=c.generation AND e.id=c.tvg_id
        WHERE c.generation=?1 AND c.tvg_id<>'' ORDER BY e.priority,e.feed_id,e.id",
        [generation],
    )?;
    c.execute("WITH candidates AS (
        SELECT DISTINCT c.id AS channel_id,e.feed_id,e.id AS epg_id FROM channel_entries c
        JOIN epg_channels e ON e.generation=c.generation AND e.name<>'' AND (e.name=c.epg_name OR e.name=c.normalized_name)
        WHERE c.generation=?1
    ), unique_matches AS (
        SELECT channel_id,MIN(feed_id) AS feed_id,MIN(epg_id) AS epg_id FROM candidates GROUP BY channel_id HAVING COUNT(*)=1
    ) INSERT OR IGNORE INTO epg_links SELECT ?1,channel_id,feed_id,epg_id,'name' FROM unique_matches",[generation])?;
    Ok(())
}
