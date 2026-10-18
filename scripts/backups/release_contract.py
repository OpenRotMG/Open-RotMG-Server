from contextlib import closing
import gzip
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import backup


def check(binary, data, directory, revision):
    binary, data, directory = Path(binary).resolve(), Path(data).resolve(), Path(directory)
    directory.mkdir(mode=0o700)
    os.environ['OPENROTMG_BACKUP_BINARY'] = str(binary)
    written = directory / 'written'
    subprocess.run([str(binary), '--backup-fixture', str(written), str(data)], check=True)
    discord = directory / 'levels.sqlite'
    with closing(sqlite3.connect(discord)) as db, db:
        db.execute('CREATE TABLE levels(guild_id TEXT,user_id TEXT,username TEXT,xp INTEGER,last_xp_at INTEGER,last_message_id TEXT,PRIMARY KEY(guild_id,user_id))')
    version = directory / 'REVISION'
    version.write_text(revision + '\n')
    archive = directory / 'state.json.gz'
    bundle = backup.export(written / 'game.db', discord, archive, version, version, version)
    if b'BACKUP_FIXTURE_SECRET' in gzip.decompress(archive.read_bytes()):
        raise RuntimeError('Release backup exported a login identity')
    restored = directory / 'restored'
    backup.restore(archive, restored)
    subprocess.run([str(binary), '--backup-load', str(restored / 'game.db'), str(data)], check=True)
    if backup.snapshot(restored / 'game.db', backup.RELOADED_GAME) != bundle['game']:
        raise RuntimeError('Release loading changed restored gameplay records')
    result = subprocess.run([str(binary), '--backup-exercise-vault', str(restored / 'game.db'), str(data)], text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError('Release restored vault exercise failed: ' + result.stderr.strip())
    if json.loads(result.stdout) != {'exercised_vault_chests': 2, 'transfers': 1, 'withdrawals': 1, 'deposits': 1, 'items_conserved': True}:
        raise RuntimeError('Release restored vault exercise returned an invalid receipt')
    subprocess.run([str(binary), '--backup-load', str(restored / 'game.db'), str(data)], check=True)
    print(json.dumps({'event': 'release_backup_contract_passed', 'revision': revision}))


if __name__ == '__main__':
    check(*sys.argv[1:])
