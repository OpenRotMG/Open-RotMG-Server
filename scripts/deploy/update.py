#!/usr/bin/env python3
import ctypes
import datetime
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request
from contextlib import closing

REPO = 'https://github.com/OpenRotMG/Open-RotMG-Server'
LIVE = Path('/opt/openrotmg/server')
ROOT = Path('/var/lib/openrotmg-deploy')
BACKUPS = Path('/opt/openrotmg-backup')
CONFIG = Path('/etc/openrotmg/config.json')
ANNOUNCE_TOKEN = Path('/etc/openrotmg/discord-link-token')
RESTART_NOTICE_SECONDS = 20
LAST_BACKUP = Path('/var/lib/openrotmg-backup/last-success.json')
sys.dont_write_bytecode = True
os.environ['PYTHONDONTWRITEBYTECODE'] = '1'


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def files(root):
    return {str(p.relative_to(root)): digest(p) for p in root.rglob('*') if p.is_file()}


def status(state, desired, **fields):
    value = {'state': state, 'desired': desired, 'running': (LIVE / 'REVISION').read_text().strip(),
             'checked_at': int(time.time()), **fields}
    temporary = ROOT / 'status.json.new'
    temporary.write_text(json.dumps(value) + '\n')
    temporary.replace(ROOT / 'status.json')
    print(json.dumps(value), flush=True)


def download(url, path):
    with urllib.request.urlopen(url, timeout=30) as response:
        require(response.url.startswith('https://'), 'Non-HTTPS artifact redirect')
        body = response.read(64 * 1024 * 1024 + 1)
    require(len(body) <= 64 * 1024 * 1024, 'Release exceeds 64 MiB limit')
    path.write_bytes(body)


def unpack(archive, directory, metadata):
    directory.mkdir(mode=0o755)
    directory.chmod(0o755)
    with tarfile.open(archive) as bundle:
        for member in bundle.getmembers():
            require(not Path(member.name).is_absolute() and '..' not in Path(member.name).parts
                    and member.isfile(), 'Unsafe release archive member')
        bundle.extractall(directory)
    for path in directory.rglob('*'):
        path.chmod(0o755 if path.is_dir() or path.name == 'openrotmg' or path.suffix == '.sh' else 0o644)
    require(files(directory) == metadata['manifest'], 'Release manifest mismatch')
    require(digest(directory / 'openrotmg') == metadata['binary_sha256'], 'Release binary mismatch')
    require((directory / 'REVISION').read_text().strip() == metadata['revision'], 'Release revision mismatch')


def copy_db(source, target):
    with closing(sqlite3.connect(Path(source).resolve().as_uri() + '?mode=ro', uri=True)) as src, closing(sqlite3.connect(target)) as dst:
        src.backup(dst)
    target.chmod(0o600)


def rows(path):
    with closing(sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)) as db:
        db.execute('BEGIN')
        require(db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok', 'SQLite integrity failed')
        require(not db.execute('PRAGMA foreign_key_check').fetchall(), 'SQLite foreign keys failed')
        tables = [r[0] for r in db.execute("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")]
        return {table: db.execute('SELECT * FROM "' + table.replace('"', '""') + '" ORDER BY 1').fetchall() for table in tables}


def ready(port, process=None):
    deadline = time.monotonic() + 45
    while True:
        if process is not None:
            require(process.poll() is None, 'Rehearsal exited before readiness; inspect shadow.log')
        try:
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=3) as response:
                health = json.load(response)
            require(type(health['uptime_seconds']) is int, 'Invalid server health')
            return health
        except OSError:
            require(time.monotonic() < deadline, 'Server readiness deadline exceeded')
            time.sleep(.2)


