import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch, MagicMock
from zipfile import ZipFile, ZipInfo

spec = importlib.util.spec_from_file_location("launcher", Path(__file__).with_name("run.py"))
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)
resolve_selection = launcher.client_selection


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.revision = 'a' * 40
        self.prefix = f"Open-RotMG-Client-{self.revision}"
        self.url = f'https://codeload.github.com/OpenRotMG/Open-RotMG-Client/zip/{self.revision}'
        self.client = self.root / '.local' / self.prefix
        self.lock = {'version': '0.7.1-openrotmg.7', 'bundleSha256': 'b' * 64}
        self.data = self.archive({"client.swf": b"reference", "index.html": b"page", "scripts/run.py": b"prepare", "run.sh": b"#!/bin/sh\n",
                                  'ruffle/lock.json': json.dumps(self.lock).encode()})
        self.selection = {'client': self.revision, 'client_archive_sha256': hashlib.sha256(self.data).hexdigest(),
                          'runtime_version': self.lock['version'], 'runtime_bundle_sha256': self.lock['bundleSha256']}
        self.patch = patch.multiple(launcher, ROOT=self.root, LOG=self.root / ".local/server.log")
        self.patch.start()
        self.resolve = patch.object(launcher, 'client_selection', return_value=self.selection)
        self.resolve.start()
        self.git = patch.object(launcher.subprocess, 'check_output', side_effect=lambda command, **_: 'test-revision\n' if command[1] == 'rev-parse' else '')
        self.git.start()
        self.port = patch.object(launcher.socket, 'socket')
        self.port.start()

    def archive(self, files):
        stream = io.BytesIO()
        with ZipFile(stream, 'w') as archive:
            for name, content in files.items():
                info = ZipInfo(f'{self.prefix}/{name}')
                info.external_attr = (0o100755 if name == 'run.sh' else 0o100644) << 16
                archive.writestr(info, content)
        return stream.getvalue()

    def tearDown(self):
        self.port.stop()
        self.git.stop()
        self.resolve.stop()
        self.patch.stop()
        self.temp.cleanup()

    def test_download_then_verified_cache(self):
        with patch.object(launcher, "urlopen", return_value=io.BytesIO(self.data)) as download:
            self.assertEqual(launcher.prepare_client(), self.client)
            download.assert_called_once_with(self.url, timeout=60)
        self.assertEqual((self.client / 'run.sh').stat().st_mode & 0o777, 0o755)
        (self.client / 'run.sh').chmod(0o644)
        with patch.object(launcher, "urlopen", side_effect=AssertionError("no silent redownload")):
            self.assertEqual(launcher.prepare_client(), self.client)
        self.assertEqual((self.client / 'index.html').read_bytes(), b'page')
        self.assertEqual((self.client / 'run.sh').stat().st_mode & 0o777, 0o755)

    def test_default_checks_deployed_selection_even_with_a_cached_client(self):
        def response(request, **_):
            if isinstance(request, launcher.Request):
                self.assertEqual(request.full_url, 'https://openrotmg.org/diagnostics-build.json')
                self.assertEqual(request.get_header('Cache-control'), 'no-cache')
                return io.BytesIO(json.dumps(self.selection).encode())
            self.assertEqual(request, self.url)
            return io.BytesIO(self.data)
        with patch.object(launcher, 'client_selection', side_effect=resolve_selection), \
                patch.object(launcher, 'urlopen', side_effect=response) as download:
            self.assertEqual(launcher.prepare_client(), self.client)
            self.assertEqual(launcher.prepare_client(), self.client)
        self.assertEqual([getattr(call.args[0], 'full_url', call.args[0]) for call in download.call_args_list],
                         [launcher.CLIENT_SELECTION_URL, self.url, launcher.CLIENT_SELECTION_URL])
        self.assertEqual(json.loads((self.client.parent / 'client-selection.json').read_text()), self.selection)
        self.assertIn(self.lock['version'], launcher.LOG.read_text())

    def test_corrupt_download_and_archive_cache_fail(self):
        cache = self.root / f'.local/client-{self.revision}.zip'
        with patch.object(launcher, "urlopen", return_value=io.BytesIO(b"corrupt")):
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                launcher.prepare_client()
        self.assertFalse(cache.exists())
        cache.write_bytes(b"corrupt")
        with patch.object(launcher, "urlopen", side_effect=AssertionError("no silent redownload")):
            with self.assertRaisesRegex(RuntimeError, "checksum mismatch"):
                launcher.prepare_client()
        self.assertEqual(cache.read_bytes(), b"corrupt")

    def test_modified_cached_assets_fail_without_replacement(self):
        with patch.object(launcher, "urlopen", return_value=io.BytesIO(self.data)):
            launcher.prepare_client()
        (self.client / 'index.html').write_text('modified')
        with self.assertRaisesRegex(RuntimeError, 'missing or modified'):
            launcher.prepare_client()
        self.assertEqual((self.client / 'index.html').read_text(), 'modified')

    def test_archive_traversal_and_symlinks_are_rejected(self):
        stream = io.BytesIO()
        with ZipFile(stream, 'w') as archive:
            link = ZipInfo(f'{self.prefix}/link')
            link.external_attr = 0o120777 << 16
            archive.writestr(link, '../../outside')
        for data in [self.archive({'../../outside': b'bad'}), stream.getvalue()]:
            with patch.dict(self.selection, client_archive_sha256=hashlib.sha256(data).hexdigest()), \
                    patch.object(launcher, 'urlopen', return_value=io.BytesIO(data)):
                with self.assertRaisesRegex(RuntimeError, 'Unsafe client archive'):
                    launcher.prepare_client()
        self.assertFalse((self.root / 'outside').exists())

    def test_ports_and_custom_client_are_forwarded(self):
        (self.client / 'scripts').mkdir(parents=True)
        (self.client / 'scripts/run.py').touch()
        cases = [(['8080'], ['--port', '8080']),
                 (['1', '--dev-control'], ['--port', '1', '--dev-control']),
                 (['--local-client', str(self.client), '65535'], ['--port', '65535'])]
        for args, expected in cases:
            with self.subTest(args=args), patch('sys.argv', ['run.py', *args]), \
                    patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                    patch.object(launcher, 'prepare_client', return_value=self.client) as download, \
                    patch.object(launcher, 'run', return_value=0) as run:
                self.assertEqual(launcher.main(), 0)
                self.assertEqual(run.call_count, 2)
                self.assertEqual(run.call_args.args[0], ['cargo', 'run', '--locked', '--', '--client', str(self.client), *expected[:2], '--state', str(self.root / '.local'), *expected[2:]])
                if '--local-client' in args:
                    download.assert_not_called()

    def test_optimized_bots_keep_state_isolation_and_qa_disabled(self):
        (self.client / 'scripts').mkdir(parents=True)
        (self.client / 'scripts/run.py').touch()
        state = self.root / 'bots'
        for flag in ['--shore-bots', '--no-shore-bots']:
            with self.subTest(flag=flag), patch('sys.argv', ['run.py', '8081', '--client', str(self.client), '--state', str(state), flag, '--release']), \
                    patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                    patch.object(launcher, 'run', return_value=0) as run:
                self.assertEqual(launcher.main(), 0)
            self.assertEqual(run.call_args.args[0], ['cargo', 'run', '--locked', '--release', '--', '--client', str(self.client), '--port', '8081', '--state', str(state), flag])
        with patch('sys.argv', ['run.py', '8081', '--shore-bots', '--no-shore-bots']), self.assertRaises(SystemExit) as error:
            launcher.main()
        self.assertEqual(error.exception.code, 2)

    def test_explicit_state_is_used_for_saves_cache_and_logs(self):
        state = self.root / 'isolated'
        with patch('sys.argv', ['run.py', '8081', '--state', str(state)]), \
                patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                patch.object(launcher, 'urlopen', return_value=io.BytesIO(self.data)), \
                patch.object(launcher, 'run', return_value=0) as run:
            launcher.main()
        self.assertEqual((run.call_args.args[0][-2:], launcher.LOG.parent,
                          (state / f'client-{self.revision}.zip').exists(),
                          (self.root / '.local').exists()),
                         (['--state', str(state)], state, True, False))

    def test_client_alias_is_explicit_and_receipt_identifies_environment(self):
        (self.client / 'scripts').mkdir(parents=True)
        (self.client / 'scripts/run.py').touch()
        with patch('sys.argv', ['run.py', '8081', '--client', str(self.client)]), \
                patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                patch.object(launcher, 'run', return_value=0), \
                patch.object(launcher, 'prepare_client', side_effect=AssertionError('custom client must not download')):
            launcher.main()
        self.assertIn(f'Local source: test-revision dirty=False; port=8081; state={self.root / ".local"}; client={self.client}', launcher.LOG.read_text())

    def test_occupied_port_fails_before_download(self):
        with patch('sys.argv', ['run.py', '8081']), \
                patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                patch.object(launcher.socket, 'socket', side_effect=OSError('address already in use')), \
                patch.object(launcher, 'prepare_client', side_effect=AssertionError('must fail before download')):
            launcher.main()
        self.assertIn('Cannot listen on 127.0.0.1:8081', launcher.LOG.read_text())

    def test_missing_port_fails_before_setup(self):
        for args in [[], ['--dev-control'], ['--local-client', str(self.client)]]:
            with self.subTest(args=args), patch('sys.argv', ['run.py', *args]), \
                    patch.object(launcher.shutil, 'which') as cargo, \
                    patch.object(launcher, 'prepare_client') as prepare, \
                    patch('sys.stderr', new_callable=io.StringIO) as stderr:
                with self.assertRaises(SystemExit) as error:
                    launcher.main()
                self.assertEqual(error.exception.code, 2)
                self.assertIn('usage: run.sh', stderr.getvalue())
                self.assertIn('required: port', stderr.getvalue())
                cargo.assert_not_called()
                prepare.assert_not_called()
                self.assertFalse(launcher.LOG.exists())

    def test_invalid_ports_fail_before_download(self):
        for port in ['0', '-1', '65536', 'abc', '8080.5']:
            with self.subTest(port=port), patch('sys.argv', ['run.py', port]), \
                    patch.object(launcher, 'prepare_client') as prepare, patch('sys.stderr', new_callable=io.StringIO) as stderr:
                with self.assertRaises(SystemExit) as error:
                    launcher.main()
                self.assertEqual(error.exception.code, 2)
                self.assertIn('port must be', stderr.getvalue())
                prepare.assert_not_called()

    def test_failed_download_logs_url_and_cause(self):
        with patch('sys.argv', ['run.py', '8080']), patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                patch.object(launcher, 'urlopen', side_effect=OSError('connection refused')):
            self.assertEqual(launcher.main(), 1)
        self.assertIn(self.url, launcher.LOG.read_text())
        self.assertIn('connection refused', launcher.LOG.read_text())

    def test_failed_preparation_does_not_start_backend(self):
        (self.client / 'scripts').mkdir(parents=True)
        (self.client / 'scripts/run.py').touch()
        with patch('sys.argv', ['run.py', '8080']), patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                patch.object(launcher, 'prepare_client', return_value=self.client), patch.object(launcher, 'run', return_value=37) as run:
            self.assertEqual(launcher.main(), 1)
            self.assertEqual(run.call_count, 1)
        self.assertIn('status 37', launcher.LOG.read_text())

    def test_manifest_failure_never_uses_a_stale_cached_client(self):
        with patch.object(launcher, 'urlopen', return_value=io.BytesIO(self.data)):
            launcher.prepare_client()
        with patch.object(launcher, 'client_selection', side_effect=resolve_selection), \
                patch.object(launcher, 'urlopen', side_effect=OSError('offline')):
            with self.assertRaisesRegex(RuntimeError, 'Resolve deployed client.*offline'):
                launcher.prepare_client()
        self.assertEqual((self.client / 'index.html').read_bytes(), b'page')

    def test_manifest_validates_revision_checksums_and_runtime(self):
        for field in self.selection:
            for bad in [None, '', '../main', 'latest', 7]:
                with self.subTest(field=field, bad=bad), patch.object(launcher, 'urlopen', return_value=io.BytesIO(json.dumps({**self.selection, field: bad}).encode())):
                    with self.assertRaisesRegex(RuntimeError, 'Invalid deployed client selection: ' + field):
                        resolve_selection()
        with patch.object(launcher, 'urlopen', return_value=io.BytesIO(b'{broken')):
            with self.assertRaisesRegex(RuntimeError, 'Resolve deployed client'):
                resolve_selection()

    def test_new_deployment_selects_a_new_immutable_cache_without_changing_old_client(self):
        with patch.object(launcher, 'urlopen', return_value=io.BytesIO(self.data)):
            launcher.prepare_client()
        old_client = self.client
        self.prefix = 'Open-RotMG-Client-' + 'c' * 40
        data = self.archive({'client.swf': b'next', 'index.html': b'next-page', 'scripts/run.py': b'prepare',
                             'run.sh': b'#!/bin/sh\n', 'ruffle/lock.json': json.dumps(self.lock).encode()})
        with patch.dict(self.selection, client='c' * 40, client_archive_sha256=hashlib.sha256(data).hexdigest()), \
                patch.object(launcher, 'urlopen', return_value=io.BytesIO(data)):
            new_client = launcher.prepare_client()
        self.assertNotEqual(old_client, new_client)
        self.assertEqual((old_client / 'index.html').read_bytes(), b'page')
        self.assertEqual((new_client / 'index.html').read_bytes(), b'next-page')

    def test_saved_manifest_is_an_explicit_offline_revision_override(self):
        manifest = self.root / 'saved-selection.json'
        manifest.write_text(json.dumps(self.selection))
        with patch.object(launcher, 'urlopen', return_value=io.BytesIO(self.data)):
            launcher.prepare_client()
        with patch.object(launcher, 'client_selection', side_effect=resolve_selection), \
                patch.object(launcher, 'urlopen', side_effect=AssertionError('explicit verified cache needs no network')):
            self.assertEqual(launcher.prepare_client(manifest), self.client)
        with patch('sys.argv', ['run.py', '8081', '--client', str(self.client), '--client-manifest', str(manifest)]), \
                patch('sys.stderr', new_callable=io.StringIO), self.assertRaises(SystemExit):
            launcher.main()

    def test_cli_saved_manifest_is_forwarded_once_for_the_whole_session(self):
        manifest = self.root / 'saved-selection.json'
        with patch('sys.argv', ['run.py', '8081', '--client-manifest', str(manifest)]), \
                patch.object(launcher, 'prepare_client', return_value=self.client) as prepare, \
                patch.object(launcher, 'run', return_value=0), \
                patch.object(launcher.shutil, 'which', return_value='/cargo'):
            (self.client / 'scripts').mkdir(parents=True)
            (self.client / 'scripts/run.py').write_text('prepare')
            self.assertEqual(launcher.main(), 0)
        prepare.assert_called_once_with(manifest)

    def test_manifest_runtime_must_match_archive_before_executing_its_launcher(self):
        with patch.dict(self.selection, runtime_version='0.7.1-openrotmg.8'), \
                patch.object(launcher, 'urlopen', return_value=io.BytesIO(self.data)):
            with self.assertRaisesRegex(RuntimeError, 'runtime lock does not match'):
                launcher.prepare_client()
        self.assertFalse(self.client.exists())

    def test_cached_runtime_lock_cannot_drift(self):
        with patch.object(launcher, 'urlopen', return_value=io.BytesIO(self.data)):
            launcher.prepare_client()
        (self.client / 'ruffle/lock.json').write_text('{}')
        with self.assertRaisesRegex(RuntimeError, 'missing or modified'):
            launcher.prepare_client()
        self.assertEqual((self.client / 'ruffle/lock.json').read_text(), '{}')

    def test_backend_failure_is_printed_and_logged(self):
        (self.client / 'scripts').mkdir(parents=True)
        (self.client / 'scripts/run.py').touch()
        prepare = MagicMock()
        prepare.__enter__.return_value = prepare
        prepare.stdout = []
        prepare.wait.return_value = 0
        backend = MagicMock()
        backend.__enter__.return_value = backend
        backend.stdout = ['Error: cannot listen on 127.0.0.1:18923\n']
        backend.wait.return_value = 1
        with patch('sys.argv', ['run.py', '8080']), patch.object(launcher.shutil, 'which', return_value='/cargo'), \
                patch.object(launcher, 'prepare_client', return_value=self.client), \
                patch.object(launcher.subprocess, 'Popen', side_effect=[prepare, backend]):
            self.assertEqual(launcher.main(), 1)
        self.assertIn('cannot listen', launcher.LOG.read_text())
        self.assertIn('status 1', launcher.LOG.read_text())


if __name__ == '__main__':
    unittest.main()
