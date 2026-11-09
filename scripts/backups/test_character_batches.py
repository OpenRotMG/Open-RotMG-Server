import json
import subprocess
import unittest
from unittest.mock import patch
import backup


class CharacterBatches(unittest.TestCase):
    def test_bounded_batches_preserve_every_record_and_legacy_mode(self):
        values = [{'index': i, 'text': '\u2603' * 350000} for i in range(3)]
        batches = []

        def validate(command, *, input, **kwargs):
            self.assertEqual(command, ['/release/openrotmg', '--backup-validate', 'legacy'])
            self.assertLessEqual(len(input.encode()), 4 * 1024 * 1024)
            rows = json.loads(input)
            batches.append(rows)
            return subprocess.CompletedProcess(command, 0, json.dumps({'validated_characters': len(rows)}), '')

        with patch.dict('os.environ', OPENROTMG_BACKUP_BINARY='/release/openrotmg'), patch.object(backup.subprocess, 'run', side_effect=validate):
            backup.characters(values, legacy=True)
        self.assertEqual(len(batches), 3)
        self.assertEqual([row for batch in batches for row in batch], values)

    def test_later_batch_failure_and_invalid_receipt_are_not_ignored(self):
        values = [{'text': 'x' * (3 * 1024 * 1024)}] * 2
        for failure in [subprocess.CompletedProcess([], 1, '', 'Unsupported character payload'),
                        subprocess.CompletedProcess([], 0, '{"validated_characters":0}', '')]:
            with self.subTest(failure=failure), patch.dict('os.environ', OPENROTMG_BACKUP_BINARY='/release/openrotmg'), patch.object(backup.subprocess, 'run', side_effect=[subprocess.CompletedProcess([], 0, '{"validated_characters":1}', ''), failure]) as run:
                with self.assertRaises(ValueError):
                    backup.characters(values)
                self.assertEqual(run.call_count, 2)

    def test_empty_input_and_oversized_single_record(self):
        with patch.dict('os.environ', OPENROTMG_BACKUP_BINARY='/release/openrotmg'), patch.object(backup.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, '{"validated_characters":0}', '')) as run:
            backup.characters([])
            self.assertEqual(run.call_args.kwargs['input'], '[]')
            run.reset_mock()
            with self.assertRaisesRegex(ValueError, '4 MiB'):
                backup.characters([{'text': 'x' * (4 * 1024 * 1024)}])
            run.assert_not_called()
