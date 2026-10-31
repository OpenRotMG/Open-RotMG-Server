import importlib.util
import json
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

if __name__ == '__main__':
    unittest.main()
