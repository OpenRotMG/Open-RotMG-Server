#!/usr/bin/env python3
"""Synthetic local-save CLI/legacy-auth journey; optional private native client continuation."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--client', type=Path, required=True)
parser.add_argument('--port', type=int, required=True)
parser.add_argument('--native', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
out = Path(os.environ['QA_RESULTS_DIR'])
binary = out / 'openrotmg'
os.link(args.binary.resolve(), binary)
data = root / 'data'
source, state = out / 'source', out / 'local'
state.mkdir(mode=0o700)
report = dict(binary=str(binary), sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), assertions=[])
password = b'independent-local-password\n'
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
base = f'http://127.0.0.1:{args.port}'


def command(*values, secret=None, success=True):
    result = subprocess.run([str(binary), *map(str, values)], input=secret, capture_output=True, cwd=out)
    assert (result.returncode == 0) == success, result.stderr.decode()
    if success:
        return json.loads(result.stdout)
    assert password.strip() not in result.stderr
    return result.stderr.decode()


def save(*values, **kw):
    return command('--local-save', *values, **kw)


def rows(path):
    with sqlite3.connect(f'file:{path}?mode=ro', uri=True) as db:
        return list(db.iterdump())


command('--backup-fixture', source, data)
before = rows(source / 'game.db')
portable = out / 'player.json'
save('export', source, data, 1, portable.name)
assert rows(source / 'game.db') == before
preview = save('preview', state, data, portable)
assert preview['login'] == 'player-1@local.invalid' and not preview['replaced']
result = save('import', state, data, portable, out / 'unused-first-backup.db', secret=password)
assert result['login'] == 'player-1@local.invalid'
report['import'] = result
save('verify-account', portable, data)
# Cancellation/EOF or failed pre-backup must not touch a populated destination.
before = rows(state / 'game.db')
save('replace', state, data, portable, out / 'cancelled.db', secret=b'', success=False)
assert rows(state / 'game.db') == before
save('replace', state, data, portable, out / 'absent' / 'failed.db', secret=password, success=False)
assert rows(state / 'game.db') == before
report['assertions'].extend(['source-read-only', 'independent-login', 'cancelled-import-unchanged', 'failed-backup-unchanged'])
config = out / 'server.json'
config.write_text(json.dumps(dict(bind=f'127.0.0.1:{args.port}', origin=base, data=str(data), client=str(args.client.resolve()), state=str(state), max_players=85, behind_proxy=False, shore_bots=False, bot_llm=False, realm_seed=42)))


def account(secret):
    with http.open(base + '/legacy-api/char/list', urllib.parse.urlencode(dict(guid=result['login'], password=secret)).encode(), timeout=15) as response:
        return ET.fromstring(response.read())


for phase in ['before', 'reopened']:
    with (out / f'{phase}.log').open('w') as log:
        child = subprocess.Popen([str(binary), '--desktop', '--config', str(config)], stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 600
            while True:
                assert child.poll() is None, f'server exited {child.returncode}'
                try:
                    with http.open(base + '/health', timeout=2) as response:
                        health = json.load(response)
                    assert health['process'] == child.pid
                    break
                except (urllib.error.URLError, TimeoutError):
                    assert time.monotonic() < deadline
                    time.sleep(.2)
            document = account(password.decode().strip())
            assert document.tag == 'Chars' and len(document.findall('Char')) == 2
            assert account('backup-fixture-password').tag == 'Error'
            if args.native:
                env = dict(os.environ, ACCOUNT_SAVE_ORIGIN=base, ACCOUNT_SAVE_LOGIN=result['login'], ACCOUNT_SAVE_PHASE=phase)
                subprocess.run(['node', str(root / 'examples/testing/account-save-native.mjs')], input=password, env=env, check=True)
            child.stdin.write(b'stop\n'); child.stdin.flush()
            assert child.wait(timeout=30) == 0
            report['assertions'].append(phase + '-legacy-auth-and-clean-shutdown')
        finally:
            if child.poll() is None:
                child.stdin.write(b'stop\n'); child.stdin.flush()
                try:
                    assert child.wait(timeout=30) == 0
                except BaseException:
                    child.kill(); child.wait()
                    raise RuntimeError('forced server cleanup is not acceptance')
backup = out / 'manual-0.db'
for index in range(8):
    save('snapshot', state, data, out / f'manual-{index}.db')
assert len(list(out.glob('manual-*.db'))) == 8
save('verify-snapshot', backup, data)
old = rows(state / 'game.db')
save('replace', state, data, portable, out / 'pre-replace.db', secret=b'replacement-password\n')
assert rows(out / 'pre-replace.db') == old
save('restore', state, data, backup, out / 'pre-restore.db')
assert rows(state / 'game.db') == rows(backup)
report['assertions'].extend(['consistent-pre-replace-backup', 'full-local-restore'])
report['native'] = args.native
report['packageIntegration'] = 'NOT tested: candidate server CLI/native core only; frozen Phase 1 package runtime must be paired by its owner'
(out / 'roundtrip-result.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
