import hashlib
import importlib.util
import io
import json
import sqlite3
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
from contextlib import closing

spec = importlib.util.spec_from_file_location('update', Path(__file__).with_name('update.py'))
update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(update)


class UpdateTest(unittest.TestCase):
    def test_additive_gold_migration_preserves_every_existing_cell(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'game.db'
            with closing(sqlite3.connect(path)) as db, db:
                db.executescript("CREATE TABLE accounts(id INTEGER PRIMARY KEY,guid TEXT,name TEXT);"
                                 "INSERT INTO accounts VALUES(38,'fixture','Bayleaf')")
            before = update.rows(path)
            with closing(sqlite3.connect(path)) as db, db:
                db.execute('ALTER TABLE accounts ADD COLUMN credits INTEGER NOT NULL DEFAULT 100')
            after = update.rows(path)
            self.assertEqual(after['accounts'][0]['credits'], 100)
            update.verify_migration(before, after)
            row = after['accounts'][0]
            invalid = [{}, {'accounts': []}, {'accounts': [row, row]},
                       {'accounts': [{**row, 'name': 'Changed'}]},
                       {'accounts': [{k: v for k, v in row.items() if k != 'guid'}]}]
            for changed in invalid:
                with self.subTest(changed=changed), self.assertRaisesRegex(RuntimeError, 'Migration changed existing records'):
                    update.verify_migration(before, changed)

    def test_vault_migration_preserves_legacy_rows_and_only_provisions_unread_accounts(self):
        before = {'accounts': [{'id': 1}, {'id': 2}], 'vaults': [
            {'account': 1, 'items': '[2594,-1,-1,-1,-1,-1,-1,-1]', 'revision': 37}]}
        after = {'accounts': [{'id': 1, 'vault_mask': 1}, {'id': 2, 'vault_mask': 1}], 'vaults': [
            {**before['vaults'][0], 'chest': 0},
            {'account': 2, 'chest': 0, 'items': '[-1,-1,-1,-1,-1,-1,-1,-1]', 'revision': 0}]}
        update.verify_migration(before, after)
        for rows in [[], after['vaults'][:1], after['vaults'] * 2,
                     [{**row, 'revision': 0} for row in after['vaults']]]:
            with self.subTest(rows=rows), self.assertRaisesRegex(RuntimeError, 'Vault migration'):
                update.verify_migration(before, {**after, 'vaults': rows})
        with self.assertRaisesRegex(RuntimeError, 'Migration changed existing records'):
            update.verify_migration(after, {**after, 'vaults': after['vaults'][:1]})

    def test_vault_migration_preserves_saved_rows_with_unread_accounts_before_between_and_after(self):
        before = {'accounts': [{'id': account} for account in range(1, 6)], 'vaults': [
            {'account': 2, 'items': '[2594,-1,-1,-1,-1,-1,-1,-1]', 'revision': 37},
            {'account': 4, 'items': '[-1,2595,-1,-1,-1,-1,-1,-1]', 'revision': 9}]}
        after = {'accounts': [{**row, 'vault_mask': 1} for row in before['accounts']], 'vaults': [
            {'account': 1, 'chest': 0, 'items': '[-1,-1,-1,-1,-1,-1,-1,-1]', 'revision': 0},
            {**before['vaults'][0], 'chest': 0},
            {'account': 3, 'chest': 0, 'items': '[-1,-1,-1,-1,-1,-1,-1,-1]', 'revision': 0},
            {**before['vaults'][1], 'chest': 0},
            {'account': 5, 'chest': 0, 'items': '[-1,-1,-1,-1,-1,-1,-1,-1]', 'revision': 0}]}
        update.verify_migration(before, after)
        for rows in [after['vaults'][:1] + after['vaults'][2:],
                     after['vaults'][:2] + after['vaults'][3:],
                     [{**row, 'items': '[-1,-1,-1,-1,-1,-1,-1,-1]'} if row['account'] == 2 else row for row in after['vaults']],
                     [{**row, 'revision': 0} if row['account'] == 4 else row for row in after['vaults']],
                     after['vaults'] + [{'account': 2, 'chest': 1, 'items': '[-1,-1,-1,-1,-1,-1,-1,-1]', 'revision': 0}]]:
            with self.subTest(rows=rows), self.assertRaisesRegex(RuntimeError, 'Vault migration'):
                update.verify_migration(before, {**after, 'vaults': rows})

    def test_unpack_accepts_exact_files_and_rejects_hash_changes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive = root / 'server.tar.gz'
            contents = {'openrotmg': b'binary', 'REVISION': b'a' * 40 + b'\n'}
            with tarfile.open(archive, 'w:gz') as tar:
                for name, value in contents.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(value)
                    tar.addfile(info, io.BytesIO(value))
            metadata = {'revision': 'a' * 40, 'binary_sha256': hashlib.sha256(b'binary').hexdigest(),
                        'manifest': {k: hashlib.sha256(v).hexdigest() for k, v in contents.items()}}
            update.unpack(archive, root / 'valid', metadata)
            self.assertEqual((root / 'valid/openrotmg').stat().st_mode & 0o777, 0o755)
            metadata['manifest']['REVISION'] = '0' * 64
            with self.assertRaisesRegex(RuntimeError, 'manifest mismatch'):
                update.unpack(archive, root / 'invalid', metadata)

    def test_archive_traversal_and_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for index, (name, kind) in enumerate([('../escape', tarfile.REGTYPE), ('link', tarfile.SYMTYPE)]):
                archive = root / f'{index}.tar.gz'
                with tarfile.open(archive, 'w:gz') as tar:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = '/etc/passwd'
                    tar.addfile(member)
                with self.assertRaisesRegex(RuntimeError, 'Unsafe release'):
                    update.unpack(archive, root / str(index), {})

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
                        patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'deploy') as deploy:
                    with self.assertRaisesRegex(RuntimeError, error):
                        update.main('a' * 40, raw)
                    deploy.assert_not_called()
                    self.assertEqual(json.loads((root / 'state/status.json').read_text())['state'], 'failed')
        with patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'LIMIT', 1):
            with self.assertRaisesRegex(RuntimeError, 'exceeds'):
                update.main('a' * 40, raw)

    def test_up_to_date_requires_matching_artifact_process_and_health(self):
        raw, metadata = self.delivery()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'REVISION').write_text('a' * 40)
            config = root / 'config.json'
            config.write_text('{"bind":"127.0.0.1:18923"}')
            with patch.object(update, 'ROOT', root / 'state'), patch.object(update, 'LIVE', root), \
                    patch.object(update, 'CONFIG', config), patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update, 'files', return_value=metadata['manifest']), \
                    patch.object(update, 'digest', side_effect=[metadata['archive_sha256'], metadata['binary_sha256']]), \
                    patch.object(update.subprocess, 'check_output', return_value='123'), \
                    patch.object(update, 'ready', side_effect=RuntimeError('dead process')):
                with self.assertRaisesRegex(RuntimeError, 'dead process'):
                    update.main('a' * 40, raw)
                self.assertEqual(json.loads((root / 'state/status.json').read_text())['state'], 'activated_unverified')

    def test_successful_backup_starts_reset_the_retry_counter(self):
        with patch.object(update.subprocess, 'check_output', return_value='success'), patch.object(update, 'run') as run, patch.object(update, 'verify_backup') as verify:
            update.start_backup()
            self.assertEqual([call.args for call in run.call_args_list],
                             [('systemctl', 'reset-failed', 'openrotmg-backup.service'),
                              ('systemctl', 'start', 'openrotmg-backup.service')])
            verify.assert_called_once()

    def test_failed_backups_never_bypass_the_retry_limit(self):
        with patch.object(update.subprocess, 'check_output', return_value='exit-code'), patch.object(update, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'Previous backup failed'):
                update.start_backup()
            run.assert_not_called()

    def test_readonly_backup_lock_still_reaches_save_failure_before_exchange(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lock = root / 'backup.lock'
            lock.touch()
            lock.chmod(0o444)
            with patch.object(update, 'LAST_BACKUP', root / 'receipt.json'), patch.object(update, 'run'), patch.object(update, 'install_backups'), \
                    patch.object(update, 'rehearse'), patch.object(update, 'files', return_value={}), \
                    patch.object(update.subprocess, 'check_output', return_value='failed'), \
                    patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update.ctypes, 'CDLL') as exchange:
                with self.assertRaisesRegex(RuntimeError, 'Shutdown save failed'):
                    update.deploy(root, root, {'manifest': {}, 'revision': 'a' * 40}, {'state': str(root)})
                exchange.assert_not_called()


    def test_backup_tools_are_immutable_bundles_with_atomic_promotion(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / 'stage'
            (stage / 'scripts/backups').mkdir(parents=True)
            (stage / 'data').mkdir()
            (stage / 'data/objects.xml').write_text('matching release data')
            for name in ['backup.py', 'openrotmg-backup.sh']:
                (stage / 'scripts/backups' / name).write_text(name)
            tools = root / 'tools'
            tools.mkdir()
            with patch.object(update, 'BACKUPS', tools), patch.object(update.subprocess, 'check_output', return_value='inactive'), patch.object(update, 'run'), patch.object(update, 'start_backup'):
                for revision in ['a' * 40, 'b' * 40]:
                    (stage / 'REVISION').write_text(revision)
                    (stage / 'openrotmg').write_text(revision)
                    update.install_backups(stage)
                    self.assertEqual((tools / 'current').resolve(), tools / 'releases' / revision)
                self.assertEqual((tools / 'releases' / ('a' * 40) / 'openrotmg').read_text(), 'a' * 40)
                self.assertEqual((tools / 'current/data/objects.xml').read_text(), 'matching release data')
                (tools / 'current/openrotmg').write_text('tampered')
                with self.assertRaisesRegex(RuntimeError, 'differs from release'):
                    update.install_backups(stage)

    def test_failed_backup_is_not_reported_as_running(self):
        with patch.object(update.subprocess, 'check_output', return_value='failed'), patch.object(update, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'Backup service is failed'):
                update.install_backups(Path('/unused'))
            run.assert_not_called()

    def test_rehearsal_explicitly_disables_dialogue_without_changing_live_opt_in(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stage = root / 'stage'
            stage.mkdir()
            (stage / 'REVISION').write_text('a' * 40)
            config = {'state': str(root / 'live'), 'shore_bots': True, 'bot_llm': True}

            def inspect(command, **kwargs):
                shadow = json.loads(Path(command[-1]).read_text())
                self.assertFalse(shadow['shore_bots'])
                self.assertFalse(shadow['bot_llm'])
                self.assertTrue(config['bot_llm'])
                raise RuntimeError('rehearsal inspected')

            with patch.object(update, 'run'), patch.object(update, 'copy_db'), \
                    patch.object(update, 'rows', return_value={}), \
                    patch.object(update.shutil, 'copyfile', side_effect=lambda source, target: Path(target).touch()), \
                    patch.object(update.subprocess, 'Popen', side_effect=inspect):
                with self.assertRaisesRegex(RuntimeError, 'rehearsal inspected'):
                    update.rehearse(stage, root, config)

    def test_failed_write_rehearsal_cannot_install_tools_or_stop_production(self):
        with patch.object(update, 'run') as run, patch.object(update, 'rehearse', side_effect=RuntimeError('write contract failed')), patch.object(update, 'install_backups') as install:
            with self.assertRaisesRegex(RuntimeError, 'write contract failed'):
                update.deploy(Path('/stage'), Path('/work'), {}, {})
            install.assert_not_called()
            self.assertEqual(len(run.call_args_list), 1)

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

    def test_restart_notice_is_announced_and_awaited_before_the_cutover(self):
        order = []
        with patch.object(update.subprocess, 'check_output', return_value='active'), \
                patch.object(update, 'announce', side_effect=lambda config, text: order.append(('announce', text)) or {'players': 3}), \
                patch.object(update.time, 'sleep', side_effect=lambda seconds: order.append(('sleep', seconds))):
            self.assertTrue(update.warn_restart({'bind': '127.0.0.1:18923'}))
        self.assertEqual(order, [('announce', 'Server restarting in 20 seconds to apply an update.'), ('sleep', 20)])

    def test_inactive_service_is_never_announced(self):
        with patch.object(update.subprocess, 'check_output', return_value='inactive'), patch.object(update, 'announce') as announce, \
                patch.object(update.time, 'sleep') as sleep:
            self.assertFalse(update.warn_restart({'bind': '127.0.0.1:18923'}))
        announce.assert_not_called()
        sleep.assert_not_called()

    def test_rejected_announcement_blocks_the_cutover_instead_of_dropping_the_warning(self):
        with patch.object(update.subprocess, 'check_output', return_value='active'), \
                patch.object(update, 'announce', side_effect=RuntimeError('Announcement rejected: HTTP 403')), \
                patch.object(update.time, 'sleep') as sleep:
            with self.assertRaisesRegex(RuntimeError, 'Announcement rejected: HTTP 403'):
                update.warn_restart({'bind': '127.0.0.1:18923'})
        sleep.assert_not_called()

    def test_announcement_endpoint_errors_are_loud_but_an_old_server_still_restarts(self):
        with tempfile.TemporaryDirectory() as tmp:
            token = Path(tmp) / 'discord-link-token'
            token.write_text('a' * 64)

            def fail(code):
                return patch.object(update.urllib.request, 'urlopen', side_effect=urllib.error.HTTPError(
                    'http://127.0.0.1:18923/internal/discord/announce', code, 'rejected', {}, None))

            with patch.object(update, 'ANNOUNCE_TOKEN', token), fail(403):
                with self.assertRaisesRegex(RuntimeError, 'HTTP 403'):
                    update.announce({'bind': '127.0.0.1:18923'}, 'text')
            with patch.object(update, 'ANNOUNCE_TOKEN', token), fail(404) as urlopen:
                self.assertIsNone(update.announce({'bind': '127.0.0.1:18923'}, 'text'))
            # axum's Json extractor answers 415 without this header.
            self.assertEqual(urlopen.call_args.args[0].get_header('Content-type'), 'application/json')

    def test_deploy_warns_players_before_stopping_production(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'backup.lock').touch()
            order = []
            with patch.object(update, 'LAST_BACKUP', root / 'receipt.json'), patch.object(update, 'rehearse'), \
                    patch.object(update, 'install_backups'), patch.object(update, 'files', return_value={}), \
                    patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update, 'warn_restart', side_effect=lambda config: order.append('warn')), \
                    patch.object(update, 'run', side_effect=lambda *args: order.append(args)), \
                    patch.object(update.subprocess, 'check_output', return_value='failed'):
                with self.assertRaisesRegex(RuntimeError, 'Shutdown save failed'):
                    update.deploy(root, root, {'manifest': {}, 'revision': 'a' * 40}, {'state': str(root)})
            self.assertEqual(order[-2:], ['warn', ('systemctl', 'stop', 'openrotmg.service')])


if __name__ == '__main__':
    unittest.main()
