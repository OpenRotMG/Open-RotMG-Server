import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPT = Path(__file__).with_name('qa.py').resolve()
spec = importlib.util.spec_from_file_location('qa', SCRIPT)
qa = importlib.util.module_from_spec(spec)
spec.loader.exec_module(qa)


def eventually(check, timeout=15):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = check()
        if value:
            return value
        time.sleep(0.05)
    raise AssertionError('Timed out waiting for owned QA job')


def space(directory):
    return qa.scratch_path(directory, qa.load(directory))


class JobTests(unittest.TestCase):
    def setUp(self):
        self.base = Path(tempfile.mkdtemp(prefix='lifecycle-', dir=os.environ.get('QA_RESULTS_DIR')))
        self.short = Path(tempfile.mkdtemp(prefix='oq-', dir='/var/tmp'))
        self.env = {**os.environ, 'XDG_STATE_HOME': str(self.base), 'OPENROTMG_QA_SCRATCH_ROOT': str(self.short / 'q')}
        self.jobs = self.base / 'openrotmg/qa-jobs'
        self.addCleanup(self.stop_jobs)

    def stop_jobs(self):
        if self.jobs.exists():
            for directory in self.jobs.iterdir():
                if qa.ID.fullmatch(directory.name) and (directory / 'job.json').exists():
                    row = json.loads((directory / 'job.json').read_text())
                    if qa.unit_state(row['unit'])['ActiveState'] in ('activating', 'active'):
                        subprocess.run(['systemctl', '--user', 'stop', row['unit']], check=True, capture_output=True)
                    if 'cgroup' in row:
                        eventually(lambda: not qa.group_processes(row['cgroup']))
        shutil.rmtree(self.short)

    def invoke(self, *args, **kwargs):
        return subprocess.run([sys.executable, str(SCRIPT), *args], env=self.env, capture_output=True, text=True, timeout=45, **kwargs)

    def launch(self, code, *options):
        result = self.invoke(*options, 'exec', '--', sys.executable, '-c', code)
        self.assertIn('QA job: ', result.stdout, result.stderr)
        directory = Path(result.stdout.split('QA job: ', 1)[1].splitlines()[0])
        return result, directory

    def receipt(self, directory):
        path = directory / 'collection.json'
        eventually(path.exists)
        return json.loads(path.read_text())

    def test_cancel_reaps_command_without_interrupting_its_cleanup_children(self):
        code = """import os,pathlib,signal,subprocess,sys,time
root=pathlib.Path(os.environ['QA_RESULTS_DIR'])
helper=\"import os,pathlib,signal,sys,time; root=pathlib.Path(os.environ['QA_RESULTS_DIR']); signal.signal(signal.SIGTERM,lambda *_:(root.joinpath('wrong-signal').touch(),sys.exit(61))); signal.signal(signal.SIGINT,lambda *_:sys.exit(0)); root.joinpath('helper-ready').touch(); time.sleep(60)\"
child=subprocess.Popen([sys.executable,'-c',helper])
def stop(*_):
    child.send_signal(signal.SIGINT)
    assert child.wait(timeout=3)==0
    root.joinpath('graceful').touch()
    sys.exit(23)
signal.signal(signal.SIGTERM,stop)
while not root.joinpath('helper-ready').exists(): time.sleep(.01)
root.joinpath('ready').touch()
time.sleep(60)
"""
        for timed in (False, True):
            with self.subTest(runtime_deadline=timed):
                options = ('--background', '--timeout', '5') if timed else ('--background',)
                result, directory = self.launch(code, *options)
                self.assertEqual(result.returncode, 0, result.stderr)
                eventually(lambda: (directory / 'results/ready').exists())
                if not timed:
                    cancelled = self.invoke('cancel', str(directory))
                    self.assertEqual(cancelled.returncode, 0, cancelled.stderr)
                eventually(lambda: (directory / 'command.json').exists())
                self.assertEqual(json.loads((directory / 'command.json').read_text())['returncode'], 23)
                self.assertTrue((directory / 'results/graceful').exists())
                self.assertFalse((directory / 'results/wrong-signal').exists())
                self.assertEqual(self.receipt(directory)['status'], 'reclaimed')
                self.assertFalse(space(directory).exists())

    def test_cancel_completed_command_retires_its_remaining_cgroup(self):
        result, directory = self.launch("import subprocess,sys; subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'])", '--background')
        self.assertEqual(result.returncode, 0, result.stderr)
        eventually(lambda: (directory / 'command.json').exists())
        self.assertEqual(json.loads((directory / 'command.json').read_text())['returncode'], 0)
        cancelled = self.invoke('cancel', str(directory))
        self.assertEqual(cancelled.returncode, 0, cancelled.stderr)
        self.assertEqual(self.receipt(directory)['status'], 'reclaimed')
        self.assertFalse(space(directory).exists())

    def test_cancel_crashed_worker_closes_children_without_inventing_command_return(self):
        result, directory = self.launch("import os,pathlib,time; pathlib.Path(os.environ['QA_RESULTS_DIR'],'ready').touch(); time.sleep(60)", '--background')
        self.assertEqual(result.returncode, 0, result.stderr)
        eventually(lambda: (directory / 'results/ready').exists())
        unit = qa.load(directory)['unit']
        subprocess.run(['systemctl', '--user', 'kill', '--kill-whom=main', '--signal=SIGKILL', unit], check=True, capture_output=True)
        eventually(lambda: qa.unit_state(unit)['MainPID'] == '0')
        self.assertFalse((directory / 'command.json').exists())
        cancelled = self.invoke('cancel', str(directory))
        self.assertEqual(cancelled.returncode, 0, cancelled.stderr)
        self.assertIn('preserving the unknown return', cancelled.stderr)
        self.assertEqual(self.receipt(directory)['status'], 'reclaimed')
        self.assertFalse((directory / 'command.json').exists())
        self.assertFalse(space(directory).exists())

    def test_cancel_forces_a_term_ignoring_command_without_fabricating_a_wait(self):
        code = "import os,pathlib,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); pathlib.Path(os.environ['QA_RESULTS_DIR'],'ready').touch(); time.sleep(60)"
        result, directory = self.launch(code, '--background')
        self.assertEqual(result.returncode, 0, result.stderr)
        eventually(lambda: (directory / 'results/ready').exists())
        cancelled = self.invoke('cancel', str(directory))
        self.assertEqual(cancelled.returncode, 0, cancelled.stderr)
        self.assertEqual(self.receipt(directory)['status'], 'reclaimed')
        # Escalation may kill the receipt writer before or after it reaps the command.
        receipt = directory / 'command.json'
        if receipt.exists():
            self.assertEqual(json.loads(receipt.read_text())['returncode'], -signal.SIGKILL)
        else:
            self.assertIn('preserving the unknown return', cancelled.stderr)
        self.assertFalse(space(directory).exists())

    def test_success_and_failure_keep_evidence_and_remove_only_scratch(self):
        for exit_code in (0, 37):
            with self.subTest(exit_code=exit_code):
                result, directory = self.launch("import os,pathlib,tempfile,sys; pathlib.Path(os.environ['QA_RESULTS_DIR'],'evidence.db').write_bytes(b'durable'); print(tempfile.mkstemp()[1]); print('error-stream',file=sys.stderr); sys.exit(" + str(exit_code) + ")")
                self.assertEqual(result.returncode, exit_code, result.stderr)
                self.assertEqual((directory / 'results/evidence.db').read_bytes(), b'durable')
                self.assertIn('error-stream', (directory / 'results/stderr.log').read_text())
                self.assertEqual(self.receipt(directory)['status'], 'reclaimed')
                self.assertFalse(space(directory).exists())
                self.assertEqual(qa.hashlib.sha256((directory / 'runner.py').read_bytes()).hexdigest(), json.loads((directory / 'job.json').read_text())['runner_sha256'])

if __name__ == '__main__':
    unittest.main()
