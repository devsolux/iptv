CREATE TABLE IF NOT EXISTS app_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL) STRICT;
INSERT OR IGNORE INTO app_meta VALUES ('revision', 0);
CREATE TABLE IF NOT EXISTS sources (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, config BLOB NOT NULL,
  active_generation TEXT, updated_at INTEGER, has_epg INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS generations (
  id TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  state TEXT NOT NULL CHECK(state IN ('staging','active')), created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS channel_entries (
  source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
  generation TEXT NOT NULL REFERENCES generations(id) ON DELETE CASCADE,
  id TEXT NOT NULL, name TEXT NOT NULL, normalized_name TEXT NOT NULL,
  tvg_id TEXT NOT NULL, epg_name TEXT NOT NULL, country TEXT NOT NULL,
  shift_seconds INTEGER NOT NULL DEFAULT 0, metadata BLOB NOT NULL,
  PRIMARY KEY (generation, id)
) STRICT;
CREATE INDEX IF NOT EXISTS channel_order ON channel_entries(generation, name COLLATE NOCASE, id);
CREATE INDEX IF NOT EXISTS channel_identity ON channel_entries(id, generation);
CREATE TABLE IF NOT EXISTS stream_variants (
  generation TEXT NOT NULL, channel_id TEXT NOT NULL, id TEXT NOT NULL,
  secret BLOB NOT NULL, ordinal INTEGER NOT NULL,
  PRIMARY KEY(generation, channel_id, id),
  FOREIGN KEY(generation, channel_id) REFERENCES channel_entries(generation,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE IF NOT EXISTS channel_groups (
  generation TEXT NOT NULL, channel_id TEXT NOT NULL, name TEXT NOT NULL,
  PRIMARY KEY(generation, channel_id, name),
  FOREIGN KEY(generation, channel_id) REFERENCES channel_entries(generation,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX IF NOT EXISTS group_lookup ON channel_groups(name, generation, channel_id);
CREATE TABLE IF NOT EXISTS channel_countries (
  generation TEXT NOT NULL, channel_id TEXT NOT NULL, code TEXT NOT NULL,
  PRIMARY KEY(generation, channel_id, code),
  FOREIGN KEY(generation, channel_id) REFERENCES channel_entries(generation,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX IF NOT EXISTS country_lookup ON channel_countries(code, generation, channel_id);
CREATE TABLE IF NOT EXISTS favorites (channel_id TEXT PRIMARY KEY) STRICT;
CREATE TABLE IF NOT EXISTS epg_channels (
  generation TEXT NOT NULL REFERENCES generations(id) ON DELETE CASCADE,
  feed_id TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL,
  priority INTEGER NOT NULL,
  PRIMARY KEY(generation, feed_id, id, name)
) STRICT;
CREATE INDEX IF NOT EXISTS epg_name ON epg_channels(generation, name);
CREATE TABLE IF NOT EXISTS programmes (
  generation TEXT NOT NULL REFERENCES generations(id) ON DELETE CASCADE,
  feed_id TEXT NOT NULL, channel_id TEXT NOT NULL,
  start INTEGER NOT NULL, end INTEGER NOT NULL CHECK(end > start),
  title TEXT NOT NULL, description TEXT NOT NULL,
  PRIMARY KEY(generation, feed_id, channel_id, start, end, title)
) STRICT;
CREATE INDEX IF NOT EXISTS programme_window ON programmes(generation, feed_id, channel_id, start, end);
CREATE TABLE IF NOT EXISTS epg_overrides (
  channel_id TEXT PRIMARY KEY, feed_id TEXT NOT NULL, epg_id TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS epg_links (
  generation TEXT NOT NULL, channel_id TEXT NOT NULL, feed_id TEXT NOT NULL, epg_id TEXT NOT NULL,
  reason TEXT NOT NULL CHECK(reason IN ('manual','id','name')),
  PRIMARY KEY(generation, channel_id),
  FOREIGN KEY(generation, channel_id) REFERENCES channel_entries(generation,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version = 1;
