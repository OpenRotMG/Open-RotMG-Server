#!/usr/bin/env python3
import argparse
from contextlib import closing
import datetime
import gzip
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess

LEGACY_GAME = {
    'accounts': ('id', 'guid', 'name'),
    'characters': ('id', 'account', 'stats', 'items', 'revision', 'alive'),
    'deaths': ('character', 'snapshot', 'killed_by', 'died_at'),
}
GAME = {**LEGACY_GAME, 'vaults': ('account', 'items', 'revision')}
SHORE_GAME = {**GAME, 'shore_bots': ('name', 'account')}
HISCORES_GAME = {**SHORE_GAME, 'competitors': ('account',), 'collections': ('account', 'item', 'found_at'), 'discord_links': ('account', 'discord_id')}
ACTIONS_GAME = {**HISCORES_GAME, 'account_fame': ('account', 'balance', 'earned'), 'discord_recovery': ('account', 'enabled_at', 'last_issued_at')}
SOCIAL_GAME = {**ACTIONS_GAME, 'account_lists': ('account', 'list_id', 'target')}
UPDATES_GAME = {**SOCIAL_GAME, 'update_feed': ('id', 'snapshot', 'synced_at')}
GOLD_GAME = {**UPDATES_GAME, 'accounts': (*GAME['accounts'], 'credits')}
GUILDS_GAME = {**GOLD_GAME, 'guilds': ('id', 'name', 'level', 'fame', 'earned', 'board'), 'guild_members': ('account', 'guild', 'rank', 'fame')}
REWARDS_GAME = {**GUILDS_GAME, 'gold_rewards': ('account', 'reward', 'awarded_at', 'notified_at')}
SLOTS_GAME = {**REWARDS_GAME, 'accounts': (*GOLD_GAME['accounts'], 'character_slots'), 'character_slot_purchases': ('account', 'timer', 'slots')}
GAME_SCHEMAS = {1: LEGACY_GAME, 2: GAME, 3: SHORE_GAME, 4: HISCORES_GAME, 5: ACTIONS_GAME, 6: SOCIAL_GAME, 7: UPDATES_GAME, 8: GOLD_GAME, 9: GUILDS_GAME, 10: REWARDS_GAME, 11: REWARDS_GAME, 12: SLOTS_GAME}
AUTH_GAME = {**GAME, 'credentials': ('account', 'email', 'password_hash')}
AUTH_SHORE_GAME = {**SHORE_GAME, 'credentials': AUTH_GAME['credentials']}
AUTH_HISCORES_GAME = {**HISCORES_GAME, 'credentials': AUTH_GAME['credentials'], 'discord_codes': ('account', 'hash', 'issued_at', 'expires_at')}
AUTH_ACTIONS_GAME = {**ACTIONS_GAME, 'credentials': AUTH_GAME['credentials'], **{t: AUTH_HISCORES_GAME['discord_codes'] for t in ['discord_codes', 'recovery_codes', 'password_resets']}}
AUTH_SOCIAL_GAME = {**AUTH_ACTIONS_GAME, 'account_lists': SOCIAL_GAME['account_lists']}
AUTH_UPDATES_GAME = {**AUTH_SOCIAL_GAME, 'update_feed': UPDATES_GAME['update_feed']}
# Loading a restore with the release binary recreates its empty credential tables; the deploy helper and release contract share this.
AUTH_GOLD_GAME = {**AUTH_UPDATES_GAME, 'accounts': GOLD_GAME['accounts']}
AUTH_GUILDS_GAME = {**AUTH_GOLD_GAME, 'guilds': GUILDS_GAME['guilds'], 'guild_members': GUILDS_GAME['guild_members']}
AUTH_REWARDS_GAME = {**AUTH_GUILDS_GAME, 'gold_rewards': REWARDS_GAME['gold_rewards']}
RELOADED_GAME = {**AUTH_REWARDS_GAME, 'accounts': SLOTS_GAME['accounts'], 'character_slot_purchases': SLOTS_GAME['character_slot_purchases']}
DISCORD = {'levels': ('guild_id', 'user_id', 'username', 'xp', 'last_xp_at', 'last_message_id')}


