from contextlib import closing
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

import backup


class DeathSchema(unittest.TestCase):
    def test_export_rejects_noncanonical_current_metadata_without_writing(self):
        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            subprocess.run([os.environ['OPENROTMG_BACKUP_BINARY'], '--backup-fixture',
                            str(directory / 'written'), str(root / 'data')], check=True)
            with closing(sqlite3.connect(directory / 'levels.sqlite')) as db, db:
                db.execute('CREATE TABLE levels(guild_id TEXT,user_id TEXT,username TEXT,xp INTEGER,last_xp_at INTEGER,last_message_id TEXT,PRIMARY KEY(guild_id,user_id))')
            revision = directory / 'REVISION'
            revision.write_text('a' * 40)
            for case, (old, new) in enumerate([
                ('character INTEGER PRIMARY KEY', 'character INTEGER'),
                ('character INTEGER PRIMARY KEY', 'character TEXT PRIMARY KEY'),
                ('character INTEGER PRIMARY KEY', 'character INTEGER NOT NULL PRIMARY KEY'),
                ('character INTEGER PRIMARY KEY', 'character INTEGER PRIMARY KEY DEFAULT 0'),
                ('snapshot TEXT NOT NULL', 'snapshot BLOB NOT NULL'),
                ('snapshot TEXT NOT NULL', 'snapshot TEXT'),
                ('snapshot TEXT NOT NULL', "snapshot TEXT NOT NULL DEFAULT '{}'"),
                ('killed_by TEXT NOT NULL', 'killed_by BLOB NOT NULL'),
                ('killed_by TEXT NOT NULL', 'killed_by TEXT'),
                ('killed_by TEXT NOT NULL', "killed_by TEXT NOT NULL DEFAULT ''"),
                ('died_at INTEGER NOT NULL DEFAULT(unixepoch())', 'died_at TEXT NOT NULL DEFAULT(unixepoch())'),
                ('died_at INTEGER NOT NULL DEFAULT(unixepoch())', 'died_at INTEGER DEFAULT(unixepoch())'),
                ('died_at INTEGER NOT NULL DEFAULT(unixepoch())', 'died_at INTEGER NOT NULL'),
                ('died_at INTEGER NOT NULL DEFAULT(unixepoch())', 'died_at INTEGER NOT NULL DEFAULT 0'),
                ('equipment_fame INTEGER NOT NULL', 'equipment_fame TEXT NOT NULL'),
                ('equipment_fame INTEGER NOT NULL', 'equipment_fame INTEGER'),
                ('equipment_fame INTEGER NOT NULL', 'equipment_fame INTEGER NOT NULL DEFAULT 0'),
                ('equipment_fame INTEGER NOT NULL', 'equipment_fame INTEGER NOT NULL GENERATED ALWAYS AS (0) STORED'),
            ]):
                with self.subTest(metadata=new):
                    database = directory / 'malformed.db'
                    with closing(sqlite3.connect(directory / 'written/game.db')) as source, closing(sqlite3.connect(database)) as db:
                        source.backup(db)
                        sql = db.execute("SELECT sql FROM sqlite_schema WHERE name='deaths'").fetchone()[0]
                        changed = sql.replace(old, new)
                        if new == 'equipment_fame TEXT NOT NULL':
                            changed = changed.replace(" CHECK(typeof(equipment_fame)='integer' AND equipment_fame BETWEEN 0 AND 2147483647)", '')
                        self.assertNotEqual(sql, changed)
                        columns = 'character,snapshot,killed_by,died_at' + ('' if 'GENERATED' in new else ',equipment_fame')
                        db.executescript(f'ALTER TABLE deaths RENAME TO preserved_deaths; {changed}; INSERT INTO deaths({columns}) SELECT {columns} FROM preserved_deaths; DROP TABLE preserved_deaths; CREATE INDEX deaths_by_time ON deaths(died_at);')
                    before = database.read_bytes()
                    with self.assertRaisesRegex(ValueError, 'death.*metadata'):
                        backup.export(database, directory / 'levels.sqlite', directory / f'rejected-{case}.gz',
                                      revision, revision, revision)
                    self.assertEqual(database.read_bytes(), before)
                    self.assertFalse((directory / f'rejected-{case}.gz').exists())
