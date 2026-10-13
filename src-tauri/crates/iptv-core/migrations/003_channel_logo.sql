ALTER TABLE channel_entries ADD COLUMN has_logo INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version = 3;
