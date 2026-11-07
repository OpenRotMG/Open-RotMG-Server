import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import test_update
from test_update import update

ACTOR = '175037041837473797'
RECEIPT = {'instance': 'a' * 64, 'owner': 'b' * 64, 'deadline_ms': 90000}


class RestartTest(unittest.TestCase):
    def test_countdown_checks_exact_receipt_and_fails_closed(self):
        pending = {'state': 'pending', 'receipt': RECEIPT, 'actor_id': None}
        for result in [pending, {**pending, 'state': 'cancelled', 'actor_id': ACTOR}, OSError('offline')]:
            with self.subTest(result=result), patch.object(update.subprocess, 'check_output', return_value='active'), \
                    patch.object(update, 'drain_process', return_value={}), \
                    patch.object(update, 'announce', return_value={'players': 2}), \
                    patch.object(update.time, 'monotonic', side_effect=[0, 0, 0, 90]), \
                    patch.object(update.time, 'sleep'), \
                    patch.object(update, 'restart_request', side_effect=[pending, result]) as request, \
                    contextlib.redirect_stdout(io.StringIO()):
                if result == pending:
                    self.assertEqual(update.warn_restart({}), {'process': {}, 'receipt': RECEIPT})
                else:
                    with self.assertRaises(update.RestartCancelled if isinstance(result, dict) else OSError):
                        update.warn_restart({})
                self.assertEqual(request.call_args.args, ({}, {}, 'status'))
                self.assertEqual(request.call_args.kwargs, {'receipt': RECEIPT})

    def test_cancel_during_countdown_or_final_consume_never_stops_or_exchanges(self):
        for warning in [update.RestartCancelled(ACTOR), {'process': {}, 'receipt': RECEIPT}]:
            with self.subTest(warning=warning), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'backup.lock').touch()
                with patch.object(update, 'LAST_BACKUP', root / 'backup.json'), \
                        patch.object(update, 'run'), patch.object(update, 'rehearse'), \
                        patch.object(update, 'install_backups'), patch.object(update, 'files', return_value={}), \
                        patch.object(update, 'main_revision', return_value='a' * 40), \
                        patch.object(update, 'warn_restart', side_effect=warning if isinstance(warning, Exception) else None, return_value=warning), \
                        patch.object(update, 'restart_request', return_value={'state': 'cancelled', 'actor_id': ACTOR}), \
                        patch.object(update, 'announce', return_value={'players': 2}) as announce, \
                        patch.object(update, 'stop_prepared') as stop, patch.object(update, 'exchange') as exchange, \
                        contextlib.redirect_stdout(io.StringIO()) as output:
                    with self.assertRaisesRegex(update.RestartCancelled, ACTOR):
                        update.deploy(root / 'stage', root, {'revision': 'a' * 40, 'manifest': {}}, {})
                    stop.assert_not_called()
                    exchange.assert_not_called()
                    announce.assert_called_once()
                    self.assertEqual(json.loads(output.getvalue())['restart_cancelled_by'], ACTOR)

    def test_cancelled_deployment_exits_without_installing_or_claiming_the_candidate(self):
        raw, _ = test_update.UpdateTest().delivery()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            live = root / 'live'
            live.mkdir()
            (live / 'REVISION').write_text('b' * 40)
            config = root / 'config.json'
            config.write_text('{}')
            with patch.object(update, 'ROOT', root / 'deploy'), patch.object(update, 'LIVE', live), \
                    patch.object(update, 'CONFIG', config), patch.object(update, 'main_revision', return_value='a' * 40), \
                    patch.object(update, 'retain_deployments'), patch.object(update, 'unpack'), \
                    patch.object(update, 'deploy', side_effect=update.RestartCancelled(ACTOR)), \
                    patch.object(update, 'install_helper') as helper, patch.object(update, 'install_caddy') as caddy, \
                    contextlib.redirect_stdout(io.StringIO()):
                with self.assertRaises(update.RestartCancelled):
                    update.main('a' * 40, raw)
                receipt = json.loads((root / 'deploy/status.json').read_text())
                self.assertEqual(receipt['state'], 'cancelled')
                self.assertEqual(receipt['running'], 'b' * 40)
                self.assertEqual(receipt['actor_id'], ACTOR)
                helper.assert_not_called()
                caddy.assert_not_called()

    def test_process_replacement_stale_receipt_and_bad_acknowledgements_are_errors(self):
        good = {'state': 'consumed', 'receipt': RECEIPT, 'actor_id': None}
        for processes, reply in [([1, 2], good), ([2], good), ([1, 1], {**good, 'receipt': {**RECEIPT, 'owner': 'c' * 64}}), ([1, 1], {}), ([1, 1], {**good, 'state': 'cancelled'})]:
            with self.subTest(processes=processes, reply=reply), \
                    patch.object(update, 'drain_process', side_effect=processes), \
                    patch.object(update, 'lifecycle_request', return_value=reply):
                with self.assertRaises(RuntimeError):
                    update.restart_request({}, 1, 'consume', receipt=RECEIPT)
