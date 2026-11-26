#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.dont_write_bytecode = True
from realm_contract import check_realms
from update import LIMIT, unpack


RUNTIME_XML = frozenset('data/game/123.5.1/' + name for name in (
    'game.xml', 'actors.xml', 'geometry.xml', 'strings.xml'))


def source_archive(revision, destination):
    paths = subprocess.check_output(['git', 'ls-tree', '-rz', '--name-only', revision]).decode().split('\0')
    missing = RUNTIME_XML - set(paths)
    if missing:
        raise RuntimeError('Missing selected runtime XML: ' + ', '.join(sorted(missing)))
    # Preservation and normalization inputs stay in Git; only selected rules enter the release.
    paths = [path for path in paths if path
             and not path.startswith(('data/reference/', 'tests/fixtures/xml_runtime/'))
             and not (path.startswith('data/game/') and path not in RUNTIME_XML)
             and not (Path(path).match('data/realms/*/reconstruction-decisions.json'))]
    subprocess.run(['git', 'archive', revision, '-o', str(destination), '--', *paths], check=True)


def main():
    revision, binary, output = sys.argv[1:]
    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == revision
    output = Path(output)
    output.mkdir()
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        source_archive(revision, root / 'source.tar')
        stage = root / 'server'
        stage.mkdir()
        with tarfile.open(root / 'source.tar') as archive:
            archive.extractall(stage, filter='data')
        shutil.copyfile(binary, stage / 'openrotmg')
        (stage / 'openrotmg').chmod(0o755)
        (stage / 'REVISION').write_text(revision + '\n')
        sys.path.insert(0, str(stage / 'scripts/backups'))
        from release_contract import check
        check(stage / 'openrotmg', stage / 'data', root / 'contract', revision)
        check_realms(stage / 'openrotmg', stage / 'data', root / 'realm-data')
        digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
        manifest = {str(p.relative_to(stage)): digest(p) for p in sorted(stage.rglob('*')) if p.is_file()}
        with tarfile.open(output / 'server.tar.gz', 'w:gz') as archive:
            for p in sorted(stage.rglob('*')):
                if p.is_file():
                    archive.add(p, arcname=str(p.relative_to(stage)))
        metadata = {'format': 1, 'revision': revision, 'archive_sha256': digest(output / 'server.tar.gz'),
                    'binary_sha256': manifest['openrotmg'], 'manifest': manifest}
        print(json.dumps({'event': 'release_archive_contract_check', 'revision': revision, 'files': len(manifest),
                          'uncompressed_bytes': sum((stage / name).stat().st_size for name in manifest),
                          'limit': LIMIT}), flush=True)
        unpack(output / 'server.tar.gz', root / 'packaged', metadata)
        (output / 'release.json').write_text(json.dumps(metadata, sort_keys=True) + '\n')


if __name__ == '__main__':
    main()