def backup_module(stage):
    os.environ['OPENROTMG_BACKUP_BINARY'] = str(stage / 'openrotmg')
    spec = importlib.util.spec_from_file_location('release_backup', stage / 'scripts/backups/backup.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_backup(started):
    proof = json.loads(LAST_BACKUP.read_text())
    uploaded = datetime.datetime.fromisoformat(proof['uploaded_at']).timestamp()
    require(proof.get('restored') is True and uploaded >= started
            and proof['revisions']['server'] == (LIVE / 'REVISION').read_text().strip(),
            'Fresh uploaded backup and exact restore were not verified')


def start_backup():
    require(subprocess.check_output(['systemctl', 'show', 'openrotmg-backup', '-p', 'Result', '--value'], text=True).strip() == 'success',
            'Previous backup failed; resolve it or await its scheduled retry')
    # Successful deployment backups must not consume the scheduled job's failure-retry budget.
    run('systemctl', 'reset-failed', 'openrotmg-backup.service')
    started = time.time()
    run('systemctl', 'start', 'openrotmg-backup.service')
    require(subprocess.check_output(['systemctl', 'show', 'openrotmg-backup', '-p', 'Result', '--value'], text=True).strip() == 'success', 'Independent GitHub backup failed')
    verify_backup(started)


def install_backups(stage):
    state = subprocess.check_output(['systemctl', 'show', 'openrotmg-backup', '-p', 'ActiveState', '--value'], text=True).strip()
    require(state == 'inactive', f'Backup service is {state}; inspect its journal before retrying')
    run('runuser', '-u', 'openrotmg', '--', sys.executable, '-B', '-W', 'error::ResourceWarning',
        '-m', 'unittest', 'discover', '-s', str(stage / 'scripts/backups'), '-p', 'test_*.py')
    revision = (stage / 'REVISION').read_text().strip()
    releases = BACKUPS / 'releases'
    releases.mkdir(exist_ok=True)
    bundle = releases / revision
    sources = {'backup.py': stage / 'scripts/backups/backup.py', 'openrotmg': stage / 'openrotmg',
               'REVISION': stage / 'REVISION', 'openrotmg-backup.sh': stage / 'scripts/backups/openrotmg-backup.sh'}
    sources.update({str(path.relative_to(stage)): path for path in (stage / 'data').rglob('*') if path.is_file()})
    if not bundle.exists():
        temporary = releases / (revision + '.new')
        temporary.mkdir()
        for name, source in sources.items():
            target = temporary / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source.read_bytes())
            target.chmod(0o755 if name in {'openrotmg', 'openrotmg-backup.sh'} else 0o644)
        temporary.rename(bundle)
    require(files(bundle) == {name: digest(path) for name, path in sources.items()}, 'Backup tool bundle differs from release')
    pointer = BACKUPS / 'current.new'
    pointer.symlink_to(bundle)
    pointer.replace(BACKUPS / 'current')
    wrapper = BACKUPS / 'run.sh.new'
    wrapper.write_text('#!/bin/sh\nset -eu\nexec /opt/openrotmg-backup/current/openrotmg-backup.sh\n')
    wrapper.chmod(0o755)
    wrapper.replace(BACKUPS / 'run.sh')
    start_backup()


def announce(config, text):
    token = ANNOUNCE_TOKEN.read_text().strip()
    request = urllib.request.Request(
        'http://127.0.0.1:' + config['bind'].rsplit(':', 1)[1] + '/internal/discord/announce',
        data=json.dumps({'text': text}).encode(), headers={'authorization': 'Bearer ' + token})
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        error.close()
        if error.code == 404:
            return None
        raise RuntimeError('Announcement rejected: HTTP ' + str(error.code)) from error


def warn_restart(config):
    state = subprocess.check_output(['systemctl', 'show', 'openrotmg', '-p', 'ActiveState', '--value'], text=True).strip()
    if state != 'active':
        print(json.dumps({'restart_warning': 'skipped', 'service_state': state}), flush=True)
        return False
    notice = 'Server restarting in ' + str(RESTART_NOTICE_SECONDS) + ' seconds to apply an update.'
    result = announce(config, notice)
    if result is None:
        print(json.dumps({'restart_warning': 'unsupported', 'detail': 'running server predates the announcement endpoint'}), flush=True)
        return False
    print(json.dumps({'restart_warning': notice, 'seconds': RESTART_NOTICE_SECONDS, **result}), flush=True)
    time.sleep(RESTART_NOTICE_SECONDS)
    return True


def rehearse(stage, work, config):
    run(sys.executable, '-B', str(stage / 'scripts/backups/release_contract.py'),
        str(stage / 'openrotmg'), str(stage / 'data'), str(work / 'writer-contract'),
        (stage / 'REVISION').read_text().strip())
    shadow = work / 'shadow'
    shadow.mkdir(mode=0o700)
    copy_db(Path(config['state']) / 'game.db', shadow / 'game.db')
    shutil.copyfile(Path(config['state']) / 'server.pem', shadow / 'server.pem')
    (shadow / 'server.pem').chmod(0o600)
    before = rows(shadow / 'game.db')
    cfg = {**config, 'bind': '127.0.0.1:18939', 'data': str(stage / 'data'),
           'state': str(shadow), 'shore_bots': False}
    path = work / 'shadow.json'
    path.write_text(json.dumps(cfg))
    with (work / 'shadow.log').open('w') as log:
        process = subprocess.Popen([str(stage / 'openrotmg'), '--config', str(path)],
                                   stdout=log, stderr=subprocess.STDOUT)
        try:
            health = ready(18939, process)
            require(health['players'] == health['bots'] == health['qa_actors'] == 0, 'Unexpected rehearsal actors')
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            process.wait(timeout=20)
        require(process.returncode == 0, 'Rehearsal shutdown failed')
    after = rows(shadow / 'game.db')
    require(all(after.get(table) == data for table, data in before.items()), 'Migration changed existing records')
    module = backup_module(stage)
    archive = work / 'shadow-export.json.gz'
    bundle = module.export(shadow / 'game.db', '/var/lib/openrotmg-discord-bot/levels.sqlite', archive,
                           stage / 'REVISION', '/opt/openrotmg-discord-bot/REVISION', stage / 'REVISION')
    restored = work / 'restored'
    module.restore(archive, restored)
    run(str(stage / 'openrotmg'), '--backup-load', str(restored / 'game.db'), str(stage / 'data'))
    require(module.snapshot(restored / 'game.db', module.RELOADED_GAME) == bundle['game']
            and module.snapshot(restored / 'levels.sqlite', module.DISCORD) == bundle['discord'], 'Credential-free restore mismatch')


