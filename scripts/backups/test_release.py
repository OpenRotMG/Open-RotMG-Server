from contextlib import closing
from copy import deepcopy
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
