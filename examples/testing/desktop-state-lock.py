#!/usr/bin/env python3
"""Same-state desktop exclusion and release using independently spawned consumers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.error
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--client', type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
out = Path(os.environ['QA_RESULTS_DIR'])
binary = args.binary.resolve()
state = out / 'shared state'
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
report = dict(binary=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
              source=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(), cases=[])
children = []


def config(name):
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    value = dict(bind=f'127.0.0.1:{port}', origin=f'http://127.0.0.1:{port}', data=str(root / 'data'),
                 client=str(args.client.resolve()), state=str(state), max_players=85,
                 behind_proxy=False, shore_bots=False, bot_llm=False, realm_seed=42)
    path = out / f'{name}.json'
    path.write_text(json.dumps(value, indent=2))
    return path, value['origin']


def start(name):
    path, origin = config(name)
    with (out / f'{name}.log').open('wb') as log:
        child = subprocess.Popen([str(binary), '--desktop', '--config', str(path)],
                                 stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
    children.append(child)
    deadline = time.monotonic() + 600
    while child.poll() is None:
        try:
            with http.open(origin + '/health', timeout=2) as response:
                health = json.load(response)
            assert health['process'] == child.pid
            return child, health
        except (urllib.error.URLError, TimeoutError):
            assert time.monotonic() < deadline, 'readiness deadline exceeded'
            time.sleep(.1)
    raise AssertionError(f'{name} exited before ready: {child.returncode}')


def database_hashes():
    return {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [state / 'game.db', state / 'game.db-wal'] if path.exists()}


def deny(name):
    before = database_hashes()
    assert 'game.db' in before
    path, _ = config(name)
    started = time.monotonic()
    result = subprocess.run([str(binary), '--desktop', '--config', str(path)],
                            input=b'', capture_output=True, timeout=10)
    elapsed = time.monotonic() - started
    text = (result.stdout + result.stderr).decode()
    (out / f'{name}.log').write_text(text)
    assert result.returncode != 0
    assert 'state lock held' in text and str(state) in text and 'state.lock' in text
    assert 'game frontdoor listening' not in text and 'lag log opened' not in text
    assert database_hashes() == before, 'denied consumer mutated the database or WAL'
    report['cases'].append(dict(name=name, exit=result.returncode, seconds=elapsed, database_hashes=before))


def stop(child, name):
    child.stdin.write(b'stop\n'); child.stdin.flush()
    assert child.wait(timeout=30) == 0
    assert 'shutdown persistence complete' in (out / f'{name}.log').read_text()


try:
    assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root).strip()
    first, health = start('first')
    deny('second-while-first-owns-state')
    stop(first, 'first')
    report['cases'].append(dict(name='clean-stop', health=health, exit=first.returncode))
    lock = state / 'state.lock'
    inode = lock.stat().st_ino
    second, health = start('after-clean-stop')
    assert lock.stat().st_ino == inode
    deny('third-while-second-owns-state')
    second.kill(); second.wait(timeout=30)
    report['cases'].append(dict(name='intentional-crash', health=health, exit=second.returncode))
    third, health = start('after-crash')
    assert lock.stat().st_ino == inode
    stop(third, 'after-crash')
    report['cases'].append(dict(name='reacquire-after-crash', health=health, exit=third.returncode))
    report['passed'] = True
finally:
    live = [child for child in children if child.poll() is None]
    for child in live:
        child.kill(); child.wait()
    (out / 'result.json').write_text(json.dumps(report, indent=2))
    if live:
        raise RuntimeError('forced cleanup is not successful shutdown')
print(out / 'result.json')
