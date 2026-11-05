import hashlib
import importlib.util
import io
import contextlib
import errno
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import unittest.mock
import urllib.error

spec = importlib.util.spec_from_file_location('update', Path(__file__).with_name('update.py'))
update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(update)


class UpdateTest(unittest.TestCase):

    def test_superseded_restart_cancels_only_an_advertised_countdown(self):
        config = {'bind': '127.0.0.1:18923'}
        metadata = {'revision': 'a' * 40, 'manifest': {}}
        notice = 'Server restart postponed. This countdown is cancelled.'
        stale = 'Main advanced during restart warning; no cutover performed'
        for warned, revision, reply, error in [
                (True, 'b' * 40, {'players': 2}, stale),
                (False, 'b' * 40, {'players': 2}, stale),
                (True, 'a' * 40, {'players': 2}, 'cutover reached'),
                (True, 'b' * 40, OSError('transport unavailable'), stale + '; restart postponement announcement failed: transport unavailable'),
                (True, 'b' * 40, None, stale + '; restart postponement announcement failed: Running server did not acknowledge restart postponement')]:
            with self.subTest(warned=warned, revision=revision, reply=reply), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                lock = root / 'backup.lock'
                lock.touch()
                with patch.object(update, 'LAST_BACKUP', root / 'last-success.json'), \
                        patch.object(update, 'run'), patch.object(update, 'rehearse'), \
                        patch.object(update, 'install_backups'), patch.object(update, 'files', return_value={}), \
                        patch.object(update, 'main_revision', side_effect=['a' * 40, revision]), \
                        patch.object(update, 'warn_restart', return_value={'process': {}, 'receipt': {}} if warned else False), \
                        patch.object(update, 'checked_restart_request', return_value={'state': 'consumed'}), \
                        patch.object(update, 'announce', side_effect=reply if isinstance(reply, Exception) else None, return_value=reply) as announce, \
                        patch.object(update, 'outcome', side_effect=RuntimeError('cutover reached')) as outcome, \
                        patch.object(update, 'stop_prepared') as stop, patch.object(update, 'drain_process') as drain, \
                        patch.object(update, 'exchange') as exchange, contextlib.redirect_stdout(io.StringIO()) as output:
                    with self.assertRaises(RuntimeError) as raised:
                        update.deploy(root / 'stage', root, metadata, config)
                    self.assertEqual(str(raised.exception), error)
                if warned and revision != metadata['revision']:
                    announce.assert_called_once_with(config, notice)
                else:
                    announce.assert_not_called()
                if revision == metadata['revision']:
                    outcome.assert_called_once_with(root, 'cutover', root / 'stage')
                else:
                    outcome.assert_not_called()
                if warned and revision != metadata['revision'] and isinstance(reply, dict):
                    self.assertEqual(json.loads(output.getvalue()), {'restart_postponed': notice, 'players': 2})
                stop.assert_not_called()
                drain.assert_not_called()
                exchange.assert_not_called()

    def test_drain_refusal_and_failed_shutdown_never_exchange_source(self):
        refused = 'Drain preparation failed: synthetic save failure'
        for error, result in [(update.DrainRefused(refused), 'success'),
                              (RuntimeError('Drain process replaced before stop'), 'success'),
                              (None, 'exit-code'), (None, 'success')]:
            with self.subTest(error=str(error), result=result), tempfile.TemporaryDirectory() as directory:
                root, state = Path(directory), Path(directory) / 'state'
                state.mkdir()
                (state / 'server.pem').write_text('key')
                (root / 'backup.lock').touch()
                live = root / 'server'
                live.mkdir()
                (live / 'REVISION').write_text('a' * 40 + '\n')
                calls = []
                def stop(*args):
                    if error is not None:
                        raise error
                    calls.append(('systemctl', 'stop', 'openrotmg.service'))
                show = lambda args, **_: {'Result': result + '\n', 'MainPID': '7\n'}[args[4]]
                patches = {'LAST_BACKUP': root / 'last-success.json', 'LIVE': live, 'rehearse': None, 'install_backups': None,
                           'outcome': None, 'copy_db': None, 'ready': None, 'start_backup': None}
                with contextlib.ExitStack() as stack:
                    for name, value in patches.items():
                        stack.enter_context(patch.object(update, name, value) if value else patch.object(update, name))
                    copy = update.copy_db
                    backup = update.start_backup
                    for name, kwargs in {'run': {'side_effect': lambda *a: calls.append(a)}, 'files': {'return_value': {}},
                                         'main_revision': {'return_value': 'a' * 40},
                                         'warn_restart': {'return_value': {'process': {'pid': 7}, 'receipt': {'owner': 'synthetic-recovery'}}},
                                         'checked_restart_request': {'return_value': {'state': 'consumed'}},
                                         'stop_prepared': {'side_effect': stop}, 'digest': {'return_value': 'sha'},
                                         'exchange': {'side_effect': lambda *a: calls.append(('exchange',))}}.items():
                        stack.enter_context(patch.object(update, name, **kwargs))
                    stack.enter_context(patch.object(update.subprocess, 'check_output', side_effect=show))
                    restore = stack.enter_context(patch.object(update, 'restore_service', return_value='already_running'))
                    stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
                    metadata = {'revision': 'a' * 40, 'manifest': {}, 'binary_sha256': 'sha'}
                    config = {'bind': '127.0.0.1:18923', 'state': str(state)}
                    activated = error is None and result == 'success'
                    if activated:
                        update.deploy(root / 'stage', root, metadata, config)
                    else:
                        self.assertRaises(RuntimeError, update.deploy, root / 'stage', root, metadata, config)
                self.assertEqual(('systemctl', 'stop', 'openrotmg.service') in calls, error is None)
                self.assertEqual(('exchange',) in calls, activated)
                self.assertEqual(copy.call_count, int(activated))
                self.assertEqual(backup.call_count, int(activated))
                self.assertEqual(('systemctl', 'start', 'openrotmg.service') in calls, activated)
                self.assertEqual(restore.call_count, int(not activated))
                self.assertEqual((root / 'previous-server-path').exists(), activated)

    def test_consume_failure_or_cancellation_never_reaches_stop(self):
        config = {'bind': '127.0.0.1:18923'}
        metadata = {'revision': 'a' * 40, 'manifest': {}}
        for reply in [RuntimeError('synthetic consumed settlement failure'), {'state': 'cancelled'}]:
            with self.subTest(reply=reply), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'backup.lock').touch()
                with patch.object(update, 'LAST_BACKUP', root / 'last-success.json'), \
                        patch.object(update, 'run') as run, patch.object(update, 'rehearse'), \
                        patch.object(update, 'install_backups'), patch.object(update, 'files', return_value={}), \
                        patch.object(update, 'main_revision', return_value='a' * 40), \
                        patch.object(update, 'warn_restart', return_value={'process': {'pid': 7}, 'receipt': {'owner': 'synthetic-consume'}}), \
                        patch.object(update, 'checked_restart_request', side_effect=reply if isinstance(reply, Exception) else None, return_value=reply) as consume, \
                        patch.object(update, 'outcome') as outcome, patch.object(update, 'stop_prepared') as stop, \
                        patch.object(update, 'exchange') as exchange, patch.object(update, 'restore_service') as restore:
                    with self.assertRaises(RuntimeError):
                        update.deploy(root / 'stage', root, metadata, config)
                    consume.assert_called_once_with(config, {'pid': 7}, 'consume', {'owner': 'synthetic-consume'})
                    self.assertFalse(any(call.args[:2] == ('systemctl', 'stop') for call in run.call_args_list))
                    outcome.assert_not_called()
                    stop.assert_not_called()
                    exchange.assert_not_called()
                    restore.assert_not_called()

    def test_failed_drain_cancellation_never_forces_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'backup.lock').touch()
            failed = {'state': 'failed', 'errors': ['synthetic save failure'], 'receipt': {'instance': 'owned', 'generation': 1}}
            def drain(config, process, operation, receipt=None):
                if operation == 'cancel':
                    raise RuntimeError('synthetic cancellation failure')
                self.assertEqual(operation, 'prepare')
                return failed
            with patch.object(update, 'LAST_BACKUP', root / 'last-success.json'), \
                    patch.object(update, 'run') as run, patch.object(update, 'rehearse'), \
                    patch.object(update, 'install_backups'), patch.object(update, 'files', return_value={}), \
                    patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update, 'warn_restart', return_value=False), patch.object(update, 'outcome'), \
                    patch.object(update, 'drain_process', return_value={'pid': 7}), \
                    patch.object(update, 'checked_drain_request', side_effect=drain) as request, \
                    patch.object(update, 'exchange') as exchange, patch.object(update, 'copy_db') as copy, \
                    patch.object(update, 'start_backup') as backup, \
                    patch.object(update, 'restore_service', return_value='already_running') as restore:
                with self.assertRaisesRegex(RuntimeError, 'synthetic cancellation failure'):
                    update.deploy(root / 'stage', root, {'revision': 'a' * 40, 'manifest': {}}, {'bind': '127.0.0.1:18923'})
                self.assertEqual([call.args[2] for call in request.call_args_list], ['prepare', 'cancel'])
                self.assertFalse(any(call.args[:2] == ('systemctl', 'stop') for call in run.call_args_list))
                exchange.assert_not_called()
                copy.assert_not_called()
                backup.assert_not_called()
                restore.assert_called_once()

    def test_rehearsal_compares_the_schema22_public_restore(self):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage, work = root / 'stage', root / 'work'
            stage.mkdir()
            work.mkdir()
            (stage / 'REVISION').write_text('a' * 40)
            game_schema, discord_schema = object(), object()
            module = SimpleNamespace(
                RELOADED_GAME=game_schema, DISCORD=discord_schema,
                export=lambda *args: {'game': {'achievement_partners': []}, 'discord': {}},
                restore=lambda *args: None,
                snapshot=lambda path, schema: {'achievement_partners': []} if schema is game_schema else {},
            )
            process = unittest.mock.Mock(returncode=0)
            process.poll.return_value = 0
            with patch.object(update, 'run') as run, patch.object(update, 'copy_db'), \
                    patch.object(update.shutil, 'copyfile', side_effect=lambda source, target: Path(target).touch()), \
                    patch.object(update, 'rows', return_value={}), patch.object(update, 'verify_migration'), \
                    patch.object(update, 'ready', return_value={'players': 0, 'bots': 0, 'qa_actors': 0}), \
                    patch.object(update.subprocess, 'Popen', return_value=process), \
                    patch.object(update, 'backup_module', return_value=module):
                update.rehearse(stage, work, {'state': str(root)})
                self.assertEqual(run.call_args_list[-1].args[1], '--backup-load')
            self.assertFalse((work / 'rehearsal-running').exists())

    def test_helper_install_tracks_release_and_rejects_invalid_source(self):
        with tempfile.TemporaryDirectory() as directory:
            installed, source = Path(directory) / 'update.py', Path(directory) / 'source.py'
            installed.write_text('old = 1\n')
            source.write_text('old = 1\n')
            with patch.object(update, 'HELPER', installed), contextlib.redirect_stdout(io.StringIO()):
                self.assertFalse(update.install_helper(source))
                source.write_text('new = 2\n')
                self.assertTrue(update.install_helper(source))
                self.assertEqual(installed.read_text(), 'new = 2\n')
                self.assertEqual(installed.stat().st_mode & 0o777, 0o755)
                source.write_text('def broken(:\n')
                with self.assertRaises(SyntaxError):
                    update.install_helper(source)
                self.assertEqual(installed.read_text(), 'new = 2\n')

    def test_failed_cutover_restarts_the_previous_release(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live, old = root / 'server', root / 'server-stage-old'
            live.mkdir()
            (live / 'REVISION').write_text('new\n')
            (root / 'previous-server-path').write_text(str(old) + '\n')
            calls = []
            def swap(a, b):
                calls.append(('exchange', a, b))
                (live / 'REVISION').write_text('old\n')
            stopped = unittest.mock.MagicMock(returncode=3)
            with patch.object(update, 'LIVE', live), patch.object(update, 'exchange', side_effect=swap), \
                    patch.object(update.subprocess, 'run', return_value=stopped), \
                    patch.object(update, 'run', side_effect=lambda *a: calls.append(a)), \
                    patch.object(update, 'ready', side_effect=lambda port: calls.append(('ready', port))), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(update.restore_service({'bind': '127.0.0.1:18923'}, root), 'old')
            self.assertEqual(calls, [('exchange', live, old), ('systemctl', 'reset-failed', 'openrotmg.service'),
                                     ('systemctl', 'start', 'openrotmg.service'), ('ready', 18923)])
            self.assertTrue((root / 'restored-server-path').exists())
            running = unittest.mock.MagicMock(returncode=0)
            with patch.object(update.subprocess, 'run', return_value=running), patch.object(update, 'run') as run:
                self.assertEqual(update.restore_service({'bind': '127.0.0.1:18923'}, root), 'already_running')
                run.assert_not_called()

    def test_caddy_install_reloads_changes_and_restores_on_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            live, source = Path(directory) / 'Caddyfile', Path(directory) / 'source'
            live.write_text('old')
            source.write_text('old')
            calls = []
            with patch.object(update, 'CADDY', live), patch.object(update, 'run', side_effect=lambda *a: calls.append(a)):
                self.assertFalse(update.install_caddy(source))
                self.assertEqual(calls, [])
                source.write_text('new')
                ok = unittest.mock.MagicMock(status=200)
                ok.__enter__.return_value = ok
                with patch.object(update.urllib.request, 'urlopen', return_value=ok), contextlib.redirect_stdout(io.StringIO()):
                    self.assertTrue(update.install_caddy(source))
                self.assertEqual(live.read_text(), 'new')
                self.assertEqual([c[0] for c in calls], ['caddy', 'systemctl'])
                source.write_text('broken')
                with patch.object(update.urllib.request, 'urlopen', side_effect=urllib.error.URLError('down')), \
                        self.assertRaises(urllib.error.URLError):
                    update.install_caddy(source)
                self.assertEqual(live.read_text(), 'new')

    def delivery(self, transform=lambda entries: entries):
        contents = {'openrotmg': b'binary', 'REVISION': b'a' * 40 + b'\n'}
        inner = io.BytesIO()
        with tarfile.open(fileobj=inner, mode='w:gz') as archive:
            for name, value in contents.items():
                entry = tarfile.TarInfo(name)
                entry.size = len(value)
                archive.addfile(entry, io.BytesIO(value))
        metadata = {'format': 1, 'revision': 'a' * 40, 'binary_sha256': hashlib.sha256(b'binary').hexdigest(),
                    'manifest': {k: hashlib.sha256(v).hexdigest() for k, v in contents.items()},
                    'archive_sha256': hashlib.sha256(inner.getvalue()).hexdigest()}
        output = io.BytesIO()
        with tarfile.open(fileobj=output, mode='w:') as archive:
            for name, data in transform([('release.json', json.dumps(metadata).encode()), ('server.tar.gz', inner.getvalue())]):
                entry = tarfile.TarInfo(name)
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
        return output.getvalue(), metadata

    def test_delivery_rejects_stale_main_unsafe_paths_corruption_and_limits(self):
        raw, _ = self.delivery()
        with patch.object(update, 'main_revision', return_value='b' * 40), patch.object(update, 'deploy') as deploy:
            with self.assertRaisesRegex(RuntimeError, 'Main advanced'):
                update.main('a' * 40, raw)
            deploy.assert_not_called()
        for transform, error in [
                (lambda entries: entries + [('../escape', b'bad')], 'Expected only'),
                (lambda entries: entries + [entries[0]], 'Expected only'),
                (lambda entries: [(n, b'corrupt' if n == 'server.tar.gz' else d) for n, d in entries], 'archive mismatch')]:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / 'REVISION').write_text('b' * 40)
                raw, _ = self.delivery(transform)
                with patch.object(update, 'ROOT', root / 'state'), patch.object(update, 'LIVE', root), \
                        patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'deploy') as deploy, \
                        patch.object(update.shutil, 'disk_usage', return_value=unittest.mock.Mock(free=0)):
                    with self.assertRaisesRegex(RuntimeError, error):
                        update.main('a' * 40, raw)
                    deploy.assert_not_called()
                    self.assertEqual(json.loads((root / 'state/status.json').read_text())['state'], 'failed')
        with patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'LIMIT', 1):
            with self.assertRaisesRegex(RuntimeError, 'exceeds'):
                update.main('a' * 40, raw)











    def test_zero_free_does_not_block_valid_delivery(self):
        raw, _ = self.delivery()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live = root / 'server'
            live.mkdir()
            (live / 'REVISION').write_text('b' * 40)
            config = root / 'config.json'
            config.write_text('{}')
            backup = root / 'backup.json'
            backup.write_text('{}')
            with patch.object(update, 'ROOT', root / 'deploy'), patch.object(update, 'LIVE', live), \
                    patch.object(update, 'CONFIG', config), patch.object(update, 'LAST_BACKUP', backup), \
                    patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update, 'retain_deployments'), patch.object(update, 'deploy') as deploy, \
                    patch.object(update, 'install_caddy'), patch.object(update, 'install_helper'), \
                    patch.object(update.shutil, 'disk_usage', return_value=unittest.mock.Mock(free=0)) as usage, \
                    contextlib.redirect_stdout(io.StringIO()):
                update.main('a' * 40, raw)
                deploy.assert_called_once()
                usage.assert_not_called()
                self.assertEqual(json.loads((root / 'deploy/status.json').read_text())['state'], 'deployed')

    def test_allocation_failure_is_not_ignored(self):
        raw, _ = self.delivery()
        with patch.object(update, 'main_revision', return_value='a' * 40), \
                patch.object(update.Path, 'mkdir', side_effect=OSError(errno.ENOSPC, 'synthetic allocation failure')), \
                patch.object(update, 'deploy') as deploy:
            with self.assertRaises(OSError) as raised:
                update.main('a' * 40, raw)
            self.assertEqual(raised.exception.errno, errno.ENOSPC)
            deploy.assert_not_called()

    def test_uploaded_backup_requires_fresh_restore_and_actual_running_revision(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'REVISION').write_text('a' * 40)
            receipt = root / 'receipt.json'
            proof = {'uploaded_at': '2026-10-15T12:00:00+00:00', 'restored': True, 'revisions': {'server': 'a' * 40}}
            with patch.object(update, 'LIVE', root), patch.object(update, 'LAST_BACKUP', receipt):
                receipt.write_text(json.dumps(proof))
                stamp = update.datetime.datetime.fromisoformat(proof['uploaded_at']).timestamp()
                update.verify_backup(stamp - 1)
                with self.assertRaisesRegex(RuntimeError, 'Fresh uploaded backup'):
                    update.verify_backup(stamp + 1)
                for change in [{'restored': False}, {'revisions': {'server': 'b' * 40}}]:
                    receipt.write_text(json.dumps({**proof, **change}))
                    with self.assertRaisesRegex(RuntimeError, 'Fresh uploaded backup'):
                        update.verify_backup(stamp - 1)







if __name__ == '__main__':
    unittest.main()
