#!/usr/bin/env python3
"""Exercise the consumer runtime pipe with a retained binary and isolated durable state."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
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
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
out = Path(os.environ['QA_RESULTS_DIR'])
install = out / 'runtime with spaces'
install.mkdir()
binary = install / args.binary.name
os.link(args.binary.resolve(), binary)
install.chmod(0o500)
config = dict(bind=f'127.0.0.1:{args.port}', origin=f'http://127.0.0.1:{args.port}',
              data=str(root / 'data'), client=str(args.client.resolve()), state=str(out / 'save with spaces'),
              max_players=85, behind_proxy=False, shore_bots=False, bot_llm=False, realm_seed=42)
config_path = out / 'runtime config.json'
config_path.write_text(json.dumps(config, indent=2))
base = config['origin']
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
report = dict(binary=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), cases=[])


def free_port():
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', args.port))


def ready(child):
    deadline = time.monotonic() + 600
    while child.poll() is None:
        try:
            with http.open(base + '/health', timeout=2) as response:
                health = json.load(response)
            assert health['process'] == child.pid, health
            assert health['bots'] == 0 and not health['bot_dialogue']['enabled']
            return health
        except (urllib.error.URLError, TimeoutError):
            assert time.monotonic() < deadline, 'readiness deadline exceeded'
            time.sleep(.2)
    raise AssertionError(f'child exited before readiness: {child.returncode}')


def guest():
    data = urllib.parse.urlencode(dict(guid='desktop-runtime-guest', password='')).encode()
    with http.open(base + '/legacy-api/char/list', data=data, timeout=10) as response:
        document = ET.fromstring(response.read())
    assert document.tag == 'Chars', ET.tostring(document)
    return document.find('Account/AccountId').text


try:
    account = None
    for mode in ['stop', 'eof', 'malformed', 'partial-timeout', 'desktop-signal', 'ordinary-signal']:
        free_port()
        command = [str(binary), '--config', str(config_path)]
        if mode != 'ordinary-signal':
            command.append('--desktop')
        with (out / f'{mode}.log').open('w') as log:
            child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT, cwd=install)
            try:
                health = ready(child)
                current = guest()
                if account is None:
                    account = current
                assert current == account, 'guest identity changed across shutdown/restart'
                assert not (Path(config['state']) / 'control.token').exists()
                started = time.monotonic()
                if mode == 'stop':
                    child.stdin.write(b'stop\n'); child.stdin.flush()
                elif mode == 'eof':
                    child.stdin.close()
                elif mode == 'malformed':
                    child.stdin.write(b'x' * 8192); child.stdin.flush()
                elif mode == 'partial-timeout':
                    child.stdin.write(b's'); child.stdin.flush()
                else:
                    if mode == 'ordinary-signal':
                        child.stdin.close()
                        time.sleep(.5)
                        assert child.poll() is None, 'ordinary mode treated stdin EOF as shutdown'
                    child.send_signal(signal.SIGTERM)
                code = child.wait(timeout=30)
                assert (code == 0) == (mode not in ['malformed', 'partial-timeout']), (mode, code)
                elapsed = time.monotonic() - started
                free_port()
                text = (out / f'{mode}.log').read_text()
                assert 'saving active characters before shutdown' in text
                if code == 0:
                    assert 'shutdown persistence complete' in text
                else:
                    assert 'shutdown failed' in text
                report['cases'].append(dict(mode=mode, health=health, account=account, exit=code, stop_seconds=elapsed))
            finally:
                if child.poll() is None:
                    child.kill(); child.wait()
                    raise RuntimeError('forced cleanup is not a successful shutdown')
    free_port()
    with (out / 'startup-eof.log').open('w') as log:
        child = subprocess.Popen([str(binary), '--desktop', '--config', str(config_path)],
                                 stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT, cwd=install)
        child.stdin.close()
        try:
            assert child.wait(timeout=600) == 0
        finally:
            if child.poll() is None:
                child.kill(); child.wait()
                raise RuntimeError('startup EOF left an orphan runtime')
    assert 'parent_eof' in (out / 'startup-eof.log').read_text()
    free_port()
    report['cases'].append(dict(mode='startup-eof', exit=child.returncode))
    for name, changed, error in [
        ('missing-assets', dict(client=str(out / 'missing client')), 'missing client asset'),
        ('invalid-state', dict(state=str(config_path)), 'File exists'),
    ]:
        path = out / f'{name}.json'
        path.write_text(json.dumps(config | changed))
        result = subprocess.run([str(binary), '--desktop', '--config', str(path)], capture_output=True, timeout=30, cwd=install)
        (out / f'{name}.log').write_bytes(result.stdout + result.stderr)
        assert result.returncode != 0 and error in (result.stdout + result.stderr).decode(), name
        report['cases'].append(dict(mode=name, exit=result.returncode))
    with socket.socket() as occupied:
        occupied.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        occupied.bind(('127.0.0.1', args.port)); occupied.listen()
        result = subprocess.run([str(binary), '--desktop', '--config', str(config_path)], capture_output=True, timeout=600, cwd=install)
    (out / 'occupied-port.log').write_bytes(result.stdout + result.stderr)
    assert result.returncode != 0 and b'cannot listen on' in result.stderr
    report['cases'].append(dict(mode='occupied-port', exit=result.returncode))
    report['passed'] = True
finally:
    (out / 'result.json').write_text(json.dumps(report, indent=2))
print(out / 'result.json')
