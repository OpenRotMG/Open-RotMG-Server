"""Schema24 backup integration on a synthetic Store supplied by the Rust unit test."""
from contextlib import closing
from copy import deepcopy
import gzip
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys
from unittest.mock import patch
import backup


def assert_source_sql_refusals(path, discord, revision, parent):
    cases = [
        ('extra-index', 'CREATE INDEX extra_gold ON gold_rewards(awarded_at)'),
        ('direct-trigger', "CREATE TRIGGER extra_gold BEFORE INSERT ON gold_rewards BEGIN SELECT RAISE(ABORT,'guard trigger fired'); END;"),
        ('dependent-view', 'CREATE VIEW extra_gold AS SELECT account,reward FROM gold_rewards'),
        ('other-trigger', "CREATE TRIGGER extra_account BEFORE UPDATE ON accounts BEGIN SELECT count(*) FROM gold_rewards; SELECT RAISE(ABORT,'guard trigger fired'); END;"),
        ('altered-trigger', "DROP TRIGGER guild_capacity; CREATE TRIGGER guild_capacity BEFORE INSERT ON guild_members BEGIN SELECT RAISE(ABORT,'guard trigger fired'); END;"),
        ('child-fk', 'CREATE TABLE custom_gold_child(account INTEGER,reward TEXT,FOREIGN KEY(account,reward) REFERENCES GOLD_REWARDS(account,reward) ON DELETE CASCADE); INSERT INTO custom_gold_child SELECT account,reward FROM gold_rewards LIMIT 1;'),
    ]
    directory = Path(parent) / 'sql-source-refusals'
    directory.mkdir(mode=0o700)
    evidence = []
    for name, sql in cases:
        database, output = directory / (name + '.db'), directory / (name + '.gz')
        with closing(sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)) as source, closing(sqlite3.connect(database)) as db:
            source.backup(db)
            db.executescript(sql)
            db.commit()
            before = list(db.iterdump())
        try:
            backup.export(database, discord, output, revision, revision, revision)
        except ValueError as error:
            assert 'Unsupported' in str(error), str(error)
            message = str(error)
        else:
            raise AssertionError('Noncanonical SQL accepted: ' + name)
        with closing(sqlite3.connect(database)) as db:
            after = list(db.iterdump())
        assert after == before and not output.exists(), name
        evidence.append({'case': name, 'beforeSha256': hashlib.sha256('\n'.join(before).encode()).hexdigest(), 'afterSha256': hashlib.sha256('\n'.join(after).encode()).hexdigest(), 'error': message, 'noPublishedArtifact': True})
    if 'QA_RESULTS_DIR' in os.environ:
        (Path(os.environ['QA_RESULTS_DIR']) / 'python-gold-object-refusals.json').write_text(json.dumps(evidence, indent=2) + '\n')


def check(path):
    path = Path(path)
    with closing(sqlite3.connect(path)) as db:
        before = db.execute('SELECT * FROM discord_realm_close_tokens ORDER BY request_id').fetchall()
        assert len(before) == 2 and sum(row[-1] is None for row in before) == 1
        assert db.execute('SELECT count(*) FROM characters').fetchone()[0] == 0
        assert db.execute('SELECT sql FROM sqlite_schema WHERE name="gold_rewards"').fetchone()[0] == backup.gold_reward_sql(25, True)
    public = backup.snapshot(path, backup.RELOADED_GAME)
    assert 'discord_realm_close_tokens' not in public and 'credentials' not in public
    assert len(public['achievement_partners']) == len(public['achievement_dungeons']) == len(public['achievement_realm_credits']) == 1
    assert any(row['reward'] == 'achievement_trader' and row['awarded_at'] == 111 and row['notified_at'] == 222 for row in public['gold_rewards'])
    assert any(row['reward'] == 'achievement_ah_7uck' and row['awarded_at'] == 112 and row['notified_at'] == 223 for row in public['gold_rewards'])
    parent = path.parent
    discord = parent / 'backup-test-discord.sqlite'
    with closing(sqlite3.connect(discord)) as db:
        db.execute('CREATE TABLE levels(guild_id TEXT,user_id TEXT,username TEXT,xp INTEGER,last_xp_at INTEGER,last_message_id TEXT,PRIMARY KEY(guild_id,user_id))')
    revision = parent / 'backup-test-revision'
    revision.write_text('a' * 40)

    assert_source_sql_refusals(path, discord, revision, parent)

    # This fixture has no character/community payloads; isolate only their external CLI codecs.
    def empty_characters(values, **options):
        assert values == []

    def empty_community(game):
        assert game['bot_minds'] == game['trade_transactions'] == []

    def empty_pictures(game):
        assert game['pictures'] == [] and game['picture_sequence'] == [{'id': 1, 'next_id': 1}]

    with patch.object(backup, 'characters', empty_characters), patch.object(backup, 'validate_community', empty_community), patch.object(backup, 'validate_pictures', empty_pictures):
        archive = parent / 'backup-test-public.gz'
        bundle = backup.export(path, discord, archive, revision, revision, revision)
        assert bundle['schema'] == 25 and bundle['game'] == public
        assert b'discord_realm_close_tokens' not in gzip.decompress(archive.read_bytes())
        restored = parent / 'backup-test-restored'
        backup.restore(archive, restored)
        assert backup.snapshot(restored / 'game.db', backup.RESTORED_GAME) == public
        with closing(sqlite3.connect(restored / 'game.db')) as db:
            assert db.execute("SELECT count(*) FROM discord_realm_close_tokens").fetchone()[0] == 0
        reexported = backup.export(restored / 'game.db', restored / 'levels.sqlite', parent / 'backup-test-reexport.gz', revision, revision, revision)
        assert reexported['game'] == public
        for table, field, value in [('achievement_partners', 'peer', 999999), ('achievement_dungeons', 'dungeon', 'not-a-dungeon'), ('achievement_realm_credits', 'root', 0)]:
            invalid = deepcopy(bundle)
            invalid['game'][table][0][field] = value
            try:
                backup.validate(invalid)
            except ValueError:
                pass
            else:
                raise AssertionError('Malformed achievement progress accepted: ' + table)
    with closing(sqlite3.connect(path)) as db:
        assert db.execute('SELECT * FROM discord_realm_close_tokens ORDER BY request_id').fetchall() == before
        assert db.execute('PRAGMA user_version').fetchone()[0] == 25
    print('PASS actual schema25 export/restore/reexport preserves achievement history; source tokens unchanged and private tokens intentionally omitted; empty external codecs unit-isolated, no CLI')


if __name__ == '__main__':
    check(sys.argv[1])
