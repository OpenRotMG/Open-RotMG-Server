#!/usr/bin/env python3
"""Synthetic local-save CLI/legacy-auth journey; optional private native client continuation."""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import socket
import threading
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
parser.add_argument('--http-export', action='store_true')
parser.add_argument('--cors-proxy', type=Path)
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
source_before = rows(source / 'game.db')
portable = out / 'player.json'
assert not args.cors_proxy or args.http_export
if args.http_export:
    proxy = frontend = frontend_thread = proxy_log = None
    source_bind = f'127.0.0.1:{args.port}'
    source_origin = base
    if args.cors_proxy:
        class Frontend(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200); self.send_header('Content-Type', 'text/html'); self.end_headers()
                self.wfile.write(b'<!doctype html><title>Account export CORS QA</title>Owned cross-origin API test')
            def log_message(self, *values):
                pass
        frontend = ThreadingHTTPServer(('127.0.0.1', 0), Frontend)
        frontend_thread = threading.Thread(target=frontend.serve_forever)
        frontend_thread.start()
        source_origin = f'http://127.0.0.1:{frontend.server_port}'
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0)); source_bind = f'127.0.0.1:{probe.getsockname()[1]}'
        caddyfile = out / 'Caddyfile'
        block = (root / 'scripts/deploy/Caddyfile').read_text().split('game.openrotmg.org {', 1)[1]
        caddyfile.write_text('{\n admin off\n auto_https off\n}\n' + base + ' {' + block.replace('https://openrotmg.org', source_origin).replace('127.0.0.1:18923', source_bind))
        proxy_log = (out / 'cors-proxy.log').open('w')
        proxy = subprocess.Popen([str(args.cors_proxy.resolve()), 'run', '--config', str(caddyfile), '--adapter', 'caddyfile'], stdin=subprocess.DEVNULL, stdout=proxy_log, stderr=subprocess.STDOUT)
        report['corsProxy'] = dict(binary=str(args.cors_proxy.resolve()), version=subprocess.check_output([str(args.cors_proxy.resolve()), 'version'], text=True).strip(), sha256=hashlib.sha256(args.cors_proxy.read_bytes()).hexdigest(), template='scripts/deploy/Caddyfile', templateSha256=hashlib.sha256((root / 'scripts/deploy/Caddyfile').read_bytes()).hexdigest(), frontendOrigin=source_origin, apiOrigin=base)
    source_config = out / 'source-server.json'
    source_config.write_text(json.dumps(dict(bind=source_bind, origin=source_origin, data=str(data), client=str(args.client.resolve()), state=str(source), max_players=85, behind_proxy=bool(args.cors_proxy), shore_bots=False, bot_llm=False, realm_seed=42)))
    with (out / 'source-http.log').open('w') as log:
        child = subprocess.Popen([str(binary), *([] if args.cors_proxy else ['--desktop']), '--config', str(source_config)], stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
        def stop_source():
            if args.cors_proxy:
                child.terminate()
            else:
                child.stdin.write(b'stop\n'); child.stdin.flush()
        try:
            deadline = time.monotonic() + 600
            while True:
                assert child.poll() is None, f'source server exited {child.returncode}'
                assert proxy is None or proxy.poll() is None, 'CORS proxy exited; inspect cors-proxy.log'
                try:
                    with http.open(base + '/health', timeout=2) as response:
                        assert json.load(response)['process'] == child.pid
                    break
                except (urllib.error.URLError, TimeoutError):
                    assert time.monotonic() < deadline
                    time.sleep(.2)
            request = urllib.request.Request(base + '/api/account/export', json.dumps(dict(email='backup-true@example.invalid', password='backup-fixture-password')).encode(), {'Content-Type': 'application/json', 'Origin': source_origin})
            with http.open(request, timeout=30) as response:
                assert response.headers['Cache-Control'] == 'no-store'
                assert response.headers['Content-Type'] == 'application/json'
                content = response.read(32 * 1024 * 1024 + 1)
                assert len(content) <= 32 * 1024 * 1024
                downloaded = json.loads(content)
                assert response.headers['Content-Disposition'] == f'attachment; filename="openrotmg-account-{downloaded["exported_at"]}.json"'
            with sqlite3.connect(f'file:{source / "game.db"}?mode=ro', uri=True) as db:
                secrets = [v for table, column in [('credentials', 'email'), ('credentials', 'password_hash'), ('accounts', 'guid'), ('discord_links', 'discord_id'), ('discord_codes', 'hash'), ('recovery_codes', 'hash'), ('password_resets', 'hash')] for (v,) in db.execute(f'SELECT {column} FROM {table}') if isinstance(v, str) and len(v) >= 8]
            for secret in [*secrets, 'backup-fixture-password']:
                assert secret.encode() not in content
                assert secret not in (out / 'source-http.log').read_text()
            if args.cors_proxy:
                env = dict(os.environ, ACCOUNT_SAVE_ORIGIN=source_origin, ACCOUNT_SAVE_PHASE='cors', ACCOUNT_SAVE_CORS_API=base, PI_NATIVE_TEST_CHROME_ARGS='[]')
                subprocess.run(['node', str(root / 'examples/testing/account-save-native.mjs')], input=b'backup-fixture-password\n', env=env, check=True)
                browser_content = (out / 'cors-player.json').read_bytes()
                browser_save = json.loads(browser_content)
                downloaded.pop('exported_at'); comparable = dict(browser_save); comparable.pop('exported_at')
                assert comparable == downloaded
                assert len(browser_content) <= 32 * 1024 * 1024
                for secret in [*secrets, 'backup-fixture-password']:
                    assert secret.encode() not in browser_content
                content = browser_content
                report['assertions'].append('real-browser-CORS-preflight-readable-attachment-and-body')
            portable.write_bytes(content)
            stop_source()
            assert child.wait(timeout=30) == 0
            report['assertions'].extend(['real-http-attachment', 'http-credential-canaries-absent', 'source-http-clean-shutdown'])
        finally:
            if child.poll() is None:
                stop_source()
                try:
                    assert child.wait(timeout=30) == 0
                except BaseException:
                    child.kill(); child.wait()
                    raise RuntimeError('forced source server cleanup is not acceptance')
            if proxy is not None:
                proxy.terminate()
                try:
                    assert proxy.wait(timeout=10) == 0
                except BaseException:
                    proxy.kill(); proxy.wait()
                    raise RuntimeError('CORS proxy cleanup failed')
                proxy_log.close()
            if frontend is not None:
                frontend.shutdown(); frontend.server_close(); frontend_thread.join(timeout=10)
                assert not frontend_thread.is_alive()
else:
    save('export', source, data, 1, portable.name)
assert rows(source / 'game.db') == source_before
preview = save('preview', state, data, portable)
assert preview['login'] == 'player-1@local.invalid' and not preview['replaced']
verified_input = json.loads(portable.read_bytes())
assert (preview['name'], preview['exported_at'], preview['character_count']) == (verified_input['name'], verified_input['exported_at'], len(verified_input['characters']))
result = save('import', state, data, portable, out / 'unused-first-backup.db', preview['expected'], secret=password)
assert result['login'] == 'player-1@local.invalid'
report['import'] = result
save('verify-account', portable, data)
replacement = save('preview', state, data, portable)
# Cancellation/EOF or failed pre-backup must not touch a populated destination.
before = rows(state / 'game.db')
save('replace', state, data, portable, out / 'cancelled.db', replacement['expected'], secret=b'', success=False)
assert rows(state / 'game.db') == before
save('replace', state, data, portable, out / 'absent' / 'failed.db', replacement['expected'], secret=password, success=False)
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
save('replace', state, data, portable, out / 'pre-replace.db', replacement['expected'], secret=b'replacement-password\n')
assert rows(out / 'pre-replace.db') == old
save('restore', state, data, backup, out / 'pre-restore.db')
assert rows(state / 'game.db') == rows(backup)
report['assertions'].extend(['consistent-pre-replace-backup', 'full-local-restore'])
report['native'] = args.native
report['httpExport'] = args.http_export
assert rows(source / 'game.db') == source_before
if args.http_export:
    for path in out.glob('*.log'):
        for secret in [*secrets, 'backup-fixture-password']:
            assert secret not in path.read_text()
    report['assertions'].extend(['source-still-unchanged-after-local-play', 'all-retained-process-logs-source-credential-free'])
report['packageIntegration'] = 'NOT tested: candidate server CLI/native core only; frozen Phase 1 package runtime must be paired by its owner'
(out / 'roundtrip-result.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report, indent=2))
