from contextlib import closing
from copy import deepcopy
import base64
import gzip
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import struct
import tempfile
import unittest
import zlib

import backup


def capacity_png():
    raw = bytes(264)
    # Empty deflate blocks exercise the full accepted file bound without changing native dimensions.
    compressed = b'\x78\x01\x00' + struct.pack('<HH', len(raw), 65535 - len(raw)) + raw
    compressed += b'\x00\x00\x00\xff\xff' * 3210 + b'\x03\x00' + struct.pack('>I', zlib.adler32(raw))

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))

    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 8, 8, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', compressed) + chunk(b'IEND', b''))


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
            'PRAGMA foreign_keys=OFF; UPDATE pictures SET account=99999 WHERE id=(SELECT min(id) FROM pictures)',
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

    def test_capacity_maximum_encoded_envelope_fits_unchanged_validator_bound(self):
        png = capacity_png()
        self.assertEqual(len(png), 16384)
        rows = [{'id': 2147483647 - i, 'account': 2147483647 - i // 128,
                 'name': f'{i:032d}', 'data_type': 2, 'tags': 'z' * 256,
                 'png': base64.b64encode(png).decode()} for i in range(1024)]
        encoded = json.dumps(rows).encode()
        self.assertEqual(len(encoded), 2 + 1024 * len(json.dumps(rows[0]).encode()) + 2 * 1023)
        self.assertLess(len(encoded), 32 * 1024 * 1024)
        result = subprocess.run([self.binary, '--backup-validate-pictures'], input=encoded, capture_output=True, check=True)
        self.assertEqual(json.loads(result.stdout), {'validated_pictures': 1024})
        print(json.dumps({'picture_capacity_envelope_bytes': len(encoded), 'png_bytes': len(png),
                          'account_rows': 128, 'total_rows': 1024, 'validator_bound_bytes': 32 * 1024 * 1024}))

    def test_full_capacity_backup_restore_and_matching_rejection_without_repair(self):
        database = self.root / 'capacity.db'
        with closing(sqlite3.connect(self.root / 'written/game.db')) as source, closing(sqlite3.connect(database)) as db, db:
            source.backup(db)
            owners = [row[0] for row in db.execute('SELECT id FROM accounts ORDER BY id')]
            for i in range(7):
                owner = 100 + i
                db.execute('INSERT INTO accounts(id,guid,name,credits,character_slots,vault_mask) VALUES(?,?,?,100,1,1)',
                           (owner, f'capacity-{i}', 'Cap' + chr(97 + i)))
                db.execute('INSERT INTO account_fame VALUES(?,0,0)', (owner,))
                db.execute('INSERT INTO vaults VALUES(?,0,?,0)', (owner, json.dumps([-1] * 8)))
                owners.append(owner)
            db.execute('DELETE FROM pictures')
            png = capacity_png()
            db.executemany('INSERT INTO pictures VALUES(?,?,?,?,?,?)',
                           [(i + 1, owners[i // 128], f'{i:032d}', 2, 'z' * 256, png) for i in range(1024)])
            db.execute('UPDATE picture_sequence SET next_id=1025')
        archive = self.root / 'capacity.gz'
        bundle = backup.export(database, self.root / 'levels.sqlite', archive, self.revision, self.revision, self.revision)
        restored = self.root / 'capacity-restored'
        backup.restore(archive, restored)
        subprocess.run([self.binary, '--backup-load', str(restored / 'game.db'), str(self.data)], check=True)
        self.assertEqual(backup.snapshot(restored / 'game.db', backup.RELOADED_GAME), bundle['game'])
        reexported = backup.export(restored / 'game.db', restored / 'levels.sqlite', self.root / 'capacity-reexport.gz',
                                   self.revision, self.revision, self.revision)
        self.assertEqual(reexported['game'], bundle['game'])
        for bound in ['account', 'global']:
            with self.subTest(bound=bound):
                malformed = deepcopy(bundle)
                if bound == 'account':
                    malformed['game']['pictures'][128]['account'] = owners[0]
                else:
                    malformed['game']['pictures'].append(dict(malformed['game']['pictures'][0], id=1025, account=owners[8]))
                    malformed['game']['picture_sequence'][0]['next_id'] = 1026
                bad = self.root / f'capacity-{bound}.gz'
                bad.write_bytes(gzip.compress(json.dumps(malformed).encode()))
                destination = self.root / f'capacity-reject-{bound}'
                with self.assertRaisesRegex(ValueError, f'{bound} capacity exceeded'):
                    backup.restore(bad, destination)
                self.assertFalse(destination.exists())
                rejected = subprocess.run([self.binary, '--backup-validate-pictures'], input=json.dumps(malformed['game']['pictures']),
                                          text=True, capture_output=True)
                self.assertNotEqual(rejected.returncode, 0)
                self.assertIn(f'{bound} capacity exceeded', rejected.stderr)
                damaged = self.root / f'capacity-{bound}.db'
                with closing(sqlite3.connect(restored / 'game.db')) as source, closing(sqlite3.connect(damaged)) as db, db:
                    source.backup(db)
                    if bound == 'account':
                        db.execute('UPDATE pictures SET account=? WHERE id=129', (owners[0],))
                    else:
                        db.execute('INSERT INTO pictures VALUES(1025,?,?,2,?,?)', (owners[8], 'Overflow', '', png))
                        db.execute('UPDATE picture_sequence SET next_id=1026')
                before = damaged.read_bytes()
                output = self.root / f'capacity-rejected-{bound}.gz'
                with self.assertRaisesRegex(ValueError, f'{bound} capacity exceeded'):
                    backup.export(damaged, restored / 'levels.sqlite', output, self.revision, self.revision, self.revision)
                self.assertEqual(damaged.read_bytes(), before)
                self.assertFalse(output.exists())
                state = backup.snapshot(damaged, backup.RELOADED_GAME)
                failed = subprocess.run([self.binary, '--backup-load', str(damaged), str(self.data)], text=True, capture_output=True)
                self.assertNotEqual(failed.returncode, 0)
                self.assertIn(f'{bound} capacity exceeded', failed.stderr)
                self.assertEqual(backup.snapshot(damaged, backup.RELOADED_GAME), state)
