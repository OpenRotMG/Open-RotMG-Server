"""Pinned July100 DATA reconstruction; no instruction or constant-index rewriting."""
import copy
import hashlib
import json
import re
import struct
import xml.etree.ElementTree as ET
import zlib

REPAIR_SET = 'july100-play-v1'
SWF_SHA = 'ec031064b61d22e860de802ae1967ef1bb193b2c35d297ca1d25b1a96f76684e'
REPAIRS = [
    ('0xa00', 'Blunt Sword', [('MinDamage', '400', '30'), ('MaxDamage', '500', '60'),
     ('RateOfFire', '30', '1'), ('Projectile/Speed', '500', '100'),
     ('Projectile/LifetimeMS', '700', '300')]),
    ('0xa04', 'Fire Wand', [('MinDamage', '400', '20'), ('MaxDamage', '500', '40'),
     ('RateOfFire', '60', '1'), ('Projectile/Speed', '800', '160')]),
    ('0xa14', 'Blunt Dagger', [('MinDamage', '400', '20'), ('MaxDamage', '500', '60'),
     ('Projectile/Speed', '300', '140')]),
    ('0xa1a', 'Poor Quality Bow', [('MinDamage', '450', '10'), ('MaxDamage', '500', '40'),
     ('Projectile/Speed', '360', '160')]),
    ('0xa66', 'Old Helm', [("ActivateOnEquip[@stat='21']/@amount", '500', '2')]),
    ('0xa56', 'Worn Cloak', [("ActivateOnEquip[@stat='21']/@amount", '10', '2'),
     ("ConditionEffect[text()='Invisible']/@duration", '30', '3')]),
    ('0xa4f', 'Ring of Minor Defense', [('ActivateOnEquip/@stat', '20', '21'),
     ('ActivateOnEquip/@amount', '5', '1'), ('StatDescription', '\\n+10 DEF', '\\n+1 DEF')]),
    ('0xa2e', 'Fire Spray Spell', [('MinDamage', '300', '30'), ('MaxDamage', '500', '50'),
     ('MpCost', '0', '40')]),
]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def semantic(node):
    return (node.tag, sorted(node.attrib.items()), (node.text or '').strip(),
            [semantic(child) for child in node])


def selected_objects(objects):
    result = copy.deepcopy(objects)
    for kind, name, changes in REPAIRS:
        matches = [o for o in result if int(o.attrib['type'], 0) == int(kind, 0)]
        require(len(matches) == 1 and matches[0].attrib['id'] == name,
                f'{kind}: expected exactly one {name}')
        for path, old, new in changes:
            element, _, attribute = path.partition('/@')
            matches_field = matches[0].findall(element.replace('[text()=', '[.='))
            require(len(matches_field) == 1, f'{name}: expected one {path}')
            field = matches_field[0]
            value = field.get(attribute) if attribute else field.text
            require(value == old, f'{name}/{path}: expected old {old!r}, found {value!r}')
            if attribute:
                field.set(attribute, new)
            else:
                field.text = new
    return result


def pushstrings(source):
    return [(literal, json.loads(literal.replace("\\'", "'")))
            for literal in re.findall(r'pushstring ("(?:[^"\\]|\\.)*")', source)]


