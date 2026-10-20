"""Focused data-only preparer gates; qualified asset checks are opt-in via BUILD100_QA."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
import zlib

import build100_data_repairs as repair

spec = importlib.util.spec_from_file_location(
    'prepare_build100', Path(__file__).with_name('prepare-build100.py'))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


def fixture():
    root = ET.Element('Objects')
    for kind, name, changes in repair.REPAIRS:
        node = ET.SubElement(root, 'Object', type=kind, id=name)
        ET.SubElement(node, 'Class').text = 'Equipment'
        for path, old, _ in changes:
            element, _, attribute = path.partition('/@')
            current = node
            for segment in element.split('/'):
                tag = segment.split('[')[0]
                child = current.find(tag)
                if child is None:
                    child = ET.SubElement(current, tag)
                if "@stat='21'" in segment:
                    child.set('stat', '21')
                if "text()='Invisible'" in segment:
                    child.text = 'Invisible'
                current = child
            if attribute:
                current.set(attribute, old)
            else:
                current.text = old
    for i in range(156 - len(root)):
        node = ET.SubElement(root, 'Object', type=hex(0x1000 + i), id=f'Control{i}')
        ET.SubElement(node, 'Class').text = 'Equipment'
    fragments = ['<Objects><Object']
    for i, node in enumerate(root):
        text = ET.tostring(node, encoding='unicode')
        boundary = text.index('<Class') + len('<Class')
        fragments.extend([text[len('<Object'):boundary], text[boundary:] +
                          ('<Object' if i + 1 < len(root) else '</Objects>')])
    source = '\n'.join('pushstring ' + json.dumps(f) for f in fragments)
    strings = [''] * 145 + [ET.tostring(root, encoding='unicode')] + list(dict.fromkeys(fragments))
    return root, strings, source


def binary(strings, suffix=b'unchanged methods', color=b'RGB', name=b'frame1'):
    def abc(values):
        return (struct.pack('<HH', 16, 46) + b'\1\1\1' + repair.encoded_uint(len(values) + 1) +
                b''.join(repair.encoded_uint(len(s)) + s for s in values) + suffix)

    def tag(kind, value):
        return struct.pack('<HI', (kind << 6) | 63, len(value)) + value

    body = (b'\x08\0\0\x0c\1\0' + tag(82, b'\0' * 4 + b'boot\0' + abc([b'boot'])) +
            tag(9, color) + tag(82, b'\0' * 4 + name + b'\0' + abc(strings)) + b'\0\0')
    return b'CWS\x0a' + struct.pack('<I', len(body) + 8) + zlib.compress(body)


class DataRepairTests(unittest.TestCase):
    def test_selected_delta_and_adjacent_comment_boundaries(self):
        root, strings, source = fixture()
        changes, expected = repair.object_replacements(strings, source, [source], root)
        joined = ''.join(changes.get(s, s) for _, s in repair.pushstrings(source))
        actual = ET.fromstring(joined)
        self.assertEqual(repair.semantic(expected), repair.semantic(actual))
        retained = ET.fromstring(joined, parser=ET.XMLParser(target=ET.TreeBuilder(insert_comments=True)))
        self.assertEqual(len(retained.findall('Object')), 156)
        self.assertEqual(len(retained), 164)
        self.assertEqual(len(changes), 10)  # Eight adjacent nodes: nine anchors plus literal145.
        self.assertEqual(repair.semantic(ET.fromstring(changes[strings[145]])), repair.semantic(expected))
        self.assertEqual(sum(len(changes) for _, _, changes in repair.REPAIRS), 24)

    def test_old_value_missing_field_and_identity_fail(self):
        root, _, _ = fixture()
        for mutate in [lambda r: setattr(r[0].find('MinDamage'), 'text', '999'),
                       lambda r: r[0].remove(r[0].find('MaxDamage')),
                       lambda r: r[0].append(copy.deepcopy(r[0].find('MinDamage'))),
                       lambda r: r[0].set('id', 'Unexpected')]:
            bad = copy.deepcopy(root)
            mutate(bad)
            with self.assertRaises(ValueError):
                repair.selected_objects(bad)

    def test_anchors_shared_missing_and_unsafe_comments_fail(self):
        root, strings, source = fixture()
        with self.assertRaisesRegex(ValueError, 'global anchor'):
            repair.object_replacements(strings, source, [source, source], root)
        with self.assertRaisesRegex(ValueError, 'anchor'):
            repair.object_replacements(strings, source.replace('id=\\"Blunt Sword', 'id=\\"Missing'),
                                       [source], root)
        bad = source.replace('>Equipment</Class>', '>Equipment--</Class>', 1)
        with self.assertRaisesRegex(ValueError, 'unsafe XML comment'):
            repair.object_replacements(strings, bad, [bad], root)
        head, tail = source.rsplit('>Equipment</Class>', 1)
        bad = head + '>Unexpected</Class>' + tail
        with self.assertRaisesRegex(ValueError, 'semantic delta'):
            repair.object_replacements(strings, bad, [bad], root)

    def test_binary_preserves_code_tags_indices_and_rejects_mutations(self):
        original = binary([b'before', b'other'])
        repaired = binary([b'after', b'other'])
        proof = repair.verify_binary(original, repaired, {'before': 'after'})
        self.assertEqual(proof['changed_string_indices'], [1])
        for mutated in [binary([b'other', b'after']), binary([b'after', b'other'], color=b'BAD'),
                        binary([b'after', b'other'], suffix=b'changed methods'),
                        binary([b'after', b'other'], name=b'wrong')]:
            with self.assertRaises(ValueError):
                repair.verify_binary(original, mutated, {'before': 'after'})

    def test_malformed_and_unsupported_binary_fail(self):
        for malformed in [b'', b'FWS\x0a' + b'\0' * 8, b'CWS\x0a' + b'\0' * 8,
                          binary([b'x'])[:4] + b'\0' * 4 + binary([b'x'])[8:]]:
            with self.assertRaises(ValueError):
                repair.swf_parts(malformed)
        for malformed in [b'', b'\x10\0\x2e\0\x80', b'\x10\0\x2e\0\1\1\1\2\xff']:
            with self.assertRaises(ValueError):
                repair.string_pool(malformed)
        with self.assertRaisesRegex(ValueError, 'Noncanonical ABC string'):
            repair.string_pool(b'\x10\0\x2e\0\1\1\1\2\x81\0x')
        root, _, source = fixture()
        with self.assertRaisesRegex(ValueError, 'checksum'):
            repair.repair_swf(binary([b'x']), root, source, [source])
        with patch.object(prepare.subprocess, 'run'):
            with self.assertRaisesRegex(ValueError, 'expected 371 scripts'):
                prepare.extract(Path('unused.swf'), Path('unused.jar'), all_scripts=True)

    def test_output_alias_nonempty_and_checksum_fail_before_writes(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            asset = base / 'original.swf'
            asset.write_bytes(b'not qualified')
            alias = base / 'alias'
            alias.symlink_to(base, target_is_directory=True)
            for output in [asset, base, alias]:
                with self.assertRaisesRegex(ValueError, 'aliases'):
                    prepare.validate_output(output, [asset])
            occupied = base / 'occupied'
            occupied.mkdir()
            (occupied / 'keep').write_text('preserved')
            with self.assertRaises(FileExistsError):
                prepare.validate_output(occupied, [asset])
            out = base / 'new'
            with self.assertRaisesRegex(ValueError, 'checksum'):
                prepare.prepare(asset, asset, asset, out)
            self.assertFalse(out.exists())
            with self.assertRaisesRegex(ValueError, 'repair set'):
                prepare.prepare(asset, asset, asset, out, 'other-era')
            self.assertEqual(asset.read_bytes(), b'not qualified')

    @unittest.skipUnless(os.environ.get('BUILD100_QA'),
                         'set BUILD100_QA to private phase4 evidence directory')
    def test_qualified_output_exact_selected_delta_and_preserved_controls(self):
        qa = Path(os.environ['BUILD100_QA'])
        before = ET.parse(qa / 'prepared-original/objects.xml').getroot()
        after = ET.parse(qa / 'prepared-repaired/objects.xml').getroot()
        self.assertEqual(repair.semantic(repair.selected_objects(before)), repair.semantic(after))
        self.assertEqual((qa / 'prepared-original/grounds.xml').read_bytes(),
                         (qa / 'prepared-repaired/grounds.xml').read_bytes())
        original = (qa / 'prepared-original/client.swf').read_bytes()
        played = (qa / 'prepared-repaired/client-data-repaired.swf').read_bytes()
        import hashlib
        self.assertEqual(hashlib.sha256(original).hexdigest(), repair.SWF_SHA)
        with self.assertRaisesRegex(ValueError, 'checksum'):
            repair.repair_swf(played, before, '', [])
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / 'must-not-exist'
            with self.assertRaisesRegex(ValueError, 'checksum'):
                prepare.prepare(qa / 'prepared-repaired/client-data-repaired.swf',
                                Path('unused.jar'), Path('unused.zip'), output, repair.REPAIR_SET)
            self.assertFalse(output.exists())
        for name in ['Energy Staff', 'Golden Helm', 'Ring of Greater Attack', 'Health Potion']:
            self.assertEqual(repair.semantic(before.find(f"Object[@id='{name}']")),
                             repair.semantic(after.find(f"Object[@id='{name}']")))
        spell = after.find("Object[@id='Fire Spray Spell']")
        self.assertEqual(spell.findtext('Projectile/Speed'), '3')
        self.assertEqual(spell.findtext('Projectile/LifetimeMS'), '1000')
        default_manifest = json.loads((qa / 'prepared-original/build100.json').read_text())
        manifest = json.loads((qa / 'prepared-repaired/build100.json').read_text())
        self.assertNotIn('data_repairs', default_manifest)
        self.assertEqual(manifest['data_repairs'], repair.REPAIR_SET)
        self.assertIn('not authenticated production', manifest['label'])
        self.assertIn('XML comments', manifest['representation'])
        html = (qa / 'prepared-repaired/index.html').read_text()
        self.assertIn('DATA-repaired play copy', html)
        self.assertIn("url:'/client-data-repaired.swf'", html)


if __name__ == '__main__':
    unittest.main()
