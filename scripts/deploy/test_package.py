import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import package
from package import RUNTIME_XML, source_archive


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        previous = Path.cwd()
        os.chdir(self.root)
        self.addCleanup(os.chdir, previous)
        subprocess.run(['git', 'init', '--quiet'], check=True)

    def tree(self, files):
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        subprocess.run(['git', 'add', '.'], check=True)
        # A tree is enough to test archive selection; fixture commits would imply false revision provenance.
        return subprocess.check_output(['git', 'write-tree'], text=True).strip()

    def archive(self, tree):
        archive = self.root / 'source.tar'
        source_archive(tree, archive)
        with tarfile.open(archive) as bundle:
            return {row.name: bundle.extractfile(row).read() for row in bundle if row.isfile()}

    def test_archive_retains_selected_runtime_and_rebuild_inputs_not_preservation(self):
        files = {name: name.encode() for name in [
            *sorted(RUNTIME_XML),
            'data/objects.xml', 'data/grounds.xml', 'data/ALLOY-LICENSE',
            'data/reference/README.md', 'data/reference/release-7.0/xml/equip.xml',
            'data/reference/release-7.0/source/behavior.cc',
            'data/reference/release-7.0/original.zip',
            'data/reference/deca-xml-documentation/original-html.tar.gz',
            'data/game/unselected.xml', 'data/game/123.5.1/unselected.xml',
            'tests/fixtures/xml_runtime/build.py',
            'data/realm-catalog.json', 'data/realm-sites.json', 'data/realms/world07.json',
            'data/realm-elevations.json',
            *[f'data/realm-fsod-{i}.elevation.zlib' for i in range(1, 4)],
            'data/realms/classic-world-07-reconstruction.wmap',
            *['data/realms/world07/' + name for name in [
                'cells.dictionary.json', 'cells.indices.zlib', 'source-policy.json',
                'source-annotations.json', 'lava-support.json', 'elevation.zlib',
                'reconstruction-decisions.json']],
            'data/lab-templates.json', 'data/manor-templates.json', 'data/davy-templates.json',
            'data/spider-templates.json', 'data/nexus.wmap', 'data/oryx-chamber.wmap',
            'scripts/backups/backup.py', 'scripts/deploy/update.py', 'LICENSE']}
        tree = self.tree(files)
        # Untracked private inputs must not be added by the archive operation.
        for name in ['.env', '.local/game.db', '.local/screenshot.png']:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'private')
        actual = self.archive(tree)
        excluded = {name for name in files if name.startswith((
            'data/reference/', 'tests/fixtures/xml_runtime/'))
            or name.endswith('/reconstruction-decisions.json')
            or (name.startswith('data/game/') and name not in RUNTIME_XML)}
        self.assertEqual(actual, {name: content for name, content in files.items() if name not in excluded})
        for name in excluded:
            self.assertEqual(subprocess.check_output(['git', 'show', tree + ':' + name]), files[name])
            self.assertEqual((self.root / name).read_bytes(), files[name])

    def test_missing_selected_xml_is_an_error(self):
        self.tree({name: name.encode() for name in RUNTIME_XML})
        for missing in sorted(RUNTIME_XML):
            with self.subTest(missing=missing):
                subprocess.run(['git', 'update-index', '--force-remove', missing], check=True)
                tree = subprocess.check_output(['git', 'write-tree'], text=True).strip()
                with self.assertRaisesRegex(RuntimeError, 'Missing selected runtime XML: ' + missing):
                    source_archive(tree, self.root / 'source.tar')
                self.assertFalse((self.root / 'source.tar').exists())
                subprocess.run(['git', 'add', missing], check=True)

    def test_archive_uses_exact_tree_not_later_working_bytes(self):
        files = {name: name.encode() for name in RUNTIME_XML}
        tree = self.tree(files)
        for name in RUNTIME_XML:
            (self.root / name).write_bytes(b'not the selected tree')
        self.assertEqual(self.archive(tree), files)

    def test_main_rejects_non_head_revision_before_creating_output(self):
        output = self.root / 'output'
        with patch('package.sys.argv', ['package.py', 'not-head', 'binary', str(output)]), \
                patch('package.subprocess.check_output', return_value='actual-head\n'):
            with self.assertRaises(AssertionError):
                package.main()
        self.assertFalse(output.exists())

    def test_normal_unpack_size_guard_is_64_mib(self):
        self.assertEqual(package.LIMIT, 64 * 1024 * 1024)
        archive = self.root / 'oversized.tar'
        with tarfile.open(archive, 'w') as bundle:
            member = tarfile.TarInfo('oversized')
            member.size = package.LIMIT + 1
            # Only the header is needed: unpack must reject before reading/extracting payloads.
            bundle.fileobj.write(member.tobuf())
            bundle.fileobj.seek(member.size, 1)
        with self.assertRaisesRegex(RuntimeError, 'Release exceeds size limit'):
            package.unpack(archive, self.root / 'unpacked', {})


if __name__ == '__main__':
    unittest.main()