def object_replacements(strings, source, all_sources, objects):
    expected = selected_objects(objects)
    equipment = ET.fromstring(strings[145])
    require(equipment.tag == 'Objects' and len(equipment) == 156,
            'Unsupported dormant equipment literal145')
    original_by_type = {o.attrib['type']: o for o in objects}
    expected_by_type = {o.attrib['type']: o for o in expected}
    for item in equipment:
        require(semantic(item) == semantic(original_by_type[item.attrib['type']]),
                'Dormant equipment differs from active constructor')
    pairs = pushstrings(source)
    fragments = [value for _, value in pairs]
    selected = {int(kind, 0) for kind, _, _ in REPAIRS}
    replacements = {}

    def anchor(item):
        prefix = f' type="{item.attrib["type"]}" id="{item.attrib["id"]}">'
        found = [(i, literal, value) for i, (literal, value) in enumerate(pairs)
                 if value.startswith(prefix)]
        require(len(found) == 1, f'{prefix}: missing or repeated constructor anchor')
        i, literal, value = found[0]
        require(strings.count(value) == 1 and
                sum(s.count(literal) for s in all_sources) == 1 and
                sum(s.count('pushstring ' + literal) for s in all_sources) == 1,
                f'{prefix}: expected one global anchor use and pool entry')
        return i, value

    for i, item in enumerate(equipment):
        if int(item.attrib['type'], 0) not in selected:
            continue
        require(i + 1 < len(equipment), 'Selected final equipment node has no closing anchor')
        start_index, start = anchor(item)
        end_index, end = anchor(equipment[i + 1])
        require(start_index < end_index, 'Equipment anchor order mismatch')
        require('--' not in ''.join(fragments[start_index + 1:end_index]),
                f'{item.attrib["id"]}: unsafe XML comment span')
        node = expected_by_type[item.attrib['type']]
        replacement = ET.tostring(node, encoding='unicode').strip()[len('<Object'):]
        # Shared numeric literals cannot be changed without affecting unrelated items.
        closing = '--><Object' if start in replacements else ''
        replacements[start] = closing + replacement + '<!--'
        require(end not in replacements, 'Overlapping equipment anchors')
        replacements[end] = '--><Object' + end
        equipment[i] = copy.deepcopy(node)
    reconstructed = ''.join(replacements.get(value, value) for value in fragments)
    roots = re.findall(r'<Objects>.*?</Objects>', reconstructed, re.S)
    actual = ET.Element('Objects')
    for root in roots:
        actual.extend(ET.fromstring(root))
    require(semantic(actual) == semantic(expected), 'Unexpected active XML semantic delta')
    replacements[strings[145]] = ET.tostring(equipment, encoding='unicode')
    return replacements, expected


def uint(data, offset):
    value = 0
    for shift in range(0, 35, 7):
        require(offset < len(data), 'Truncated ABC variable integer')
        byte = data[offset]
        offset += 1
        value |= (byte & 127) << shift
        if not byte & 128:
            require(value < 2 ** 32, 'Invalid ABC variable integer')
            return value, offset
    raise ValueError('Invalid ABC variable integer')


def encoded_uint(value):
    require(0 <= value < 2 ** 30, 'ABC string length exceeds u30')
    result = bytearray()
    while value >= 128:
        result.append((value & 127) | 128)
        value >>= 7
    return bytes(result + bytes([value]))


def string_pool(abc):
    require(abc[:4] == struct.pack('<HH', 16, 46), 'Unsupported ABC version')
    offset = 4
    for _ in range(2):
        count, offset = uint(abc, offset)
        for _ in range(1, count):
            _, offset = uint(abc, offset)
    count, offset = uint(abc, offset)
    offset += 8 * max(0, count - 1)
    count, offset = uint(abc, offset)
    prefix = abc[:offset]
    strings = ['']
    for _ in range(1, count):
        length, offset = uint(abc, offset)
        require(offset + length <= len(abc), 'Truncated ABC string')
        strings.append(abc[offset:offset + length].decode('utf-8'))
        offset += length
    require(count == len(strings), 'Invalid ABC string count')
    records = [s.encode('utf-8') for s in strings[1:]]
    require(abc == prefix + b''.join(encoded_uint(len(s)) + s for s in records) + abc[offset:],
            'Noncanonical ABC string encoding would alter unselected bytes')
    return prefix, strings, abc[offset:]


