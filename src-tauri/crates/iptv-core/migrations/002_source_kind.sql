ALTER TABLE sources ADD COLUMN kind TEXT NOT NULL DEFAULT 'm3u' CHECK(kind IN ('m3u','iptv-org'));
PRAGMA user_version = 2;