def deploy(stage, work, metadata, config):
    run('runuser', '-u', 'openrotmg', '--', 'test', '-x', str(stage / 'openrotmg'))
    rehearse(stage, work, config)
    install_backups(stage)
    require(files(stage) == metadata['manifest'], 'Rehearsal changed release files')
    require(main_revision() == metadata['revision'], 'Main advanced during rehearsal; no cutover performed')
    with LAST_BACKUP.with_name('backup.lock').open('rb') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        warn_restart(config)
        run('systemctl', 'stop', 'openrotmg.service')
        require(subprocess.check_output(['systemctl', 'show', 'openrotmg', '-p', 'Result', '--value'], text=True).strip() == 'success', 'Shutdown save failed; no source exchange performed')
        frozen = work / 'cutover'
        frozen.mkdir(mode=0o700)
        copy_db(Path(config['state']) / 'game.db', frozen / 'game.db')
        shutil.copyfile(Path(config['state']) / 'server.pem', frozen / 'server.pem')
        (frozen / 'server.pem').chmod(0o600)
        libc = ctypes.CDLL(None, use_errno=True)
        require(libc.renameat2(-100, os.fsencode(LIVE), -100, os.fsencode(stage), 2) == 0,
                'Atomic source exchange failed; frozen recovery state retained')
        (work / 'previous-server-path').write_text(str(stage) + '\n')
        run('runuser', '-u', 'openrotmg', '--', 'test', '-x', str(LIVE / 'openrotmg'))
        run('systemctl', 'start', 'openrotmg.service')
        ready(int(config['bind'].rsplit(':', 1)[1]))
        require((LIVE / 'REVISION').read_text().strip() == metadata['revision'], 'Activated revision mismatch')
        require(digest(LIVE / 'openrotmg') == metadata['binary_sha256'], 'Activated binary mismatch')
        pid = int(subprocess.check_output(['systemctl', 'show', 'openrotmg', '-p', 'MainPID', '--value'], text=True))
        require(pid > 0 and digest(f'/proc/{pid}/exe') == metadata['binary_sha256'], 'Running process binary mismatch')
    start_backup()


def main_revision():
    revision = subprocess.check_output(['git', 'ls-remote', '--exit-code', REPO + '.git', 'refs/heads/main'], text=True, timeout=30).split()[0]
    require(re.fullmatch('[0-9a-f]{40}', revision), 'Invalid main revision')
    return revision


def main(retry=False):
    os.umask(0o077)
    ROOT.mkdir(mode=0o700, exist_ok=True)
    desired = main_revision()
    previous = ROOT / 'status.json'
    if previous.exists() and not retry:
        last = json.loads(previous.read_text())
        if last['desired'] == desired and last['state'] == 'blocked':
            print(json.dumps(last), flush=True)
            return
    if (LIVE / 'REVISION').read_text().strip() == desired:
        ready(int(json.loads(CONFIG.read_text())['bind'].rsplit(':', 1)[1]))
        previous = ROOT / 'status.json'
        if previous.exists() and json.loads(previous.read_text())['state'] in {'failed', 'blocked', 'activated_unverified'}:
            start_backup()
        status('up_to_date', desired)
        return
    require(shutil.disk_usage(ROOT).free >= 2 * 1024**3, 'Less than 2 GiB free; inspect retained recovery data before deploying')
    status('checking_release', desired)
    work = ROOT / (desired + '-' + str(time.time_ns()))
    work.mkdir(mode=0o700)
    try:
        url = REPO + '/releases/download/server-' + desired
        try:
            download(url + '/release.json', work / 'release.json')
        except urllib.error.HTTPError as error:
            error.close()
            if error.code != 404:
                raise
            status('waiting_for_build', desired)
            return
        metadata = json.loads((work / 'release.json').read_text())
        require(metadata['format'] == 1 and metadata['revision'] == desired, 'Artifact source mismatch')
        download(url + '/server.tar.gz', work / 'server.tar.gz')
        require(digest(work / 'server.tar.gz') == metadata['archive_sha256'], 'Artifact archive mismatch')
        stage = LIVE.with_name('server-stage-' + str(time.time_ns()))
        unpack(work / 'server.tar.gz', stage, metadata)
        status('rehearsing', desired)
        config = json.loads(CONFIG.read_text())
        deploy(stage, work, metadata, config)
        status('deployed', desired, recovery_directory=str(work), backup=json.loads(LAST_BACKUP.read_text()))
    except Exception as error:
        activated = (LIVE / 'REVISION').read_text().strip() == desired
        state = 'activated_unverified' if activated else ('failed' if isinstance(error, OSError) else 'blocked')
        status(state, desired, recovery_directory=str(work), error=str(error))
        raise


if __name__ == '__main__':
    require(sys.argv[1:] in ([], ['--retry']), 'Only --retry is accepted')
    main(retry=bool(sys.argv[1:]))
