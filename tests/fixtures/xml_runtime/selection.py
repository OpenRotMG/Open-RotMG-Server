"""Reproduce the selected runtime data; originals are inputs, never runtime includes."""
from pathlib import Path
import collections
import copy
import hashlib
import json
import re
import xml.etree.ElementTree as X

DATA = Path('data')
OUT = DATA / 'game/123.5.1'
ARCHIVE = DATA / 'reference/release-7.0/xml'
SERVER = {'Group', 'Behavior', 'State', 'Transition', 'Order', 'DefineEmbed', 'Embed',
          'Loot', 'SoulboundLoot', 'SpawnProb', 'Spawn', 'PerRealmMax', 'Terrain', 'XpMult'}
OBJECT_REFERENCES = {'childId', 'childIds', 'childCorpseId', 'objectId', 'protecteeId', 'targetId',
                     'targetIds', 'targetGroups', 'listenIds', 'groupIds', 'groupId', 'textureSourceId',
                     'exist', 'noneExist'}
selected = {int(o.attrib['type'], 16): o for o in X.parse(DATA/'objects.xml').getroot() if o.tag == 'Object'}
by_name = {o.attrib['id']: k for k, o in selected.items()}
source = collections.defaultdict(list)
for file in sorted(ARCHIVE.rglob('*.xml')):
    try:
        doc = X.parse(file).getroot()
    except X.ParseError:
        assert file.name == 'candyLand.xml'
        continue
    for o in doc.findall('Object'):
        try:
            k = int(o.attrib['type'], 16)
        except ValueError:
            assert file.name == 'travisTesting.xml'
            continue
        if k in selected and (o.attrib['id'] == selected[k].attrib['id'] or k == 0x954):
            source[k].append((file, o))


def save(name, root):
    OUT.mkdir(parents=True, exist_ok=True)
    X.indent(root, space='  ')
    xml = X.tostring(root, encoding='utf-8', xml_declaration=True)
    # Removing source comments can leave whitespace-only lines inside generated empty states.
    (OUT/name).write_bytes(re.sub(rb'(?m)^[ \t]+$', b'', xml) + b'\n')


