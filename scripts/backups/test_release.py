from contextlib import closing
from copy import deepcopy
import gzip
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import backup
import unittest
from release_contract import check


class ReleaseContract(unittest.TestCase):
    def test_actual_writer_backup_restore_and_reload(self):
        root = Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as temporary:
            check(os.environ['OPENROTMG_BACKUP_BINARY'], root / 'data',
                  Path(temporary) / 'contract', 'a' * 40)
            with closing(sqlite3.connect(Path(temporary) / 'contract/written/game.db')) as db:
                value = json.loads(db.execute('SELECT stats FROM characters WHERE alive=1').fetchone()[0])
            self.assertEqual(value['dungeons'], [1, 2, 3])
            self.assertEqual(value['abysses'], 5)
            with closing(sqlite3.connect(Path(temporary) / 'contract/restored/game.db')) as db:
                self.assertEqual(db.execute('SELECT name,level,fame,earned,board FROM guilds').fetchone(),
                                 ('Backup Guild', 2, 2500, 12500, 'Welcome & prepare!\n日本語 🧙'))
                self.assertEqual(db.execute('SELECT rank,fame FROM guild_members').fetchone(), (40, 12500))
                rewards = db.execute('SELECT reward,notified_at FROM gold_rewards ORDER BY account,reward').fetchall()
                self.assertEqual([row[0] for row in rewards], ['discord_link', 'first_bug_report', 'discord_link'])
                self.assertIsNone(rewards[0][1])
                self.assertIsNone(rewards[1][1])
                self.assertGreater(rewards[2][1], 0)
                self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 11)
                db.execute('PRAGMA foreign_keys=ON')
                owner = db.execute('SELECT min(id) FROM accounts').fetchone()[0]
                for row in [(owner, 'discord_link', 123, None), (owner, 'invalid', 123, None), (owner, 'collection_top_half', 0, None), (owner, 'collection_top_half', 123, 0), (99999, 'discord_link', 123, None)]:
                    with self.subTest(row=row), self.assertRaises(sqlite3.IntegrityError):
                        db.execute('INSERT INTO gold_rewards VALUES(?,?,?,?)', row)
            bundle = json.loads(gzip.decompress((Path(temporary) / 'contract/state.json.gz').read_bytes()))
            legacy = deepcopy(bundle)
            legacy['schema'] = 10
            with self.assertRaisesRegex(ValueError, 'gold reward'):
                backup.validate(legacy)
            legacy['game']['gold_rewards'] = [r for r in legacy['game']['gold_rewards'] if r['reward'] != 'first_bug_report']
            archive = Path(temporary) / 'legacy.gz'
            archive.write_bytes(gzip.compress(json.dumps(legacy).encode()))
            restored = Path(temporary) / 'legacy'
            backup.restore(archive, restored)
            revision = Path(temporary) / 'REVISION'
            revision.write_text('a' * 40)
            reexported = backup.export(restored / 'game.db', restored / 'levels.sqlite', Path(temporary) / 'reexport.gz', revision, revision, revision)
            self.assertEqual(reexported['schema'], 10)
            self.assertEqual(reexported['game'], legacy['game'])
            for field, values in [('account', [True, 0, 99999]), ('reward', ['invalid', 100]), ('awarded_at', [True, 0, -1]), ('notified_at', [True, 0, -1])]:
                for invalid in values:
                    malformed = deepcopy(bundle)
                    malformed['game']['gold_rewards'][0][field] = invalid
                    with self.subTest(field=field, invalid=invalid), self.assertRaises(ValueError):
                        backup.validate(malformed)
            malformed = deepcopy(bundle)
            malformed['game']['gold_rewards'].append(malformed['game']['gold_rewards'][0])
            with self.assertRaisesRegex(ValueError, 'Duplicate gold reward'):
                backup.validate(malformed)
            backup.characters([value])
            for field in ['password_hash', 'token', 'email']:
                invalid = {**value, field: 'NEVER_LOG_OR_EXPORT'}
                with self.assertRaises(ValueError) as error:
                    backup.characters([invalid])
                self.assertNotIn('NEVER_LOG_OR_EXPORT', str(error.exception))
            for counters in [[1, 2], [1, -1, 3], [1, True, 3]]:
                invalid = deepcopy(value)
                invalid['dungeons'] = counters
                with self.assertRaises(ValueError):
                    backup.characters([invalid])
