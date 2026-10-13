"""Execute the shipped migration and actual Rust SQL against Python's SQLite.

These are database-invariant tests, not a substitute for Rust integration tests.
"""
from pathlib import Path
import re
import sqlite3
import unittest

ROOT = Path(__file__).resolve().parents[2]
CORE = ROOT / "src-tauri/crates/iptv-core"
RUST = (CORE / "src/db.rs").read_text()
# The matching statements contain no escaped Rust string characters. Tolerate rustfmt layouts.
MATCH_SQL = re.findall(
    r'c\.execute\(\s*"([\s\S]*?)",\s*\[generation\],?\s*\)\?', RUST.split('fn match_epg(')[1]
)
CHANNEL_FILTER = re.search(r'const CHANNEL_FILTER: &str = "([\s\S]*?)";', RUST).group(1).replace("\\\\", "\\")

class CatalogueTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("PRAGMA foreign_keys=ON")
        for name in ["001_initial.sql", "002_source_kind.sql"]:
            self.db.executescript((CORE / "migrations" / name).read_text())
        self.db.execute("INSERT INTO sources(id,name,config,active_generation,updated_at,has_epg) VALUES('source','My source',X'0102',NULL,NULL,1)")
        self.generation("old")
        self.channel("old", "stable", "News", "news.id")
        self.db.execute("UPDATE sources SET active_generation='old'")
        self.db.execute("UPDATE generations SET state='active' WHERE id='old'")
        self.db.commit()

    def tearDown(self):
        self.db.close()

    def generation(self, name):
        self.db.execute("INSERT INTO generations VALUES(?, 'source', 'staging', 0)", (name,))

    def channel(self, generation, key, name, tvg_id=""):
        self.db.execute("INSERT INTO channel_entries VALUES('source',?,?,?,?,?,'','DE',0,X'0102')", (generation,key,name,name.lower(),tvg_id))
        self.db.execute("INSERT INTO stream_variants VALUES(?,?,?,X'0102',0)", (generation,key,"variant"))

    def epg(self, generation, feed, key, name, priority=0):
        self.db.execute("INSERT INTO epg_channels VALUES(?,?,?,?,?)", (generation,feed,key,name,priority))

    def match(self, generation):
        self.assertEqual(len(MATCH_SQL), 4)
        for sql in MATCH_SQL:
            self.db.execute(sql, (generation,))

    def visible(self):
        return self.db.execute("SELECT c.id,c.name " + CHANNEL_FILTER + " ORDER BY c.name COLLATE NOCASE,c.id", ("", "%", None, None, False, None)).fetchall()

    def test_staged_rows_never_leak_into_active_catalogue(self):
        self.generation("next")
        self.channel("next", "new", "Replacement")
        self.assertEqual(self.visible(), [("stable", "News")])

    def test_failed_activation_rolls_back_to_old_generation(self):
        self.generation("next")
        self.channel("next", "stable", "Updated News")
        self.db.commit()
        self.db.execute("BEGIN")
        self.db.execute("UPDATE sources SET active_generation='next'")
        self.db.execute("DELETE FROM generations WHERE id='old'")
        self.db.rollback()
        self.assertEqual(self.visible(), [("stable", "News")])
        self.assertEqual(self.db.execute("PRAGMA foreign_key_check").fetchall(), [])

    def test_favorites_survive_generation_swap_and_cascade_deletes_only_old_variants(self):
        self.db.execute("INSERT INTO favorites VALUES('stable')")
        self.generation("next")
        self.channel("next", "stable", "News")
        self.db.execute("UPDATE sources SET active_generation='next'")
        self.db.execute("DELETE FROM generations WHERE id='old'")
        self.assertEqual(self.db.execute("SELECT * FROM favorites").fetchall(), [("stable",)])
        self.assertEqual(self.db.execute("SELECT generation FROM stream_variants").fetchall(), [("next",)])

    def test_one_channel_can_belong_to_multiple_countries_without_duplication(self):
        self.db.executemany("INSERT INTO channel_countries VALUES('old','stable',?)", [("DE",),("TR",)])
        for country in ["DE", "TR"]:
            self.assertEqual(self.db.execute("SELECT COUNT(*) " + CHANNEL_FILTER, ("", "%", None, None, False, country)).fetchone()[0], 1)
        self.assertEqual(len(self.visible()), 1)

    def test_exact_epg_id_uses_explicit_feed_priority(self):
        self.epg("old", "secondary", "news.id", "news", 1)
        self.epg("old", "primary", "news.id", "news", 0)
        self.match("old")
        self.assertEqual(self.db.execute("SELECT feed_id,reason FROM epg_links").fetchall(), [("primary","id")])

    def test_manual_override_precedes_exact_id(self):
        self.epg("old", "primary", "news.id", "news", 0)
        self.epg("old", "secondary", "override", "different", 1)
        self.db.execute("INSERT INTO epg_overrides VALUES('stable','secondary','override')")
        self.match("old")
        self.assertEqual(self.db.execute("SELECT feed_id,epg_id,reason FROM epg_links").fetchall(), [("secondary","override","manual")])

    def test_ambiguous_name_is_not_silently_matched(self):
        self.epg("old", "a", "id.a", "news")
        self.epg("old", "b", "id.b", "news")
        self.match("old")
        self.assertEqual(self.db.execute("SELECT * FROM epg_links").fetchall(), [])

    def test_unambiguous_name_match_is_source_generation_scoped(self):
        self.epg("old", "a", "external", "news")
        self.generation("next")
        self.epg("next", "b", "external.b", "news")
        self.match("old")
        self.assertEqual(self.db.execute("SELECT feed_id,reason FROM epg_links").fetchall(), [("a","name")])

    def test_duplicate_programmes_and_invalid_intervals_are_rejected(self):
        row = ("old","feed","news.id",100,200,"News","Description")
        self.db.execute("INSERT INTO programmes VALUES(?,?,?,?,?,?,?)", row)
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO programmes VALUES(?,?,?,?,?,?,?)", row)
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO programmes VALUES('old','feed','news.id',200,100,'Bad','')")

    def test_literal_search_does_not_treat_percent_as_wildcard(self):
        self.assertEqual(self.db.execute("SELECT COUNT(*) " + CHANNEL_FILTER, ("%", r"%\%%", None, None, False, None)).fetchone()[0], 0)

    def test_sqlite_schema_is_strict_and_valid(self):
        self.assertEqual(self.db.execute("PRAGMA user_version").fetchone()[0], 2)
        self.assertEqual(self.db.execute("PRAGMA integrity_check").fetchone()[0], "ok")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO sources(id,name,config) VALUES('bad','Bad','unencrypted text')")
        self.assertEqual(self.db.execute("SELECT kind FROM sources").fetchone()[0], "m3u")
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute("INSERT INTO sources(id,name,config,kind) VALUES('k','K',X'0102','other')")

    def test_version_one_profiles_migrate_in_place(self):
        db = sqlite3.connect(":memory:")
        db.executescript((CORE / "migrations/001_initial.sql").read_text())
        db.execute("INSERT INTO sources(id,name,config) VALUES('legacy','Legacy',X'0102')")
        self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], 1)
        db.executescript((CORE / "migrations/002_source_kind.sql").read_text())
        self.assertEqual(db.execute("PRAGMA user_version").fetchone()[0], 2)
        self.assertEqual(db.execute("SELECT kind FROM sources WHERE id='legacy'").fetchone()[0], "m3u")
        self.assertEqual(db.execute("PRAGMA foreign_key_check").fetchall(), [])
        db.close()

if __name__ == "__main__":
    unittest.main()
