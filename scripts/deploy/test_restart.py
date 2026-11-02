import contextlib
import importlib.util
import io
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
import threading
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('update', Path(__file__).with_name('update.py'))
update = importlib.util.module_from_spec(spec)
spec.loader.exec_module(update)

PROCESS = {'pid': 12, 'start_ticks': 123, 'binary_sha256': 'b' * 64}
RECEIPT = {'instance': 'a' * 64, 'owner': 'c' * 64, 'deadline_ms': 90000}
WARNING = {'process': PROCESS, 'receipt': RECEIPT}


class RestartTest(unittest.TestCase):
    def test_real_warning_binds_countdown_after_announcement_and_waits_original_budget(self):
        events = []
        with patch.object(update.subprocess, 'check_output', return_value='active'), \
                patch.object(update, 'drain_process', return_value=PROCESS), \
                patch.object(update, 'checked_restart_request', create=True,
                             side_effect=lambda *a: events.append(('begin', a)) or {'receipt': RECEIPT, 'state': 'pending'}), \
                patch.object(update, 'announce', side_effect=lambda *a: events.append(('announce', a)) or {'players': 2}), \
                patch.object(update.time, 'sleep', side_effect=lambda t: events.append(('sleep', t))), \
                contextlib.redirect_stdout(io.StringIO()):
            warning = update.warn_restart({'bind': '127.0.0.1:18923'})
        self.assertEqual(warning, WARNING)
        self.assertEqual([e[0] for e in events], ['announce', 'begin', 'sleep'])
        self.assertEqual(events[-1], ('sleep', 90))

    def test_cancelled_invocation_never_reaches_cutover_or_stop(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'backup.lock').touch()
            with patch.object(update, 'LAST_BACKUP', root / 'last-success.json'), \
                    patch.object(update, 'run'), patch.object(update, 'rehearse'), \
                    patch.object(update, 'install_backups'), patch.object(update, 'files', return_value={}), \
                    patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update, 'warn_restart', return_value=WARNING), \
                    patch.object(update, 'checked_restart_request', create=True, return_value={'receipt': RECEIPT, 'state': 'cancelled'}) as consume, \
                    patch.object(update, 'outcome', side_effect=AssertionError('cancelled deployment reached cutover')), \
                    patch.object(update, 'stop_prepared') as stop, patch.object(update, 'exchange') as exchange:
                with self.assertRaisesRegex(RuntimeError, 'cancelled'):
                    update.deploy(root / 'stage', root, {'revision': 'a' * 40, 'manifest': {}}, {'bind': '127.0.0.1:18923'})
                consume.assert_called_once_with({'bind': '127.0.0.1:18923'}, PROCESS, 'consume', RECEIPT)
                stop.assert_not_called()
                exchange.assert_not_called()

    def test_real_countdown_transport_binds_receipt_auth_and_process_without_redirects(self):
        requests = []
        reply = {'state': 'consumed', 'receipt': RECEIPT}

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                requests.append((self.path, self.headers['Authorization'], body))
                if reply['state'] == 'redirect':
                    self.send_response(302)
                    self.send_header('Location', 'http://127.0.0.1:9/')
                    self.end_headers()
                    return
                value = reply.copy()
                if body['operation'] == 'begin':
                    value = {'state': 'pending', 'receipt': {**RECEIPT, 'owner': body['owner']}}
                self.send_response(200)
                self.end_headers()
                self.wfile.write(json.dumps(value).encode())

        with HTTPServer(('127.0.0.1', 0), Handler) as server, tempfile.TemporaryDirectory() as directory:
            token = Path(directory) / 'token'
            token.write_text('local-test-credential')
            thread = threading.Thread(target=server.serve_forever)
            thread.start()
            config = {'bind': '127.0.0.1:' + str(server.server_port)}
            try:
                with patch.object(update, 'ANNOUNCE_TOKEN', token), patch.object(update, 'drain_process', return_value=PROCESS):
                    self.assertEqual(update.checked_restart_request(config, PROCESS, 'begin')['state'], 'pending')
                    self.assertEqual(update.checked_restart_request(config, PROCESS, 'consume', RECEIPT), reply)
                    reply['state'] = 'cancelled'
                    self.assertEqual(update.checked_restart_request(config, PROCESS, 'consume', RECEIPT)['state'], 'cancelled')
                    for change, error in [({'state': 'pending'}, 'Invalid restart state'),
                                          ({'receipt': {**RECEIPT, 'owner': 'd' * 64}}, 'Stale restart receipt'),
                                          ({'receipt': {}}, 'Invalid restart receipt')]:
                        reply.clear()
                        reply.update({'state': 'consumed', 'receipt': RECEIPT, **change})
                        with self.assertRaisesRegex(RuntimeError, error):
                            update.checked_restart_request(config, PROCESS, 'consume', RECEIPT)
                    reply['state'] = 'redirect'
                    with self.assertRaisesRegex(RuntimeError, 'Restart endpoint redirected'):
                        update.checked_restart_request(config, PROCESS, 'consume', RECEIPT)
                for identities in [[{}, PROCESS], [PROCESS, {}]]:
                    reply.clear()
                    reply.update({'state': 'consumed', 'receipt': RECEIPT})
                    with patch.object(update, 'ANNOUNCE_TOKEN', token), patch.object(update, 'drain_process', side_effect=identities):
                        with self.assertRaisesRegex(RuntimeError, 'process replaced'):
                            update.checked_restart_request(config, PROCESS, 'consume', RECEIPT)
            finally:
                server.shutdown()
                thread.join()
        self.assertTrue(all(path == '/internal/lifecycle/restart' and auth == 'Bearer local-test-credential' for path, auth, _ in requests))
        self.assertEqual(requests[1][2], {'operation': 'consume', 'receipt': RECEIPT})

    def test_main_records_cancellation_not_success_and_never_installs_or_activates(self):
        from test_update import UpdateTest
        raw, _ = UpdateTest().delivery()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live = root / 'server'
            live.mkdir()
            (live / 'REVISION').write_text('b' * 40)
            config = root / 'config.json'
            config.write_text(json.dumps({'bind': '127.0.0.1:18923'}))
            with patch.object(update, 'ROOT', root / 'state'), patch.object(update, 'LIVE', live), \
                    patch.object(update, 'CONFIG', config), patch.object(update, 'retain_deployments'), \
                    patch.object(update, 'main_revision', return_value='a' * 40), patch.object(update, 'unpack'), \
                    patch.object(update, 'deploy', side_effect=update.RestartCancelled('restart cancelled')), \
                    patch.object(update, 'install_caddy') as caddy, patch.object(update, 'install_helper') as helper:
                with self.assertRaisesRegex(update.RestartCancelled, 'restart cancelled'):
                    update.main('a' * 40, raw)
                status = json.loads((root / 'state/status.json').read_text())
                self.assertEqual(status['state'], 'cancelled')
                outcome = json.loads(next((root / 'state').glob('*/outcome.json')).read_text())
                self.assertEqual(outcome['state'], 'failed')
                self.assertEqual(outcome['reason'], 'restart cancelled')
                self.assertEqual((live / 'REVISION').read_text(), 'b' * 40)
                caddy.assert_not_called()
                helper.assert_not_called()


if __name__ == '__main__':
    unittest.main()
