import importlib.util
import json
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('profile_recorder', Path(__file__).with_name('profile.py'))
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


class ProfileTests(unittest.TestCase):
    def test_unstructured_panic_is_explicit_and_not_a_fake_incident(self):
        with patch.object(profile, 'emit') as emit:
            self.assertIsNone(profile.incident(json.dumps({'_PID': '42', 'MESSAGE': 'thread panicked'}), 42))
            emit.assert_called_once_with('unstructured_target_log', pid=42)

    def test_only_slow_ticks_trigger_captures(self):
        now = profile.time.monotonic_ns()
        def line(kind):
            sample = {'kind': kind, 'execution': {'tid': 7, 'start_ns': now - 60_000_000, 'end_ns': now}}
            fields = {'message': 'latency incident', 'pid': 42, 'clock': 'CLOCK_MONOTONIC', 'world': 1, 'sample': json.dumps(sample)}
            return json.dumps({'_PID': '42', 'MESSAGE': json.dumps({'timestamp': 'now', 'fields': fields})})
        self.assertIsNone(profile.incident(line('job'), 42))
        self.assertEqual(profile.incident(line('tick'), 42)['sample']['kind'], 'tick')

    def test_over_budget_start_fails_before_creating_a_session(self):
        # A session created by a start that cannot fit is retained on failure, so the next start fails too.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve() / 'profiles'
            root.mkdir(mode=0o700)
            with patch.object(profile, 'enforce_storage_limit', side_effect=RuntimeError('over budget')), \
                    patch.object(profile.subprocess, 'check_output', side_effect=AssertionError('session started')):
                with self.assertRaisesRegex(RuntimeError, 'over budget'):
                    profile.run('openrotmg', root)
            self.assertEqual(sorted(p.name for p in root.iterdir()), ['.lock', 'owner.json'])

if __name__ == '__main__':
    unittest.main()