def qualify(provenance, origin, reason):
    return dict(provenance=provenance, origin=origin, reason=reason)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def actors(ids):
    result = X.Element('Objects')
    done = set()
    pending = set(ids)
    corrections = []
    refs = OBJECT_REFERENCES
    spellings = dict(aquireRange='acquireRange', acquirerange='acquireRange',
                     coodown='cooldown', defaultangle='defaultAngle', numshots='numShots')
    groups = collections.defaultdict(set)
    for k, entries in source.items():
        if len(entries) == 1:
            for g in entries[0][1].findall('Group'):
                groups[g.text.strip()].add(k)
    while pending:
        k = min(pending)
        pending.remove(k)
        if k in done:
            continue
        done.add(k)
        assert len(source[k]) == 1, (hex(k), source[k])
        file, original = source[k][0]
        name = selected[k].attrib['id']
        if not any(c.tag in SERVER-{'XpMult'} for c in original) and selected[k].find('Enemy') is None and selected[k].findtext('Class') != 'Character':
            continue
        body = [copy.deepcopy(c) for c in original if c.tag in SERVER]
        changes = []
        if k==0x806:
            markers=[c for c in body if c.tag=='Loot' and not c.attrib and not (c.text or '').strip() and not len(c)]
            assert len(markers)==1 and selected[k].findtext('Class')=='Container'
            body.remove(markers[0])
            changes.append('omit only Tutorial Loot Bag0806 empty client Container Loot marker; selected/original property bytes preserved, not a server item/drop row')
        for n in body:
            for c in n.iter():
                for old, new in spellings.items():
                    if old in c.attrib:
                        assert new not in c.attrib
                        value = c.attrib.pop(old)
                        c.set(new, value)
                        changes.append(f'{old} -> {new} ({value})')
                for key in list(c.attrib):
                    value = c.attrib[key]
                    if key in refs and 'Bandit Enemy' in value:
                        c.set(key, value.replace('Bandit Enemy', 'Bandit'))
                        changes.append('exact selected Bandit 0x954 alias, not approximate name lookup')
                    if key == 'broadcast' and value in ('0', '1'):
                        c.set(key, 'true' if value == '1' else 'false')
                        changes.append(f'broadcast={value} normalized to explicit boolean')
        for n in body:
            for c in n.iter('Behavior'):
                for field in ('avoidWater','noCooldownJitter'):
                    if c.attrib.get(field) in ('0','1'):
                        value=c.attrib[field]
                        c.set(field,'true' if value=='1' else 'false')
                        changes.append(f'{field}={value} normalized to explicit documented boolean')
                if c.text.strip()=='Shoot' and c.attrib.get('predictive') in ('true','false'):
                    value=c.attrib['predictive']
                    c.set('predictive','1' if value=='true' else '0')
                    changes.append(f'Shoot predictive={value} normalized to documented numeric endpoint')
                if k==0x1740 and c.attrib.get('defaultAngle')=='22,5':
                    c.set('defaultAngle','22.5')
                    changes.append('authored decimal typo correction bulletDance/grenade4b defaultAngle22,5 ->22.5; same source mirrored22.5-degree pattern, original numeric malformed')
                if k==0x20d and c.text.strip()=='Explode' and 'projectileId' not in c.attrib:
                    c.set('projectileId','0')
                    changes.append('authored explicit Explode projectile0: sole selected Gray Blob Red Fire, implicit source slot now visible')
                if c.text.strip()=='Taunt' and 'listId' not in c.attrib:
                    lists={0x668:'Red Demon',0x675:'Phoenix Lord',0x678:'Oasis Giant',0x67d:'Cyclops God',0x6c1:'Phoenix Lord',0x925:'Deathmage',0x932:'Oryx the Mad God 2',0xd2c:'Inactive Sarcophagus'}
                    assert k in lists,(hex(k),name)
                    c.set('listId',lists[k])
                    changes.append('authored explicit missing Taunt listId -> '+lists[k]+'; same identity or Phoenix death/rebirth lineage, original implicit lookup unavailable')
                if c.text.strip()=='Follow' and 'predictive' in c.attrib and c.attrib['predictive'] not in ('true','false'):
                    value=c.attrib['predictive']
                    assert 0 <= float(value) <= 1
                    c.set('predictive','false' if float(value)==0 else 'true')
                    changes.append(f'authored Follow predictive{value} -> documented boolean nonzero=true (DOCX38); fractional original intent unresolved, no numeric strength claim')
                if k in (0x990,0x991) and c.attrib.get('cooldownJitter')=='2':
                    c.set('cooldownJitter','true')
                    changes.append('authored malformed Shoot cooldownJitter2 -> true under DOCX9/DECA boolean contract; original numeric jitter intent unresolved')
        if k == 0x954:
            changes.append('recovered Bandit Enemy is selected Bandit at the same numeric identity')
        if k==0xd61:
            no_pools=next(n for n in body if n.tag=='State' and n.attrib.get('id')=='NoPools')
            wait=no_pools[0]
            assert wait.tag=='State' and not wait.attrib
            assert wait[0].attrib=={'afterTime':'2'} and wait[0].text.strip()=='ForceTent'
            assert not any(n.attrib.get('id')=='WaitForTentacles' for n in no_pools)
            wait.set('id','WaitForTentacles')
            changes.append('authored explicit name WaitForTentacles for Hermit God/NoPools first unnamed child at encounters.xml1130; preserve first-child order, afterTime2->ForceTent and parent2.4->Base, original name unavailable')
        if k == 0xd64:
            for state in (c for n in body for c in n.iter('State')):
                if any(c.tag == 'Behavior' and c.text.strip() == 'Charge' for c in state):
                    leash = X.Element('Behavior', bucket='movement', protecteeId='Hermit God',
                                      acquireRange='15', protectionRange='10', reprotectRange='5', speed='.6')
                    leash.text = 'Protect'
                    state.insert(0, leash)
            changes.append('Authored #1465 parent leash: reacquire the living owned Hermit outside 10 tiles; bound charges and return toward the five-tile orbit')
        if k == 0x1706:
            part1 = next(n for n in body if n.tag == 'State' and n.attrib['id'] == 'Part1')
            wait = next(n for n in part1 if n.tag == 'State' and n.attrib['id'] == 'BigWait')
            for part in ('Part2', 'Part3'):
                next(n for n in body if n.tag == 'State' and n.attrib['id'] == part).append(copy.deepcopy(wait))
            changes.append('authored Part2/Part3 BigWait siblings: Part1 SetTag ini, 4s/playerWithin15 OR12s then Base')
        if k == 0xd81:
            embed = next(n for n in body if n.tag == 'DefineEmbed' and n.attrib['name'] == 'AttackOne')
            t = embed.find('Transition')
            assert t.text.strip() == 'Attack1'
            embed.remove(t)
            changes.append('remove dangling AttackOne -> Attack1; enclosing buzz exits to sibling search at5s')
        if k == 0x992:
            delete = [c for n in body for c in n.iter('Behavior') if c.text.strip() == 'DeleteObject']
            assert len(delete) == 1 and delete[0].attrib['objectId'] == 'Blue Torch Wall'
            delete[0].set('objectId', 'Lab Emergency Wall')
            changes.append('DeleteObject Blue Torch Wall -> Lab Emergency Wall: the selected Mad Lab seals the Horrific Creation with Lab Emergency Walls; the recovered opener otherwise unchanged')
        if k == 0xd19:
            for n in body:
                for c in n.iter('Behavior'):
                    if c.attrib.get('targetIds') == 'Jackal Master':
                        c.set('targetIds', 'Jackal Assassin')
            changes.append('PromoteRank2 heals Jackal Assassin, the same-source Veteran promotion destination; Jackal Master absent')
        # These unavailable projectile slots are authored adaptations, never client-property overlays.
        projectile_corrections = {0xd28: {'6': '2'}, 0xd3a: {'3': '2'},
                                  0xd3b: {'2': '0', '3': '1'}, 0xd7f: {'1': '0'}}
        if k in projectile_corrections:
            for n in body:
                for c in n.iter('Behavior'):
                    old = c.attrib.get('projectileId')
                    if old in projectile_corrections[k]:
                        new = projectile_corrections[k][old]
                        c.set('projectileId', new)
                        changes.append(f'authored unavailable projectile{old} -> selected{new}; retain timing/geometry, no historical equivalence claim')
        if k == 0x924:
            assert not any(n.tag in ('Loot', 'SoulboundLoot') for n in body)
            body.append(X.Element('SoulboundLoot', prob='.001', **qualify('authored', '79a4c92b:src/loot.rs:41',
                'Legacy-derived Quiver of Thunder; recovered testingObjects.xml has no loot for this object.')))
            body[-1].text = 'Quiver of Thunder'
            changes.append('authored legacy-derived drop: private Quiver of Thunder at .001 without a damage threshold restores 79a4c92b:src/loot.rs:41; recovered testingObjects.xml defines no loot for 0x924, which would leave the item unobtainable')
        executable = any(n.tag in ('Behavior', 'State', 'Order', 'Embed') for n in body)
        disposition = 'active' if executable else 'passive'
        if not executable:
            assert selected[k].findtext('Class') in ('Wall', 'GameObject') or k == 0xd39, (hex(k), name)
        reason = ('Recovered server rules only; selected client owns properties.' if executable else
                  'Stationary destructible scenery: selected HP/collision and recovered loot retained; no AI implied.')
        if k == 0xd39:
            reason = 'Explicitly disabled Tomb trap; distinct active switches/traps retain their executable definitions.'
        if k == 0x188a:
            reason += ' Tesla Coil remains a destructible creation-room gate participant, not an attack controller.'
        if file.name == 'testingObjects.xml':
            reason += ' Experimental/revised recovered family input; not authenticated123.5.1 behavior.'
        if changes:
            reason += ' Corrections: ' + '; '.join(dict.fromkeys(changes)) + '.'
        text = file.read_text()
        m = re.search(r'<Object\s+type="'+re.escape(original.attrib['type'])+r'"', text)
        line = text[:m.start()].count('\n')+1
        o = X.SubElement(result, 'Object', type=f'0x{k:04x}', id=name, disposition=disposition,
                         **qualify('corrected' if changes else 'recovered', f'{file}:{line}', reason))
        o.extend(body)
        if changes:
            corrections.append(dict(object=f'{k:04x}', name=name, origin=o.attrib['origin'], changes=list(dict.fromkeys(changes))))
        for n in o.iter():
            for field in refs & n.attrib.keys():
                for ref in n.attrib[field].split(','):
                    ref = ref.strip()
                    if '{' in ref:
                        continue
                    targets = {by_name[ref]} if ref in by_name else groups[ref]
                    assert targets, (hex(k), field, ref)
                    for target in targets:
                        # Native graphics/items/portals do not require invented AI.
                        if target in source and len(source[target]) == 1:
                            orig = source[target][0][1]
                            if any(c.tag in SERVER-{'XpMult'} for c in orig) or selected[target].find('Enemy') is not None:
                                pending.add(target)
                        elif selected[target].find('Enemy') is not None or selected[target].findtext('Class') == 'Character':
                            raise ValueError(f'missing actor source {target:x} {ref}')
    return result, corrections


if __name__ == '__main__':
    raise SystemExit('Use build.py to bind the selected production admission and geometry first.')