def characters(values):
    binary = os.environ.get('OPENROTMG_BACKUP_BINARY')
    if not binary:
        raise RuntimeError('OPENROTMG_BACKUP_BINARY must name the matching release binary')
    result = subprocess.run([binary, '--backup-validate'], input=json.dumps(values),
                            text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise ValueError('Release character validation failed: ' + result.stderr.strip())
    if json.loads(result.stdout) != {'validated_characters': len(values)}:
        raise ValueError('Release character validator returned an invalid receipt')


def update_snapshot(raw):
    binary = os.environ.get('OPENROTMG_BACKUP_BINARY')
    if not binary:
        raise RuntimeError('OPENROTMG_BACKUP_BINARY must name the matching release binary')
    result = subprocess.run([binary, '--backup-validate-updates'], input=raw,
                            text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise ValueError('Release update validation failed: ' + result.stderr.strip())
    if json.loads(result.stdout) != {'validated_updates': True}:
        raise ValueError('Release update validator returned an invalid receipt')


def snapshot(path, schema, *, include_schema=False):
    with closing(sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)) as db, db:
        db.execute('BEGIN')
        if db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok':
            raise ValueError('Source SQLite integrity failed')
        if db.execute('PRAGMA foreign_key_check').fetchall():
            raise ValueError('Source SQLite foreign keys failed')
        tables = {r[0] for r in db.execute("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")}
        schemas = schema if isinstance(schema, list) else [schema]
        matched = [candidate for candidate in schemas if tables == set(candidate) and all(
            tuple(r[1] for r in db.execute('PRAGMA table_info(' + table + ')')) == columns
            for table, columns in candidate.items())]
        if not matched or any(candidate != matched[0] for candidate in matched):
            raise ValueError('Unsupported database tables or columns; update and test the export schema before backing up')
        schema = matched[0]
        result = {}
        for table, columns in schema.items():
            if table in {'credentials', 'discord_codes', 'recovery_codes', 'password_resets'}:
                continue
            selected = tuple(c for c in columns if c != 'guid')
            rows = db.execute('SELECT ' + ','.join(selected) + ' FROM ' + table + ' ORDER BY ' + ','.join(selected[:2])).fetchall()
            result[table] = [dict(zip(selected, row)) for row in rows]
        return (result, schema, db.execute('PRAGMA user_version').fetchone()[0]) if include_schema else result


def validate_guilds(game, accounts):
    guilds, names = {}, set()
    for row in game.get('guilds', []):
        if (type(row['id']) is not int or not 0 < row['id'] <= 2147483647
                or type(row['name']) is not str or not re.fullmatch('[A-Za-z ]{1,20}', row['name'])
                or row['name'] != ' '.join(row['name'].split())
                or type(row['level']) is not int or not 1 <= row['level'] <= 4
                or any(type(row[k]) is not int for k in ['fame', 'earned'])
                or not 0 <= row['fame'] <= row['earned'] <= 2147483647
                or type(row['board']) is not str or len(row['board'].encode('utf-16-le')) // 2 > 4096
                or any((ord(c) < 32 or 127 <= ord(c) <= 159) and c not in '\r\n\t' for c in row['board'])):
            raise ValueError('Invalid guild record')
        if row['id'] in guilds or row['name'].lower() in names:
            raise ValueError('Duplicate guild identity')
        guilds[row['id']] = row
        names.add(row['name'].lower())
    members, counts, founders, contributions = set(), {}, {}, {}
    for row in game.get('guild_members', []):
        if (any(type(row[k]) is not int for k in ['account', 'guild', 'rank', 'fame'])
                or row['account'] not in accounts or row['guild'] not in guilds
                or row['rank'] not in [0, 10, 20, 30, 40] or not 0 <= row['fame'] <= 2147483647):
            raise ValueError('Invalid guild member')
        if row['account'] in members:
            raise ValueError('Account belongs to multiple guilds')
        members.add(row['account'])
        g = row['guild']
        counts[g] = counts.get(g, 0) + 1
        founders[g] = founders.get(g, 0) + (row['rank'] == 40)
        contributions[g] = contributions.get(g, 0) + row['fame']
    if any(not 1 <= counts.get(g, 0) <= 50 or founders[g] > 1 or contributions[g] > guilds[g]['earned'] for g in guilds):
        raise ValueError('Invalid guild roster, founder count or contributions')


def validate_slots(game):
    capacities, purchases, timers, living = {}, {}, set(), {}
    for row in game['accounts']:
        account, capacity = row['id'], row['character_slots']
        if (type(account) is not int or not 0 < account <= 2147483647 or account in capacities
                or type(capacity) is not int or not 1 <= capacity <= 14):
            raise ValueError('Invalid character slot capacity')
        capacities[account], purchases[account], living[account] = capacity, [], 0
    for row in game['character_slot_purchases']:
        account, timer, slots = row['account'], row['timer'], row['slots']
        if (type(account) is not int or account not in capacities
                or type(timer) is not int or not -2147483648 <= timer <= 2147483647
                or type(slots) is not int or not 2 <= slots <= 14 or (account, timer) in timers):
            raise ValueError('Invalid character slot purchase')
        timers.add((account, timer))
        purchases[account].append(slots)
    for row in game['characters']:
        if (type(row['account']) is not int or row['account'] not in capacities
                or type(row['alive']) is not int or row['alive'] not in (0, 1)):
            raise ValueError('Invalid character slot owner or living state')
        living[row['account']] += row['alive']
    for account, capacity in capacities.items():
        if sorted(purchases[account]) != list(range(2, capacity + 1)) or living[account] > capacity:
            raise ValueError('Inconsistent character slot purchase history or living capacity')


def validate(bundle):
    if set(bundle) != {'schema', 'created_at', 'revisions', 'game', 'discord'} or type(bundle['schema']) is not int or bundle['schema'] not in GAME_SCHEMAS:
        raise ValueError('Unsupported backup envelope')
    datetime.datetime.strptime(bundle['created_at'], '%Y-%m-%dT%H:%M:%SZ')
    if set(bundle['revisions']) not in ({'server', 'discord_bot'}, {'server', 'discord_bot', 'validator'}) or any(not re.fullmatch('[0-9a-f]{40}', v) for v in bundle['revisions'].values()):
        raise ValueError('Invalid source revisions')
    for name, schema in [('game', GAME_SCHEMAS[bundle['schema']]), ('discord', DISCORD)]:
        data = bundle[name]
        if set(data) != set(schema):
            raise ValueError('Unsupported backup tables: ' + name)
        for table, columns in schema.items():
            allowed = set(columns) - {'guid'}
            if not isinstance(data[table], list) or any(not isinstance(r, dict) or set(r) != allowed for r in data[table]):
                raise ValueError('Unsupported backup fields: ' + table)
    characters([json.loads(row['stats']) for row in bundle['game']['characters']]
               + [json.loads(row['snapshot']) for row in bundle['game']['deaths']])
    for row in bundle['game']['characters']:
        items = json.loads(row['items'])
        if not isinstance(items, list) or len(items) != 12 or any(type(i) is not int for i in items):
            raise ValueError('Invalid inventory')
    for row in bundle['game'].get('vaults', []):
        items = json.loads(row['items'])
        if type(row['account']) is not int or row['account'] <= 0 or type(row['revision']) is not int or row['revision'] < 0:
            raise ValueError('Invalid vault account/revision')
        if not isinstance(items, list) or len(items) != 8 or any(type(i) is not int or not -1 <= i <= 65535 for i in items):
            raise ValueError('Invalid eight-slot vault inventory')
    bots = bundle['game'].get('shore_bots', [])
    accounts = {row['id'] for row in bundle['game']['accounts']}
    for row in bundle['game']['accounts']:
        if bundle['schema'] >= 8 and (type(row['credits']) is not int or not 0 <= row['credits'] <= 2147483647):
            raise ValueError('Invalid account gold balance')
    for row in bots:
        if not isinstance(row['name'], str) or not row['name'] or type(row['account']) is not int or row['account'] <= 0 or row['account'] not in accounts:
            raise ValueError('Invalid shore bot name/account')
    if len({row['name'] for row in bots}) != len(bots) or len({row['account'] for row in bots}) != len(bots):
        raise ValueError('Duplicate shore bot name/account')
    for table in ['competitors', 'collections', 'discord_links', 'account_fame', 'discord_recovery']:
        rows = bundle['game'].get(table, [])
        for row in rows:
            if type(row['account']) is not int or row['account'] <= 0 or row['account'] not in accounts:
                raise ValueError('Invalid hiscores owner: ' + table)
            if table == 'account_fame' and (type(row['balance']) is not int or type(row['earned']) is not int or not 0 <= row['balance'] <= row['earned'] <= 2147483647):
                raise ValueError('Invalid account fame wallet')
            if table == 'discord_recovery' and (row['account'] not in {r['account'] for r in bundle['game']['discord_links']} or type(row['enabled_at']) is not int or row['enabled_at'] <= 0 or type(row['last_issued_at']) is not int or row['last_issued_at'] < 0):
                raise ValueError('Invalid recovery permission')
            if table == 'collections' and (type(row['item']) is not int or not 0 <= row['item'] <= 65535 or type(row['found_at']) is not int or row['found_at'] <= 0):
                raise ValueError('Invalid collection record')
            if table == 'discord_links' and (not isinstance(row['discord_id'], str) or not re.fullmatch(r'[0-9]{17,20}', row['discord_id'])):
                raise ValueError('Invalid linked Discord identity')
        identities = [(row['account'], row['item']) if table == 'collections' else row['account'] for row in rows]
        if len(set(identities)) != len(rows) or (table == 'discord_links' and len({r['discord_id'] for r in rows}) != len(rows)):
            raise ValueError('Duplicate hiscores identity: ' + table)
    rewards = bundle['game'].get('gold_rewards', [])
    reward_types = ('discord_link', 'collection_top_half') + (('first_bug_report',) if bundle['schema'] >= 11 else ())
    for row in rewards:
        if type(row['account']) is not int or row['account'] not in accounts or row['reward'] not in reward_types or type(row['awarded_at']) is not int or row['awarded_at'] <= 0 or (row['notified_at'] is not None and (type(row['notified_at']) is not int or row['notified_at'] <= 0)):
            raise ValueError('Invalid gold reward')
    if len({(row['account'], row['reward']) for row in rewards}) != len(rewards):
        raise ValueError('Duplicate gold reward')
    lists = {}
    for row in bundle['game'].get('account_lists', []):
        if any(type(row[key]) is not int or not 0 < row[key] <= 2147483647 or row[key] not in accounts for key in ['account', 'target']) or type(row['list_id']) is not int or row['list_id'] not in (0, 1) or row['account'] == row['target']:
            raise ValueError('Invalid account list entry')
        targets = lists.setdefault((row['account'], row['list_id']), set())
        if row['target'] in targets:
            raise ValueError('Duplicate account list entry')
        targets.add(row['target'])
        if len(targets) > 32767:
            raise ValueError('Account list exceeds native count limit')
    updates = bundle['game'].get('update_feed', [])
    if len(updates) > 1:
        raise ValueError('Invalid update feed row count')
    for row in updates:
        if type(row['id']) is not int or row['id'] != 1 or type(row['synced_at']) is not int or row['synced_at'] <= 0 or not isinstance(row['snapshot'], str):
            raise ValueError('Invalid update feed record')
        update_snapshot(row['snapshot'])
    if bundle['schema'] >= 5 and {r['account'] for r in bundle['game']['account_fame']} != accounts:
        raise ValueError('Every account requires a fame wallet')
    validate_guilds(bundle['game'], accounts)
    if bundle['schema'] >= 12:
        validate_slots(bundle['game'])
    return bundle


def export(game, discord, output, server_revision, bot_revision, validator_revision):
    game_data, matched, stored_version = snapshot(game, [*GAME_SCHEMAS.values(), AUTH_GAME, AUTH_SHORE_GAME, AUTH_HISCORES_GAME, AUTH_ACTIONS_GAME, AUTH_SOCIAL_GAME, AUTH_UPDATES_GAME, AUTH_GOLD_GAME, AUTH_GUILDS_GAME, AUTH_REWARDS_GAME, RELOADED_GAME], include_schema=True)
    versions = [v for v, schema in GAME_SCHEMAS.items() if schema == {k: c for k, c in matched.items() if k in game_data}]
    if stored_version > max(GAME_SCHEMAS) or ((stored_version == 12) != (12 in versions)):
        raise ValueError('Unsupported database schema version')
    if len(versions) > 1 and stored_version not in versions:
        raise ValueError('Unsupported gold reward schema version')
    version = stored_version if len(versions) > 1 else versions[0]
    bundle = validate({
        'schema': version, 'created_at': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
        'revisions': {'server': Path(server_revision).read_text().strip(), 'discord_bot': Path(bot_revision).read_text().strip(),
                      'validator': Path(validator_revision).read_text().strip()},
        'game': game_data, 'discord': snapshot(discord, DISCORD),
    })
    data = gzip.compress(json.dumps(bundle, sort_keys=True, separators=(',', ':')).encode(), mtime=0)
    output = Path(output)
    temporary = output.with_name(output.name + '.new')
    with temporary.open('xb') as file:
        file.write(data)
    temporary.replace(output)
    return bundle


def restore(archive, directory):
    bundle = validate(json.loads(gzip.decompress(Path(archive).read_bytes())))
    directory = Path(directory)
    directory.mkdir(mode=0o700)
    # NULL identities cannot authenticate; ownership must be reverified separately after a disaster.
    definitions = {
        'game': """CREATE TABLE accounts(id INTEGER PRIMARY KEY,guid TEXT UNIQUE,name TEXT NOT NULL);
CREATE TABLE characters(id INTEGER PRIMARY KEY,account INTEGER NOT NULL REFERENCES accounts(id),stats TEXT NOT NULL,items TEXT NOT NULL,revision INTEGER NOT NULL,alive INTEGER NOT NULL);
CREATE UNIQUE INDEX one_living_character ON characters(account) WHERE alive=1;
CREATE TABLE deaths(character INTEGER PRIMARY KEY REFERENCES characters(id),snapshot TEXT NOT NULL,killed_by TEXT NOT NULL,died_at INTEGER NOT NULL);""",
        'discord': """CREATE TABLE levels(guild_id TEXT NOT NULL,user_id TEXT NOT NULL,username TEXT NOT NULL,xp INTEGER NOT NULL CHECK(xp>=0 AND xp<=9007199254740991),last_xp_at INTEGER NOT NULL,last_message_id TEXT NOT NULL,PRIMARY KEY(guild_id,user_id)) STRICT;""",
    }
    if bundle['schema'] >= 2:
        definitions['game'] += '\nCREATE TABLE vaults(account INTEGER PRIMARY KEY REFERENCES accounts(id),items TEXT NOT NULL CHECK(json_array_length(items)=8),revision INTEGER NOT NULL DEFAULT 0);'
    if bundle['schema'] >= 3:
        definitions['game'] += '\nCREATE TABLE shore_bots(name TEXT PRIMARY KEY,account INTEGER NOT NULL UNIQUE REFERENCES accounts(id));'
    if bundle['schema'] >= 4:
        definitions['game'] += '''
CREATE TABLE competitors(account INTEGER PRIMARY KEY REFERENCES accounts(id));
CREATE TABLE collections(account INTEGER NOT NULL REFERENCES accounts(id),item INTEGER NOT NULL CHECK(item BETWEEN 0 AND 65535),found_at INTEGER NOT NULL,PRIMARY KEY(account,item));
CREATE TABLE discord_links(account INTEGER PRIMARY KEY REFERENCES accounts(id),discord_id TEXT NOT NULL UNIQUE);'''
    if bundle['schema'] >= 5:
        definitions['game'] += '''
CREATE TABLE account_fame(account INTEGER PRIMARY KEY REFERENCES accounts(id),balance INTEGER NOT NULL CHECK(balance BETWEEN 0 AND 2147483647),earned INTEGER NOT NULL CHECK(earned BETWEEN balance AND 2147483647));
CREATE TABLE discord_recovery(account INTEGER PRIMARY KEY REFERENCES discord_links(account),enabled_at INTEGER NOT NULL,last_issued_at INTEGER NOT NULL);'''
    if bundle['schema'] >= 6:
        definitions['game'] += '\nCREATE TABLE account_lists(account INTEGER NOT NULL REFERENCES accounts(id),list_id INTEGER NOT NULL CHECK(list_id IN (0,1)),target INTEGER NOT NULL REFERENCES accounts(id),PRIMARY KEY(account,list_id,target),CHECK(account<>target));'
    if bundle['schema'] >= 7:
        definitions['game'] += '\nCREATE TABLE update_feed(id INTEGER PRIMARY KEY CHECK(id=1),snapshot TEXT NOT NULL CHECK(json_valid(snapshot)),synced_at INTEGER NOT NULL);'
    if bundle['schema'] >= 8:
        definitions['game'] += '\nALTER TABLE accounts ADD COLUMN credits INTEGER NOT NULL DEFAULT 100 CHECK(credits BETWEEN 0 AND 2147483647);'
    if bundle['schema'] >= 9:
        definitions['game'] += '''
CREATE TABLE guilds(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK(length(name) BETWEEN 1 AND 20 AND name=trim(name) AND name NOT GLOB '*[^A-Za-z ]*'),level INTEGER NOT NULL DEFAULT 1 CHECK(level BETWEEN 1 AND 4),fame INTEGER NOT NULL DEFAULT 0 CHECK(fame BETWEEN 0 AND 2147483647),earned INTEGER NOT NULL DEFAULT 0 CHECK(earned BETWEEN fame AND 2147483647),board TEXT NOT NULL DEFAULT '');
CREATE TABLE guild_members(account INTEGER PRIMARY KEY REFERENCES accounts(id),guild INTEGER NOT NULL REFERENCES guilds(id),rank INTEGER NOT NULL CHECK(rank IN (0,10,20,30,40)),fame INTEGER NOT NULL DEFAULT 0 CHECK(fame BETWEEN 0 AND 2147483647));
CREATE UNIQUE INDEX one_guild_founder ON guild_members(guild) WHERE rank=40;
CREATE INDEX guild_roster ON guild_members(guild);
CREATE TRIGGER guild_capacity BEFORE INSERT ON guild_members WHEN (SELECT count(*) FROM guild_members WHERE guild=NEW.guild)>=50 BEGIN SELECT RAISE(ABORT,'Guild is full.'); END;'''
    if bundle['schema'] >= 10:
        rewards = "'discord_link','collection_top_half'" + (",'first_bug_report'" if bundle['schema'] >= 11 else '')
        definitions['game'] += f"\nCREATE TABLE gold_rewards(account INTEGER NOT NULL REFERENCES accounts(id),reward TEXT NOT NULL CHECK(reward IN ({rewards})),awarded_at INTEGER NOT NULL CHECK(awarded_at>0),notified_at INTEGER CHECK(notified_at>0),PRIMARY KEY(account,reward));"
    if bundle['schema'] >= 12:
        definitions['game'] += '''
ALTER TABLE accounts ADD COLUMN character_slots INTEGER NOT NULL DEFAULT 1 CHECK(typeof(character_slots)='integer' AND character_slots BETWEEN 1 AND 14);
CREATE TABLE character_slot_purchases(account INTEGER NOT NULL REFERENCES accounts(id),timer INTEGER NOT NULL CHECK(typeof(timer)='integer' AND timer BETWEEN -2147483648 AND 2147483647),slots INTEGER NOT NULL CHECK(typeof(slots)='integer' AND slots BETWEEN 2 AND 14),PRIMARY KEY(account,timer),UNIQUE(account,slots));
DROP INDEX one_living_character;
CREATE INDEX living_characters_by_account ON characters(account) WHERE alive=1;
CREATE TRIGGER character_slot_insert BEFORE INSERT ON characters WHEN NEW.alive=1 AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=NEW.account AND character_slots>(SELECT count(*) FROM characters WHERE account=NEW.account AND alive=1)) BEGIN SELECT RAISE(ABORT,'No free character slots.'); END;
CREATE TRIGGER character_slot_update BEFORE UPDATE OF account,alive ON characters WHEN NEW.alive=1 AND NOT EXISTS(SELECT 1 FROM accounts WHERE id=NEW.account AND character_slots>(SELECT count(*) FROM characters WHERE account=NEW.account AND alive=1 AND id<>OLD.id)) BEGIN SELECT RAISE(ABORT,'No free character slots.'); END;
CREATE TRIGGER character_slot_capacity BEFORE UPDATE OF character_slots ON accounts WHEN NEW.character_slots<(SELECT count(*) FROM characters WHERE account=NEW.id AND alive=1) BEGIN SELECT RAISE(ABORT,'Living characters exceed capacity.'); END;'''
    for name, filename in [('game', 'game.db'), ('discord', 'levels.sqlite')]:
        with closing(sqlite3.connect(':memory:')) as db, db:
            db.execute('PRAGMA foreign_keys=ON')
            db.executescript(definitions[name])
            if name == 'game' and bundle['schema'] >= 5:
                db.execute('PRAGMA user_version=' + str(bundle['schema']))
            for table, fields in (GAME_SCHEMAS[bundle['schema']] if name == 'game' else DISCORD).items():
                rows = bundle[name][table]
                columns = tuple(c for c in fields if c != 'guid')
                db.executemany('INSERT INTO ' + table + '(' + ','.join(columns) + ') VALUES(' + ','.join('?' for _ in columns) + ')', [tuple(row[c] for c in columns) for row in rows])
            db.commit()
            if db.execute('PRAGMA foreign_key_check').fetchall() or db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok':
                raise ValueError('Restored database integrity failed')
            path = directory / filename
            with path.open('xb'):
                pass
            with closing(sqlite3.connect(path)) as target:
                db.backup(target)
    for name, filename in [('game', 'game.db'), ('discord', 'levels.sqlite')]:
        schema = GAME_SCHEMAS[bundle['schema']] if name == 'game' else DISCORD
        if snapshot(directory / filename, schema) != bundle[name]:
            raise ValueError('Restored fields differ from uploaded archive: ' + name)
    return bundle


if __name__ == '__main__':
    os.umask(0o077)
    parser = argparse.ArgumentParser(description='Credential-free persisted-state export and offline restore')
    commands = parser.add_subparsers(dest='command', required=True)
    exp = commands.add_parser('export')
    for option in ['game', 'discord', 'output', 'server-revision', 'bot-revision', 'validator-revision']:
        exp.add_argument('--' + option, required=True)
    res = commands.add_parser('restore')
    res.add_argument('--archive', required=True)
    res.add_argument('--directory', required=True)
    args = vars(parser.parse_args())
    command = args.pop('command')
    bundle = export(**args) if command == 'export' else restore(**args)
    print(json.dumps({'event': command + '_complete', 'created_at': bundle['created_at'], 'counts': {name: {table: len(rows) for table, rows in bundle[name].items()} for name in ['game', 'discord']}}))
