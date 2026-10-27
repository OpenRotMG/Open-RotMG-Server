import hashlib
import importlib.util
import io
import contextlib
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
                        patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'deploy') as deploy:
                    with self.assertRaisesRegex(RuntimeError, error):
                        update.main('a' * 40, raw)
                    deploy.assert_not_called()
                    self.assertEqual(json.loads((root / 'state/status.json').read_text())['state'], 'failed')
        with patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'LIMIT', 1):
            with self.assertRaisesRegex(RuntimeError, 'exceeds'):
                update.main('a' * 40, raw)











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
