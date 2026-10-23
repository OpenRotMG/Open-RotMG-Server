#!/usr/bin/env python3
"""Prepare user-supplied build100 assets; never modifies or publishes the source SWF."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

from build100_data_repairs import REPAIR_SET, REPAIRS, repair_swf, semantic

SWF_SHA = 'ec031064b61d22e860de802ae1967ef1bb193b2c35d297ca1d25b1a96f76684e'
RUFFLE_SHA = '77e7b3f818dc1ac25b309fd2380b6c23223a8fcd46563061ae27ebbbb522f24b'


def extract(swf, ffdec, all_scripts=False):
    with tempfile.TemporaryDirectory(prefix='build100-') as temp:
        selection = [] if all_scripts else [
            '-selectclass', 'com.company.assembleegameclient.util.++']
        subprocess.run(['java', '-Xss8m', '-jar', str(ffdec.resolve()), '-format', 'script:pcode',
                        *selection, '-export', 'script', temp, str(swf.resolve())],
                       check=True, stdout=subprocess.DEVNULL)
        all_sources = [p.read_text() for p in Path(temp).rglob('*.pcode')]
        if all_scripts and len(all_sources) != 371:
            raise ValueError(
                f'Incomplete build100 global export: expected 371 scripts, found {len(all_sources)}')
        sources = [s for s in all_sources if 'id=\\"Wizard\\"' in s and 'id=\\"Pirate\\"' in s]
        if len(sources) != 1:
            raise ValueError(f'Expected exactly one embedded content class, found {len(sources)}')
        fragments = re.findall(r'pushstring ("(?:[^"\\]|\\.)*")', sources[0])
        joined = ''.join(json.loads(s.replace("\\'", "'")) for s in fragments)
        roots = [ET.fromstring(s) for s in re.findall(r'<Objects>.*?</Objects>', joined, re.S)]
        grounds = re.findall(r'<GroundTypes>.*?</GroundTypes>', joined, re.S)
        if len(roots) != 7 or sum(map(len, roots)) != 528 or len(grounds) != 1:
            raise ValueError('Incomplete build100 embedded XML inventory')
        objects = ET.Element('Objects')
        for root in roots:
            objects.extend(root)
        ground_root = ET.fromstring(grounds[0])
        if len(ground_root) != 115:
            raise ValueError('Incomplete build100 ground inventory; expected 115')
        for root in (objects, ground_root):
            ids = [int(n.attrib['type'], 0) for n in root]
            if len(set(ids)) != len(ids):
                raise ValueError(f'Duplicate build100 {root.tag} type')
        return objects, grounds[0], sources[0], all_sources


def validate_output(output, inputs):
    if any(output.resolve() == p.resolve() or output.resolve() in p.resolve().parents
           for p in inputs):
        raise ValueError(f'Output aliases or contains an input asset: {output}')
    if output.exists() and (not output.is_dir() or any(output.iterdir())):
        raise FileExistsError(f'Output directory must be empty: {output}')


def prepare(swf, ffdec, ruffle, output, data_repairs=None):
    if data_repairs not in (None, REPAIR_SET):
        raise ValueError(f'Unsupported data repair set: {data_repairs}')
    validate_output(output, [swf, ffdec, ruffle])
    for path, expected in [(swf, SWF_SHA), (ruffle, RUFFLE_SHA)]:
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f'Unsupported asset checksum: {path}; expected {expected}')
    output.mkdir(parents=True, exist_ok=True)
    manifest = {'client_sha256': SWF_SHA, 'ruffle_zip_sha256': RUFFLE_SHA,
                'objects': 528, 'ground_types': 115}
    html = HTML
    if data_repairs:
        original_objects, original_grounds, source, all_sources = extract(swf, ffdec, all_scripts=True)
        repaired, expected_objects, proof = repair_swf(
            swf.read_bytes(), original_objects, source, all_sources)
        client = output / 'client-data-repaired.swf'
        client.write_bytes(repaired)
        # Server XML must come from the output's active constructor, not a second patch table.
        objects, grounds, _, _ = extract(client, ffdec)
        if semantic(objects) != semantic(expected_objects) or grounds != original_grounds:
            raise ValueError('Output SWF extraction differs from selected data/unchanged grounds')
        label = 'July100 reconstructed DATA-repaired play copy — not authenticated production balance'
        manifest.update(client_sha256=hashlib.sha256(repaired).hexdigest(),
                        original_client_sha256=SWF_SHA, client_file=client.name,
                        data_repairs=REPAIR_SET, label=label, binary_invariants=proof,
                        representation='Relocated Object literals; superseded data enclosed in XML comments',
                        selected=[{'type': kind, 'name': name, 'changes': changes}
                                  for kind, name, changes in REPAIRS])
        html = html.replace('OpenRotMG July100 reconstruction — unchanged client', label).replace(
            'Supplied client bytes unchanged.', label + '.').replace(
            "url:'/client.swf'", "url:'/client-data-repaired.swf'")
    else:
        objects, grounds, _, _ = extract(swf, ffdec)
        shutil.copyfile(swf, output / 'client.swf')
    ET.ElementTree(objects).write(output / 'objects.xml', encoding='unicode')
    (output / 'grounds.xml').write_text(grounds)
    with zipfile.ZipFile(ruffle) as archive:
        for entry in archive.infolist():
            if not (output / 'vendor' / entry.filename).resolve().is_relative_to((output / 'vendor').resolve()):
                raise ValueError('Unsafe Ruffle archive path')
        archive.extractall(output / 'vendor')
    (output / 'index.html').write_text(html)
    (output / 'build100.json').write_text(json.dumps(manifest, indent=2) + '\n')
    mode = 'DATA-repaired play copy' if data_repairs else 'unchanged build100 client'
    print(f'Prepared {mode} and Ruffle 0.7.1-openrotmg.5 at {output}')


HTML = '''<!doctype html><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; connect-src 'self'; worker-src 'self' blob:; img-src 'self' data:; media-src 'self' blob:">
<title>OpenRotMG July100 reconstruction — unchanged client</title>
<style>body{background:#222;color:white;font:14px sans-serif}ruffle-player{display:block;width:800px;height:600px}pre{white-space:pre-wrap}</style>
<div id="game"></div><pre id="status">July100 server reconstruction: Tutorial → shared realm, no Nexus. Supplied client bytes unchanged.
Sparse traversal, not a complete restored game. An explicitly launched --arena server is only a compatibility fixture.</pre>
<script src="/vendor/ruffle.js"></script><script>
(async()=>{
 const p=window.RufflePlayer.newest().createPlayer();document.querySelector('#game').append(p);
 const socket=new URL('/game',location.href);socket.protocol='ws:';
 await p.ruffle().load({url:'/client.swf',autoplay:'on',unmuteOverlay:'hidden',openUrlMode:'deny',allowScriptAccess:false,preferredRenderer:'wgpu-webgl',logLevel:'info',
 urlRewriteRules:[[/^http:\\/\\/www\\.realmofthemadgod\\.com\\//,location.origin+'/legacy/'],[/^http:\\/\\/(?:rotmgtesting|realmofthemadgod)\\.appspot\\.com\\//,location.origin+'/legacy/']],
 socketProxy:[{host:'127.0.0.1',port:2050,proxyUrl:socket.href}]});
})().catch(e=>{document.querySelector('#status').textContent=String(e);console.error(e)});
</script>'''

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-repairs', choices=[REPAIR_SET])
    for name in ('swf', 'ffdec', 'ruffle', 'output'):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    prepare(args.swf, args.ffdec, args.ruffle, args.output, args.data_repairs)