def swf_parts(data):
    require(data[:4] == b'CWS\x0a' and len(data) >= 8, 'Unsupported build100 SWF header')
    try:
        body = zlib.decompress(data[8:])
    except zlib.error as error:
        raise ValueError('Malformed compressed build100 SWF') from error
    require(len(body) + 8 == struct.unpack_from('<I', data, 4)[0], 'SWF length mismatch')
    require(bool(body), 'Missing SWF frame header')
    offset = (5 + 4 * (body[0] >> 3) + 7) // 8 + 4
    prefix = body[:offset]
    tags = []
    while offset < len(body):
        start = offset
        require(offset + 2 <= len(body), 'Truncated SWF tag header')
        header = struct.unpack_from('<H', body, offset)[0]
        offset += 2
        length = header & 63
        if length == 63:
            require(offset + 4 <= len(body), 'Truncated long SWF tag header')
            length = struct.unpack_from('<I', body, offset)[0]
            offset += 4
        require(offset + length <= len(body), 'Truncated SWF tag payload')
        tags.append((header >> 6, body[offset:offset + length], body[start:offset + length]))
        offset += length
    require(tags and tags[-1][0] == 0, 'Missing SWF End tag')
    return prefix, tags


def abc_start(payload):
    require(len(payload) >= 5 and b'\0' in payload[4:], 'Missing DoABC name')
    return payload.index(b'\0', 4) + 1


def verify_binary(original, repaired, replacements):
    before_prefix, before = swf_parts(original)
    after_prefix, after = swf_parts(repaired)
    require(before_prefix == after_prefix and len(before) == len(after), 'SWF frame/tag layout changed')
    changed = []
    pool_proof = {}
    abc_count = 0
    for index, ((kind, old, raw_old), (new_kind, new, raw_new)) in enumerate(zip(before, after)):
        require(kind == new_kind, 'SWF tag order changed')
        if old == new:
            require(raw_old == raw_new, 'Unchanged SWF tag encoding changed')
            abc_count += kind == 82
            continue
        require(kind == 82, f'Unexpected changed SWF tag{index}/{kind}')
        abc_count += 1
        pos = abc_start(old)
        require(pos == abc_start(new) and old[:pos] == new[:pos], 'DoABC flags/name changed')
        p, strings, suffix = string_pool(old[pos:])
        pp, updated, ss = string_pool(new[pos:])
        require(p == pp and suffix == ss, 'Nonstring ABC bytes/instructions changed')
        require(len(strings) == len(updated), 'ABC string count changed')
        require(updated == [replacements.get(s, s) for s in strings],
                'Unexpected ABC string/index change')
        changed.append(index)
        pool_proof = {'string_count': len(strings),
                      'changed_string_indices': [i for i, (a, b) in enumerate(zip(strings, updated))
                                                 if a != b],
                      'unchanged_abc_prefix_sha256': hashlib.sha256(p).hexdigest(),
                      'unchanged_abc_suffix_sha256': hashlib.sha256(suffix).hexdigest()}
    require(abc_count == 2 and len(changed) == 1, 'Expected exactly one changed ABC of two')
    return {'tags': len(before), 'abc_tags': abc_count, 'changed_tag': changed[0],
            'changed_strings': len(replacements), **pool_proof,
            'invariant': 'All nonstring ABC bytes, other tag bytes and tag order identical'}


def repair_swf(original, objects, source, all_sources):
    require(hashlib.sha256(original).hexdigest() == SWF_SHA, 'Unsupported original SWF checksum')
    prefix, tags = swf_parts(original)
    candidates = [(i, payload, abc_start(payload)) for i, (kind, payload, _) in enumerate(tags)
                  if kind == 82 and payload[4:11] == b'frame1\0']
    require(len(candidates) == 1, 'Expected one frame1 ABC')
    index, payload, pos = candidates[0]
    pool_prefix, strings, suffix = string_pool(payload[pos:])
    require(len(strings) == 8442, 'Unsupported build100 string pool layout')
    replacements, expected = object_replacements(strings, source, all_sources, objects)
    encoded = []
    for string in strings[1:]:
        value = replacements.get(string, string).encode('utf-8')
        encoded.append(encoded_uint(len(value)) + value)
    new_payload = payload[:pos] + pool_prefix + b''.join(encoded) + suffix
    body = prefix + b''.join(
        struct.pack('<HI', (82 << 6) | 63, len(new_payload)) + new_payload
        if i == index else raw for i, (_, _, raw) in enumerate(tags))
    result = b'CWS\x0a' + struct.pack('<I', len(body) + 8) + zlib.compress(body)
    return result, expected, verify_binary(original, result, replacements)
