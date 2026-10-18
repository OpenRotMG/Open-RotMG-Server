from contextlib import closing
from copy import deepcopy
import base64
import gzip
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

import backup


class PicturesBackup(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temp.name)
        cls.binary = os.environ['OPENROTMG_BACKUP_BINARY']
        cls.data = Path(__file__).resolve().parents[2] / 'data'
        subprocess.run([cls.binary, '--backup-fixture', str(cls.root / 'written'), str(cls.data)], check=True)
        with closing(sqlite3.connect(cls.root / 'levels.sqlite')) as db, db:
            db.execute('CREATE TABLE levels(guild_id TEXT,user_id TEXT,username TEXT,xp INTEGER,last_xp_at INTEGER,last_message_id TEXT,PRIMARY KEY(guild_id,user_id))')
        cls.revision = cls.root / 'REVISION'
        cls.revision.write_text('a' * 40)
        cls.bundle = backup.export(cls.root / 'written/game.db', cls.root / 'levels.sqlite',
                                 cls.root / 'state.gz', cls.revision, cls.revision, cls.revision)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_artwork_bytes_ownership_high_water_restore_and_usable_reload(self):
        bundle = self.bundle
        self.assertEqual(bundle['schema'], 15)
        self.assertEqual(len(bundle['game']['pictures']), 2)
        self.assertEqual(bundle['game']['picture_sequence'], [{'id': 1, 'next_id': 4}])
        with closing(sqlite3.connect(self.root / 'written/game.db')) as db:
            for row in bundle['game']['pictures']:
                original = db.execute('SELECT account,name,data_type,tags,png FROM pictures WHERE id=?', (row['id'],)).fetchone()
                self.assertEqual(original, (row['account'], row['name'], row['data_type'], row['tags'], base64.b64decode(row['png'])))
        restored = self.root / 'restored'
        backup.restore(self.root / 'state.gz', restored)
        subprocess.run([self.binary, '--backup-load', str(restored / 'game.db'), str(self.data)], check=True)
        self.assertEqual(backup.snapshot(restored / 'game.db', backup.RELOADED_GAME), bundle['game'])
        result = subprocess.run([self.binary, '--backup-exercise-pictures', str(restored / 'game.db'), str(self.data)], check=True, text=True, capture_output=True)
        self.assertEqual(json.loads(result.stdout), {'exercised_pictures': True, 'deleted_id': 4, 'next_id': 5})
        expected = deepcopy(bundle['game'])
        expected['picture_sequence'][0]['next_id'] = 5
        self.assertEqual(backup.snapshot(restored / 'game.db', backup.RELOADED_GAME), expected)

    def test_invalid_art_archive_is_rejected_before_restore_directory_creation(self):
        cases = []
        for field, values in [('id', [True, 0, -1, 2147483648]), ('account', [True, 99999]),
                              ('name', ['', '<html>', 'x' * 33]), ('data_type', [True, 0, 8]),
                              ('tags', ['UPPER', 'x' * 257]), ('png', ['AA==', '!', 'x' * 21849])]:
            for value in values:
                malformed = deepcopy(self.bundle)
                malformed['game']['pictures'][0][field] = value
                cases.append(malformed)
        for next_id in [True, 0, 1, 2, 2147483649]:
            malformed = deepcopy(self.bundle)
            malformed['game']['picture_sequence'][0]['next_id'] = next_id
            cases.append(malformed)
        for sequence in [[], [{'id': 2, 'next_id': 4}], self.bundle['game']['picture_sequence'] * 2]:
            malformed = deepcopy(self.bundle)
            malformed['game']['picture_sequence'] = sequence
            cases.append(malformed)
        for duplicate in ['id', 'owner-name']:
            malformed = deepcopy(self.bundle)
            if duplicate == 'id':
                malformed['game']['pictures'][1]['id'] = malformed['game']['pictures'][0]['id']
            else:
                malformed['game']['pictures'][1]['account'] = malformed['game']['pictures'][0]['account']
            cases.append(malformed)
        for i, malformed in enumerate(cases):
            with self.subTest(case=i):
                archive = self.root / f'malformed-{i}.gz'
                archive.write_bytes(gzip.compress(json.dumps(malformed).encode()))
                directory = self.root / f'invalid-{i}'
                with self.assertRaises(ValueError):
                    backup.restore(archive, directory)
                self.assertFalse(directory.exists())

    def test_noncanonical_art_schema_or_missing_sequence_fails_without_export(self):
        for i, mutation in enumerate([
            'DROP TABLE pictures', 'DROP TABLE picture_sequence', 'DELETE FROM picture_sequence',
            'UPDATE picture_sequence SET next_id=1',
            'ALTER TABLE pictures ADD COLUMN hidden TEXT GENERATED ALWAYS AS (name) VIRTUAL',
            'PRAGMA foreign_keys=OFF; UPDATE pictures SET account=99999',
            "UPDATE pictures SET png=x'00'",
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'png BLOB NOT NULL','png BLOB') WHERE name='pictures'; PRAGMA writable_schema=OFF;",
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'REFERENCES accounts(id)','') WHERE name='pictures'; PRAGMA writable_schema=OFF;",
            "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'next_id INTEGER NOT NULL','next_id INTEGER NOT NULL DEFAULT 1') WHERE name='picture_sequence'; PRAGMA writable_schema=OFF;",
        ]):
            with self.subTest(mutation=mutation):
                database = self.root / f'malformed-{i}.db'
                with closing(sqlite3.connect(self.root / 'written/game.db')) as source, closing(sqlite3.connect(database)) as db:
                    source.backup(db)
                    db.executescript(mutation)
                output = self.root / f'rejected-{i}.gz'
                before = database.read_bytes()
                with self.assertRaises(ValueError):
                    backup.export(database, self.root / 'levels.sqlite', output, self.revision, self.revision, self.revision)
                self.assertFalse(output.exists())
                self.assertEqual(database.read_bytes(), before)
