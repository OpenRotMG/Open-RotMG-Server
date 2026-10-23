import errno
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('build_storage', Path(__file__).with_name('cargo.py'))
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


class BuildStorageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        (self.source / 'scripts').mkdir(parents=True)
        shutil.copyfile(guard.__file__, self.source / 'scripts/cargo.py')
        source = patch.object(guard, 'ROOT', self.source)
        source.start()
        self.addCleanup(source.stop)
        env = {key: value for key, value in os.environ.items() if not key.startswith(('CARGO_', 'RUSTFLAGS', 'RUSTUP_TOOLCHAIN'))}
        env.update(XDG_CACHE_HOME=str(self.root / 'cache'))
        self.env = patch.dict(os.environ, env, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)
        self.output = patch('sys.stdout', new_callable=io.StringIO)
        self.stdout = self.output.start()
        self.addCleanup(self.output.stop)
        self.errors = patch('sys.stderr', new_callable=io.StringIO)
        self.stderr = self.errors.start()
        self.addCleanup(self.errors.stop)

    def build(self, args=None):
        with patch.object(guard.subprocess, 'check_output', return_value='rustc test host'), \
                patch.object(guard, 'execute', return_value=0) as execute:
            code = guard.main(args or ['build', '--locked'])
        return code, execute

    def test_targets_isolate_checkouts_and_configuration_but_reuse_commands(self):
        with patch.object(guard, 'ROOT', self.root / 'worktree-a'):
            code, first = self.build()
            _, same = self.build(['test', '--locked'])
            with patch.dict(os.environ, RUSTFLAGS='-C debuginfo=1'):
                _, changed = self.build()
        with patch.object(guard, 'ROOT', self.root / 'worktree-b'):
            code2, second = self.build()
        self.assertEqual((code, code2), (0, 0))
        self.assertEqual(first.call_args.args[1], same.call_args.args[1])
        self.assertNotEqual(first.call_args.args[1], second.call_args.args[1])
        self.assertNotEqual(first.call_args.args[1], changed.call_args.args[1])

    def test_runtime_settings_reuse_targets_but_compiler_settings_remain_isolated(self):
        _, first = self.build()
        for key in ['RUST_LOG', 'RUST_BACKTRACE', 'RUST_LIB_BACKTRACE', 'RUST_TEST_THREADS', 'CARGO_BUILD_JOBS']:
            with self.subTest(key=key), patch.dict(os.environ, {key: '2'}):
                _, same = self.build()
                self.assertEqual(first.call_args.args[1], same.call_args.args[1])
        for key in ['RUSTFLAGS', 'RUSTC_WRAPPER', 'CARGO_PROFILE_DEV_DEBUG', 'CARGO_BUILD_TARGET']:
            with self.subTest(key=key), patch.dict(os.environ, {key: 'different'}):
                _, changed = self.build()
                self.assertNotEqual(first.call_args.args[1], changed.call_args.args[1])

    def test_build_never_checks_disk_space_or_walks_the_cache(self):
        with patch.object(guard.shutil, 'disk_usage', side_effect=AssertionError('disk space check')), \
                patch.object(guard.os, 'walk', side_effect=AssertionError('cache scan')):
            code, execute = self.build()
        self.assertEqual(code, 0)
        execute.assert_called_once()
        self.assertIn('Cargo target:', self.stdout.getvalue())
        self.assertEqual(self.stderr.getvalue(), '')

    def test_formatting_skips_cache_and_compiler_setup_and_preserves_failure(self):
        with patch.object(guard, 'cache_root', side_effect=AssertionError('formatting must not use the cache')), \
                patch.object(guard.subprocess, 'check_output', side_effect=AssertionError('compiler lookup')), \
                patch.object(guard.subprocess, 'call', return_value=37) as cargo:
            self.assertEqual(guard.main(['--toolchain', '1.94.1', 'fmt', '--all', '--', '--check']), 37)
        cargo.assert_called_once_with(['cargo', '+1.94.1', 'fmt', '--all', '--', '--check'], cwd=self.source)

    def test_real_allocation_failures_are_reported_as_failures(self):
        for number in [errno.ENOSPC, errno.EDQUOT]:
            with self.subTest(errno=number), \
                    patch.object(guard.subprocess, 'check_output', return_value='rustc'), \
                    patch.object(guard, 'execute', side_effect=OSError(number, os.strerror(number))):
                self.assertEqual(guard.main(['build']), 1)
                self.assertIn(os.strerror(number), self.stderr.getvalue())

    def test_storage_overrides_and_worktree_local_cache_are_rejected(self):
        for args in [['build', '--target-dir=x'], ['build', '--config', 'build.target-dir="x"'], ['build', '--manifest-path=x']]:
            with self.subTest(args=args):
                code, execute = self.build(args)
                self.assertEqual(code, 1)
                execute.assert_not_called()
        with patch.dict(os.environ, CARGO_TARGET_DIR=str(self.root / 'other')):
            self.assertEqual(self.build()[0], 1)
        with patch.dict(os.environ, XDG_CACHE_HOME=str(self.root)), patch.object(guard, 'ROOT', self.root):
            self.assertEqual(self.build()[0], 1)

    def test_cleanup_preserves_binary_codegen_and_adjacent_evidence(self):
        root = guard.cache_root()
        with guard.locked(root):
            pass
        target = root / ('a' * 20)
        deleted = ['debug/deps/a', 'debug/incremental/a', 'release/deps/a']
        kept = ['debug/build/sdk.abc', 'debug/build/nested/debug/deps/sdk.abc', 'debug/openrotmg', 'release/openrotmg', 'evidence.json']
        for name in deleted + kept:
            path = target / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name)
        self.assertEqual(guard.main(['clean']), 0)
        for name in deleted:
            self.assertFalse((target / name).exists())
        for name in kept:
            self.assertEqual((target / name).read_text(), name)

    def test_cleanup_refuses_foreign_contents_and_symlinks_without_deleting_original(self):
        root = guard.cache_root()
        root.mkdir(parents=True)
        original = root / 'original.db'
        original.write_text('irreplaceable')
        self.assertEqual(guard.main(['clean']), 1)
        self.assertEqual(original.read_text(), 'irreplaceable')
        original.unlink()
        with guard.locked(root):
            pass
        external = self.root / 'original.db'
        external.write_text('keep')
        target = root / ('a' * 20)
        (target / 'debug').mkdir(parents=True)
        (target / 'debug/deps').symlink_to(external)
        self.assertEqual(guard.main(['clean']), 1)
        self.assertEqual(external.read_text(), 'keep')

    def test_build_failure_propagates_and_does_not_publish_output(self):
        output = self.root / 'accepted'
        output.write_text('old')
        with patch.object(guard.subprocess, 'check_output', return_value='rustc'), \
                patch.object(guard, 'execute', return_value=37):
            self.assertEqual(guard.main(['--output', str(output), 'build']), 37)
        self.assertEqual(output.read_text(), 'old')

    def test_real_cargo_reuses_checkouts_and_rebuilds_changed_compile_time_env(self):
        for name in ['first', 'second']:
            repo = self.root / name
            (repo / 'src').mkdir(parents=True)
            shutil.copyfile(Path(__file__).resolve().parents[1] / 'rust-toolchain.toml', repo / 'rust-toolchain.toml')
            (repo / 'Cargo.toml').write_text('[package]\nname="openrotmg"\nversion="0.1.0"\nedition="2021"\n')
            (repo / 'src/main.rs').write_text(f'fn main() {{ println!("{name} {{}}", env!("RUST_LOG")); }}')
            for source in [repo / 'Cargo.toml', repo / 'src/main.rs']:
                os.utime(source, (1, 1))
        outputs = []
        for index, (name, level) in enumerate([('first', 'info'), ('second', 'debug'), ('first', 'info'), ('first', 'trace')]):
            output = self.root / f'retained-{index}'
            with patch.object(guard, 'ROOT', self.root / name), patch.dict(os.environ, RUST_LOG=level):
                self.assertEqual(guard.main(['--output', str(output), 'build', '--offline']), 0)
            outputs.append(output)
        self.assertEqual([subprocess.check_output([str(path)], text=True).strip() for path in outputs],
                         ['first info', 'second debug', 'first info', 'first trace'])
        self.assertEqual(len(list(guard.cache_root().glob('*/debug'))), 2)
        self.assertEqual(guard.main(['clean']), 0)
        self.assertEqual(subprocess.check_output([str(outputs[0])], text=True).strip(), 'first info')

    def test_concurrent_builds_lock_and_cleanup_refuses_active_cargo(self):
        bins = self.root / 'bin'
        bins.mkdir()
        cargo = bins / 'cargo'
        cargo.write_text('#!' + sys.executable + '\nimport sys\nprint("READY", flush=True)\nsys.stdin.read(1)\n')
        cargo.chmod(0o755)
        env = {**os.environ, 'PATH': str(bins) + os.pathsep + os.environ['PATH']}
        command = [sys.executable, str(self.source / 'scripts/cargo.py'), 'build']
        processes = []
        try:
            first = subprocess.Popen(command, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            processes.append(first)
            while first.stdout.readline().strip() != 'READY':
                if first.poll() is not None:
                    self.fail(first.stderr.read())
            self.assertEqual(guard.main(['clean']), 1)
            self.assertIn('active build cache', self.stderr.getvalue())
            second = subprocess.Popen(command, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            processes.append(second)
            self.assertIn('Waiting for build cache lock', second.stdout.readline())
            first.terminate()
            first.wait(timeout=15)
            self.assertEqual(guard.main(['clean']), 1)
            self.assertIn('active build cache', self.stderr.getvalue())
            self.assertEqual(first.communicate('x', timeout=15)[1], '')
            self.assertLess(first.returncode, 0)
            self.assertEqual(second.communicate('x', timeout=15)[1], '')
            self.assertEqual(second.returncode, 0)
        finally:
            for process in processes:
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=15)


if __name__ == '__main__':
    unittest.main()
