import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

from package import source_archive


class PackageTests(unittest.TestCase):
    def test_archive_retains_rebuild_inputs_but_not_nonruntime_witness_ledgers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            previous = Path.cwd()
            os.chdir(root)
            try:
                files = {name: name.encode() for name in [
                    'data/realm-catalog.json', 'data/realm-sites.json', 'data/realms/world07.json',
                    'data/realms/classic-world-07-reconstruction.wmap',
                    *['data/realms/world07/' + name for name in [
                        'cells.dictionary.json', 'cells.indices.zlib', 'source-policy.json',
                        'source-annotations.json', 'lava-support.json', 'reconstruction-decisions.json']],
                    'scripts/backups/backup.py', 'scripts/deploy/update.py', 'LICENSE']}
                for name, content in files.items():
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(content)
                subprocess.run(['git', 'init', '--quiet'], check=True)
                subprocess.run(['git', 'add', '.'], check=True)
                subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                                '-c', 'commit.gpgsign=false', 'commit', '--quiet', '-m', 'fixture'], check=True)
                revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
                archive = root / 'source.tar'
                source_archive(revision, archive)
                with tarfile.open(archive) as bundle:
                    actual = {row.name: bundle.extractfile(row).read() for row in bundle if row.isfile()}
                witness = 'data/realms/world07/reconstruction-decisions.json'
                self.assertEqual(actual, {name: content for name, content in files.items() if name != witness})
                self.assertEqual(subprocess.check_output(['git', 'show', revision + ':' + witness]), files[witness])
                self.assertEqual((root / witness).read_bytes(), files[witness])
            finally:
                os.chdir(previous)


if __name__ == '__main__':
    unittest.main()
