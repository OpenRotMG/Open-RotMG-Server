"""Normalize the explicitly selected XML and authored geometry, without editing reference bytes."""
import collections
import copy
import json
import math
import re
import xml.etree.ElementTree as X
from pathlib import Path
from selection import DATA, OUT, ARCHIVE, SERVER, actors, by_name, qualify, save, selected, sha, source
from string_lists import select_string_lists
from native import append_native, wmap, SCRIPTED_FACTORY_ROOT

def wmap_size(file):
    return wmap(file)[:2]


geometry = X.Element('Geometry', schema='1')
qualifications = []


def q(origin, reason, provenance='authored'):
    return qualify(provenance, origin, reason)


def obj(k):
    return selected[k].attrib['id']


def template(name, rows, legend, origin, reason, spawns=(), anchor=None):
    width = max(map(len, rows))
    t = X.SubElement(geometry, 'Template', id=name, width=str(width), height=str(len(rows)),
                     rotation='fixed', rooms='1', **q(origin, reason))
    used = set(''.join(rows)) - {' '}
    for char in sorted(used):
        X.SubElement(t, 'CharMap', char=char, **legend[char])
    pattern = X.SubElement(t, 'Pattern')
    for row in rows:
        X.SubElement(pattern, 'Row', value=row.ljust(width))
    for x, y in spawns:
        X.SubElement(t, 'Spawn', x=str(x), y=str(y))
    if anchor:
        X.SubElement(t, 'Anchor', x=str(anchor[0]), y=str(anchor[1]))
    return t


# These remain actual source-qualified candidate assets, not recovered official layouts.
static = ['tutorial', 'kitchen', 'nexus', 'nexus-limbo', 'vault', 'oryx-castle',
          'oryx-chamber', 'wine-cellar', 'guild-hall-1', 'guild-hall-2', 'guild-hall-3', 'guild-hall-4']
for name in static:
    file = DATA/f'{name}.wmap'
    width, height = wmap_size(file)
    X.SubElement(geometry, 'Map', name=name, file=file.name, sha256=sha(file), format='wmap',
                 width=str(width), height=str(height),
                 **q(f'data/{file.name}; data/README.md; required notices in game.xml',
                     'Existing selected candidate geometry, preserved byte-for-byte; not authenticated123.5.1 layout.'))

catalogue = json.loads((DATA/'realm-catalog.json').read_text())
realm_entries = []
for entry in catalogue:
    if entry['kind'] == 'baseline':
        name = entry['id']
        file = DATA/entry['path']
        width, height = wmap_size(file)
        X.SubElement(geometry, 'Map', name=name, file=entry['path'], sha256=entry['sha256'],
                     format='realm-wmap', width=str(width), height=str(height),
                     **q('data/realm-catalog.json; data/README.md; FSOD-NOTICE', 'Existing candidate Realm cells, not authenticated official layout.'))
    else:
        file = DATA/'realms'/entry['recipe']
        recipe = json.loads(file.read_text())
        name = recipe['id']
        X.SubElement(geometry, 'Reconstruction', id=name, recipe=entry['recipe'], sha256=sha(file),
                     outputSha256=entry['sha256'], width=str(recipe['width']), height=str(recipe['height']),
                     **q(str(file), recipe['provenance']['status']))
    realm_entries.append(name)
pool = X.SubElement(geometry, 'Pool', id='realm', seed='world', expectedCount='16',
                    **q('data/realm-catalog.json; terrain::realms selection', 'Preserve all16 existing layouts and caller-owned world RNG; weights1 are authored equal selection, not historical rates.'))
for name in realm_entries:
    X.SubElement(pool, 'Entry', map=name, weight='1')

# Normalize all76 actual stamp structures. Marker decisions below are explicit authored
# constraints, not the synthetic socket choices used by compiler tests.
for family in ('lab', 'manor', 'davy', 'spider'):
    lib = json.loads((DATA/f'{family}-templates.json').read_text())
    for name, record in lib['templates'].items():
        rows = record['rows']
        legend = {}
        sockets = []
        spawns = []
        for char in sorted(set(''.join(rows))-{' '}):
            ground, object_name = lib['legend'][char]
            ground = ground or ('Empty' if family != 'davy' else 'Space')
            tile = dict(ground=ground)
            positions = [(x, y) for y, row in enumerate(rows) for x, c in enumerate(row) if c == char]
            marker = object_name in ('table slot', 'vat field', 'secret shelf', 'key slot', 'door slot', 'loose wall')
            if marker:
                socket_id = f'{family}-{name}-{ord(char)}'
                tile['socket'] = socket_id
                if object_name == 'table slot':
                    lo, hi, scope, choices = 0, 2, 'room', [dict(object='Lab Table')]
                    kind = 'table'
                elif object_name == 'vat field':
                    lo = hi = len(positions)
                    scope, choices, kind = 'room', [dict(preserve='true')], 'vat-field'
                elif object_name == 'secret shelf':
                    lo = hi = len(positions)
                    scope, choices, kind = 'room', [dict(object=obj(0x181a))], 'secret-shelf'
                elif object_name == 'key slot':
                    lo = hi = 1
                    scope, kind = 'room', 'key'
                    choices = [dict(object=obj(k)) for k in (0xe3e, 0xe3f, 0xe40, 0xe41)]
                elif object_name == 'door slot':
                    lo = hi = len(positions)
                    scope, kind = 'room', 'door'
                    choices = [dict(object=obj(k)) for k in (0x1868, 0x1869, 0x186a, 0x186b, 0x186c, 0x186d, 0x186e, 0x186f)]
                else:
                    lo, hi, scope, kind = 0, len(positions), 'room', 'loose-wall'
                    choices = [dict(object='Spider Dirt Wall')]
                sockets.append((socket_id, kind, scope, lo, hi, choices))
            elif object_name == 'spawn':
                spawns.extend((x+.5, y+.5) for x, y in positions)
                if family == 'manor':
                    tile['object'] = obj(0x703)
            elif object_name:
                assert object_name in by_name, object_name
                tile['object'] = object_name
            legend[char] = tile
        if family=='lab':
            # The captured stamps omit invisible actors; materialize bounded3x3
            # pool coverage as XML cells, not a per-Lab runtime dispatch escape.
            mutable=[list(row) for row in rows]
            for ground,actor in [('Lab Bad Vat Rug','Hexxer'),('Lab Good Vat Rug','DeHexxer')]:
                wet={(x,y) for y,row in enumerate(rows) for x,ch in enumerate(row)
                     if ch!=' ' and lib['legend'][ch][0]==ground and lib['legend'][ch][1] in ('','vat field')}
                covered=set()
                for point in sorted(wet,key=lambda p:(p[1],p[0])):
                    if point in covered:continue
                    near=lambda p:{(p[0]+a,p[1]+b) for a in (-1,0,1) for b in (-1,0,1)}
                    candidates=near(point)&wet
                    best=max(sorted(candidates),key=lambda p:(len((near(p)&wet)-covered),-len(near(p)-wet)))
                    original_char=mutable[best[1]][best[0]]
                    symbol=next(chr(i) for i in range(0x100,0x400) if chr(i) not in legend)
                    legend[symbol]=dict(legend[original_char],object=actor)
                    mutable[best[1]][best[0]]=symbol
                    covered.update(near(best))
            rows=[''.join(row) for row in mutable]
        reason = ('Exact source rows/ports/rotation/room-equivalence and pack identities; candidate provenance in data/README.md. '
                  + record.get('note', '') + ' Authored Lab pool coverage adds explicit Hexxer/DeHexxer cells on open matching vats, bounded3x3 cover under src/dungeon/lab.rs qualified source; exact source floors/objects/ports otherwise retained. Authored marker semantics: table0..2/room from prior qualified estimate; vat-field marks actor-only effect cells with preserved floor; secret-shelf fills all marked cells; key1/room and complete selected key/door candidate pools require generation-time matched-color path constraints, not independent random colors; loose-wall0..all/room. These are not recovered official generator rules.')
        t = template(f'{family}-{name}', rows, legend, f'data/{family}-templates.json:{name}; data/README.md', reason, spawns)
        t.set('rotation', 'fixed' if record.get('fixed') else 'quarter-turns')
        t.set('rooms', str(record.get('rooms', 1)))
        for socket_id, kind, scope, lo, hi, choices in sockets:
            s = X.SubElement(t, 'Socket', id=socket_id, kind=kind, scope=scope, min=str(lo), max=str(hi))
            for choice in choices:
                X.SubElement(s, 'Choice', weight='1', **choice)
        for side, offset in record.get('ports', []):
            X.SubElement(t, 'Port', side=dict(N='north', S='south', E='east', W='west')[side], offset=str(offset), width='4')
        for member in record.get('pack', []):
            X.SubElement(t, 'Population', object=member, min='1', max='1')

# No original .jm bytes survived. These explicit room bindings are authored use of
# separately qualified candidate stamps, never a closest-name or empty-map fallback.
# A distinct record for each absent name keeps uncertainty visible at the use site.
missing_bindings = {}

def stamp_binding(name, candidate, reason):
    assert name not in missing_bindings
    original = next(t for t in geometry if t.attrib.get('id') == candidate)
    clone = copy.deepcopy(original)
    clone.set('id', name)
    clone.attrib.update(q(f'authored absent {name}; {original.attrib["origin"]}', reason+' '+original.attrib['reason']))
    geometry.append(clone)
    missing_bindings[name] = candidate


# Missing room resources are deliberately selected by explicit reconstruction tables.
lab_map = {
    'start':'start', 'boss_main':'boss', 'frogger':'acid', 'frogger1':'walkway_cross',
    'frogger2':'vat_hall', 'frogger3':'pair', 'frogger4':'cluster', 'main9':'big_room',
    'main1':'vat_hall', 'main2':'pair', 'main3':'table_room', 'main6':'cross_hollow',
    'main12':'big_room', 'main15':'walkway_cross', 'main16':'maze', 'main19_L_rd':'l_room',
    'main19_T_rdu':'cross_hollow', 'main19_X':'cluster', 'main4':'table_room',
    'main5':'vat_hall', 'main7':'pair', 'main8':'maze', 'main10':'big_room', 'main11':'cross_hollow',
    'main13':'walkway_cross', 'main14':'table_room', 'main17':'vat_hall', 'main18':'pair'}
for name, candidate in lab_map.items():
    stamp_binding(f'mad_lab/{name}.jm', f'lab-{candidate}', 'Authored Lab room reconstruction from selected123.5.1 wiki-derived stamp; source name alone does not recover the lost geometry.')

davy_map = {'start_v':'start', 'start_h':'start_b', 'boss':'boss', 'room_back_1':'closet',
            'hall_h2a':'H24', 'hall_v2a':'V24', 'hall_h3a':'H36', 'hall_v3a':'V36', 'hall_h4a':'H48', 'hall_v4a':'V48'}
for color in ('green', 'red', 'yellow'):
    for orient in ('h', 'h2', 'v', 'v2'):
        davy_map[f'{color}_{orient}'] = {'h':'rug', 'h2':'map', 'v':'chapel', 'v2':'armoury'}[orient]
for i, name in enumerate(['rug', 'map', 'chapel', 'table_rug', 'armoury', 'puddle', 'table'], 1):
    davy_map[f'room_h_{i}'] = name
for i, name in enumerate(['brig', 's10x16', 's12x13', 'e22x12', 'e19x12', 'e16x10', 'e20x14', 's12x22'], 1):
    davy_map[f'room_v_{i}'] = name
for name, candidate in davy_map.items():
    stamp_binding(f'ghost_ship/{name}.jm', f'davy-{candidate}', 'Authored Locker room reconstruction from qualified captured/minimap stamp. Color assignment is a generation constraint, not evidence for this absent original name.')
manor_names = list(json.loads((DATA/'manor-templates.json').read_text())['templates'])
for i in range(1,31):
    stamp_binding(f'gothic/{i:03}.jm', 'manor-'+manor_names[i+2], 'Authored Manor room selection from qualified candidate stamps; numeric source filename has no recovered geometry.')
for name, candidate in {'start1':'start','start2':'start_moat','start3':'start','bossroom':'boss','016_secret':'vault','018_secret':'vault_b'}.items():
    stamp_binding(f'gothic/{name}.jm', f'manor-{candidate}', 'Authored Manor entrance/boss/secret room reconstruction, not recovered original geometry.')
stamp_binding('spider_den/spiderden_boss.jm', 'spider-arena', 'Authored binding to separately qualified captured Spider arena; no original .jm survives.')

# Small missing arena/transition stamps are independently authored from selected
# geometry roles. They contain the actual named encounter actors/floors, not blanks.
def arena(name, ground, wall=None, boss=None, radius=10, spawn=False, motif=None):
    n = radius*2+1
    rows = [[' ']*n for _ in range(n)]
    legend = {'.':dict(ground=ground)}
    if wall:
        legend['#'] = dict(ground=ground, object=wall)
    for y in range(n):
        for x in range(n):
            d = ((x-radius)**2+(y-radius)**2)**.5
            if d <= radius:
                rows[y][x] = '#' if wall and d > radius-1 else '.'
    if boss:
        legend['B'] = dict(ground=ground, object=boss)
        rows[radius][radius] = 'B'
    if motif:
        mg, mo = motif
        legend['m'] = dict(ground=mg)
        if mo:
            legend['m']['object'] = mo
        for dx, dy in ((-4,0),(4,0),(0,-4),(0,4)):
            rows[radius+dy][radius+dx] = 'm'
    t = template(name, [''.join(r) for r in rows], legend,
                 f'authored absent {name}; selected-client definitions; data/README.md candidate geometry qualifications',
                 'Independently authored bounded circular arena with explicit selected ground/actor/cover. Original map bytes absent; radius and placement are authored, not authenticated historical values.',
                 [(radius+.5,radius+4.5)] if spawn else [], (radius,radius))
    missing_bindings[name] = 'authored circle with explicit selected features'
    return t

arenas = {
 'jungle/jungle_boss.jm':('Dirt','Jungle Tree Big','Mixcoatl the Masked God',12),
 'ocean/oceanStart.jm':('OceanFloor','Ocean Wall','Portal of Cowardice',8),
 'ocean/oceanBoss.jm':('OceanFloor','Ocean Wall','Thessal the Mermaid Goddess',15),
 'undead_lair/undead_lair_start.jm':('Grey Closed','Grey Wall','Portal of Cowardice',8),
 'undead_lair/undead_lair_boss.jm':('Grey Closed','Grey Wall','Septavius the Ghost God',12),
 'abyss/abyss_boss.jm':('Red Small Checks','Red Wall','Archdemon Malphas',12),
 'abyss/abyss_treasure.jm':('Red Small Checks','Red Wall','Abyss Idol',10),
 'abyss/abyss_branchend.jm':('Red Small Checks','Red Wall','Demon',8),
 'snake_pit/snakepit_start.jm':('Brown Closed','Brown Wall','Portal of Cowardice',8),
 'snake_pit/snakepit_boss.jm':('Brown Closed','Brown Wall','Stheno the Snake Queen',12),
 'snake_pit/snakepit_treasure.jm':('Brown Closed','Brown Wall','Snakepit Guard Spawner',10),
 'snake_pit/snakepit_branchend1.jm':('Brown Closed','Brown Wall','Snakepit Guard Spawner',8),
 'snake_pit/snakepit_branchend2.jm':('Brown Closed','Brown Wall','Snakepit Guard Spawner',9),
 'tombOfTheAncientsBoss.jm':('Sandstone Tile','Tomb Wall','Tomb Boss Anchor',16),
}
for name, (ground, wall, boss, radius) in arenas.items():
    t=arena(name, ground, wall, boss, radius, spawn=('start' in name.lower() or name in ('beachzone.jm','tombOfTheAncientsBoss.jm')))
    if name=='jungle/jungle_boss.jm':
        X.SubElement(t,'CharMap',char='T',ground='Dirt',object='Boss Totem')
        for row_index,entry in enumerate(t.find('Pattern')):
            row=list(entry.attrib['value'])
            for dx,dy in [(-5,-5),(5,-5),(-5,5),(5,5)]:
                if row_index==radius+dy:row[radius+dx]='T'
            entry.set('value',''.join(row))
        t.set('reason',t.attrib['reason']+' Authored four Boss Totem corners at +/-5 preserve existing qualified jungle generator caller placements; no boss-wave dispatch in Rust.')


# Encounter overlays do not reproduce the emitting actor, so placement cannot
# recursively duplicate its source encounter.
def overlay(name, rows, legend, reason):
    return template(name, rows, legend,
                    f'authored absent {name}; recovered PlaceMap call and selected properties',
                    'Authored concrete overlay: '+reason+'; exact lost geometry unavailable, no historical acceptance.',
                    anchor=(len(rows[0])//2,len(rows)//2))

for name, ground, decoration in [
    ('encounter_maps/sphinx_map.jm','Light Sand','Sphinx Artifact 1'),
    ('encounter_maps/skull_shrine_map.jm','Dark Cobblestone','Rock Grey'),
    ('encounter_maps/pentaract_map.jm','Grass','Pentaract Tower Corpse'),
    ('ghost_ship/encounter/beachbum.jm','Light Sand','Palm Tree'),
    ('ghost_ship/encounter/wp_full_lake2.jm','Dark Water','Beach Spectre Spawner'),
    ('ghost_ship/encounter/storm_clouds.jm','Dark Water','Tempest Cloud'),
    ('ghost_ship/encounter/davyisland.jm','Light Sand',"Davy Jones's Locker Portal"),
    ('jungle/totem_floor.jm','Undergrowth','Jungle Ground Flowers'),
    ('courtyard_maps/map1.jm','Lab Floor','Lab Equipment Wall')]:
    # Existing event ownership supplies its own boss/towers; decoration only.
    if name=='encounter_maps/pentaract_map.jm': decoration=None
    if name=='jungle/totem_floor.jm': decoration=None
    t=arena(name,ground,None,None,8,motif=(ground,decoration))
    if name=='encounter_maps/skull_shrine_map.jm':
        t.set('reason',t.attrib['reason']+' Authored correction: Dark Stone and Stone Statue were nonexistent names; explicitly select client Dark Cobblestone0x01 and Rock Grey0x107 as this new arena floor/cover, not recovered aliases.')

# Preserve the existing8-spawner Hermit site through concrete typed placements.
rows=[list('.'*32) for _ in range(29)]
for x,y in [(13,8),(20,17),(16,8),(18,8),(10,13),(21,13),(11,17),(15,18)]:rows[y][x]='T'
rows[14][16]='D'
overlay('ocean/oceanEncounter.jm',[''.join(row) for row in rows],
        {'.':dict(ground='Dark Water'),'T':dict(ground='Dark Water',object='Hermit God Tentacle Spawner'),'D':dict(ground='Dark Water',object='Hermit God Drop')},
        'authored water site retains8existing qualified ocean_setpiece.rs spawner offsets; emitter/ownership supplies Hermit God and timed source Whirlpool children; recovered Hermit God Drop (encounters.xml:1571) at the anchor turns into the Ocean Trench Portal once no Hermit God exists')

for name, ground in [('ghost_ship/floorlight.jm','GhostGround Light'),('ghost_ship/floordark.jm','GhostGround Dark')]:
    overlay(name,['.'*23 for _ in range(23)],{'.':dict(ground=ground)},'23-square lantern illumination field; preserve objects')
for color, k in [('purple',0x186a),('green',0x186c),('red',0x1868),('yellow',0x186e)]:
    for orientation in ('h','v'):
        rows=['LR'] if orientation=='h' else ['L','R']
        overlay(f'ghost_ship/doors/{color}_{orientation}.jm',rows,
                {'L':dict(ground='GhostGround Mid',object=obj(k+1)), 'R':dict(ground='GhostGround Mid',object=obj(k))},
                'selected paired closed door halves; color and orientation explicit')
# Guard/button/dart actors and lava/exit listeners exist in these authored stamps,
# not merely in a source filename inventory.
rows=[list('.'*25) for _ in range(25)]
legend={'.':dict(ground='Red Small Checks'),'L':dict(ground='Red Small Checks',object='AbyssTreasureLavaBomb'),'G':dict(ground='Red Small Checks',object='AbyssExitGuarder')}
for x,y in [(8,8),(12,8),(16,8),(8,12),(16,12),(8,16),(12,16),(16,16)]:rows[y][x]='L'
for x,y in [(0,12),(24,12),(12,0),(12,24)]:rows[y][x]='G'
overlay('abyss/abyss_treasure_overlay.jm',[''.join(r) for r in rows],legend,'25-square treasure arena,8 owned lava cells and4 exit listeners; no duplicate Idol')
for step in range(6):
    rows=[]
    for y in range(5):
        rows.append(''.join('L' if abs(x-2)+abs(y-2)<=step and step else '.' for x in range(5)))
    overlay(f'abyss/abyss_lb{step}.jm',rows,{'.':dict(ground='Red Small Checks'),'L':dict(ground='Lava')} if step else {'.':dict(ground='Red Small Checks')},f'authored progressive lava exposure stage{step}; source state timing remains unchanged')
for step in (1,2,3):
    attrs=dict(ground='Red Small Checks')
    if step==2:attrs['object']='Red Wall'
    if step==3:attrs['clearObject']='true'
    overlay(f'abyss/exit{step}.jm',['...']*3,{'.':attrs},f'exit transition{step}: warning floor, closed wall, then explicit reopened object layer')
rows=[list('.'*23) for _ in range(23)]
legend={'.':dict(ground='Brown Closed'),'B':dict(ground='Brown Closed',object='Snakepit Button'),'D':dict(ground='Brown Closed',object='Snakepit Dart Thrower')}
rows[11][5]='B'
for y in (6,16):
    for x in (6,11,16):rows[y][x]='D'
overlay('snake_pit/snakepit_treasure2.jm',[''.join(r) for r in rows],legend,'authored rail chamber with1 button and6 dart actors; emitter supplies its guard')

# Beach is genuinely unseeded. Retain the existing candidate's island,16 umbrellas,
# 32 towels, partial wooden stage and4 braziers as concrete authored cells.
ground_names={int(n.attrib['type'],16):n.attrib['id'] for n in X.parse(DATA/'grounds.xml').getroot()}
width,height=136,127
cells=[]
for y in range(height):
    row=[]
    for x in range(width):
        dx,dy=(x-70)/1.07,y-65
        a=math.atan2(dy,dx)
        r=math.hypot(dx,dy)+1.5*math.sin(a*3)+math.cos(a*5)
        ground=0xff if r>65 else 0xb1 if r>51 else 0xb2 if r>44 else 0xb3 if r>34 else 0xf5 if r>22 else 0xbe if r>20 else 0x23
        row.append((ground,None))
    cells.append(row)
for n in range(16):
    a=n*math.tau/16
    x,y=int(70+29*math.cos(a)),int(65+27*math.sin(a))
    cells[y][x]=(0xf5,0x18a7)
    for dx in (-2,2):cells[y+1][x+dx]=(0xb4 if n%2==0 else 0xb5,None)
for y in range(60,73):
    for x in range(65,78):
        if math.hypot(x-71,y-66)<=6:cells[y][x]=(0xf5,None)
for x,y in [(70,66),(71,65),(72,66),(71,67)]:cells[y][x]=(0xb6,None)
for x,y in [(68,63),(74,63),(68,69),(74,69)]:cells[y][x]=(0xf5,0xdc6)
cells[66][71]=(0xb6,0xe56)
cells[65][44]=(0xf5,0x703)
choices=sorted(set(c for row in cells for c in row),key=lambda c:(c[0],c[1] or 0))
alphabet='0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!$%&()*+,-./:;<=>?@[]^_{}~'
assert len(choices)<=len(alphabet)
symbols=dict(zip(choices,alphabet))
legend={symbols[c]:dict(ground=ground_names[c[0]],**({'object':obj(c[1])} if c[1] else {})) for c in choices}
template('beachzone.jm',[''.join(symbols[c] for c in row) for row in cells],legend,
         'authored Beach candidate from src/dungeon/beach.rs::generate; selected123.5.0 guide citation and data/README.md',
         'Unseeded authored island geometry preserving16 Beach Umbrellas,32 towels,4 braziers,5 Bridge Light stage cells, Masked Party God and living return; no Beach Ball or palm placement is claimed; formula normalized independently, not authenticated original cells. Source dungeons.xml width256/centerStartRoom remain generator-owned.',
         [(46.5,65.5)],(70,65))
missing_bindings['beachzone.jm']='authored unseeded qualified Beach candidate'

world_sources = [
 ('Mad Lab','mad-lab','lab'), ("Davy Jones's Locker",'davy-jones-locker','davy'),
 ('Manor of the Immortals','manor-of-the-immortals','manor'), ('Forbidden Jungle','forbidden-jungle','jungle'),
 ('Ocean Trench','ocean-trench','trench'), ('Pirate Cave','pirate-cave','pirate'),
 ('Spider Den','spider-den','spider'), ('Undead Lair','undead-lair','undead'),
 ('Abyss of Demons','abyss-of-demons','abyss'), ('Snake Pit','snake-pit','snake'),
 ('Sprite World','sprite-world','sprite'), ('Tomb of the Ancients','tomb-of-the-ancients','tomb'),
 ('Beachzone','beachzone','beach')]
dungeons = X.parse(ARCHIVE/'dungeons.xml').getroot()
metadata = X.Element('WorldServices', schema='1')


def text(node, tag, default=None):
    return node.findtext(tag, default).strip() if node.findtext(tag, default) is not None else None


def exact_names(value):
    # Explicit known source spellings, not fuzzy lookup.
    return {'Bandit Enemy':'Bandit'}.get(value, value)


def population(parent, src):
    name = exact_names(src.text.strip())
    attrs = {('object' if name in by_name else 'group'): name, 'min':src.attrib.get('minNum','1'), 'max':src.attrib.get('maxNum','1')}
    if 'prob' in src.attrib:
        attrs['prob'] = src.attrib['prob']
    if 'position' in src.attrib:
        attrs['position'] = {'centerPlusSalt':'center-plus-salt', 'randomFocus':'random-focus'}.get(src.attrib['position'], src.attrib['position'])
    if 'ground' in src.attrib:
        attrs['ground'] = src.attrib['ground']
    X.SubElement(parent,'Population',**attrs)


for source_name, slug, kind in world_sources:
    d = next(d for d in dungeons if d.attrib.get('id') == source_name)
    attrs = dict(id=slug, kind=kind, seed='none' if kind=='beach' else 'world',
                 width=text(d,'Width'), height=text(d,'Height'), padding='1', start='start',
                 boss='start' if kind=='beach' else 'end',
                 center='map' if d.find('CenterMap') is not None else 'start' if d.find('CenterStartRoom') is not None else 'none')
    reason = 'Selected recovered dungeon dimensions/branches/corridors/room probabilities/placements and paint literals; missing assets visibly authored. Geometry defaults below are authored, not recovered engine facts. Explicit Room MaxCount is retained; only absent caps default to1 for start/end and4096 for other room categories as an authored safety bound. Room padding carries literal MinPadding as the minimum spacing constraint; only absence uses authored1, never replacing a specified0. Explicit paired room hallway lengths remain corridor overrides including0..0; absent pairs inherit the generator span without a fabricated room value.'
    if kind != 'beach':
        for src, lo, hi, default in [('MainBranchLen','mainBranchMin','mainBranchMax','1'),('NumSubBranches','subBranchesMin','subBranchesMax','0')]:
            attrs[lo]=attrs[hi]=text(d,src,default)
        for src, lo, hi, default in [('SubBranchLen','subBranchMin','subBranchMax','1'),('HallwayLen','corridorLengthMin','corridorLengthMax','1'),('HallwayWidth','corridorWidthMin','corridorWidthMax','1')]:
            attrs[lo]=text(d,'Min'+src,default)
            attrs[hi]=text(d,'Max'+src,default)
        for src, target in [('RandomizeGlobalReplaceTiles','randomizeReplacements'),('RemoveHallwayEndObjects','removeHallwayEndObjects')]:
            if d.find(src) is not None:
                attrs[target]='true'
        if d.find('InsurePath') is not None:
            attrs['ensurePath']='true'
            if text(d,'InsurePath'):
                attrs['pathObject']=text(d,'InsurePath')
    if kind=='beach':
        reason += ' Authored background explicitly selects client Tropical Water Deep0xb1, the surrounding water used by the existing qualified Beach candidate, not nonexistent Deep Water.'
    g = X.SubElement(geometry,'Generator',**attrs,**q(f'data/reference/release-7.0/xml/dungeons.xml:{source_name}',reason,'corrected'))
    tile_roles = [('DefaultTile','DefaultObj','background'),('HallwayTile','HallwayObj','hallway'),('WallTile','WallObj','wall'),('HallwayEndTile',None,'hallway-end'),(None,'EndCapObj','end-cap'),('SurroundWith',None,'surround')]
    for ground_tag, object_tag, role in tile_roles:
        ground = text(d,ground_tag) if ground_tag else None
        container = d.find(object_tag) if object_tag else None
        objects = [] if container is None else list(container)
        weighted = any('prob' in o.attrib for o in objects) or len(objects) > 1
        if ground or objects or role in ('background','hallway') and kind!='beach':
            a=dict(role=role)
            if ground:
                a['ground']=ground
            if not weighted and objects:
                a['object']=objects[0].text.strip()
            if not ground and role in ('background','hallway'):
                a['ground']='Composite' if role=='background' else 'Grass'
            tile=X.SubElement(g,'Tile',**a)
            if weighted:
                for o in objects:
                    X.SubElement(tile,'Choice',prob=o.attrib.get('prob','1'),object=o.text.strip())
    if kind=='beach' and not any(n.attrib.get('role')=='background' for n in g):
        X.SubElement(g,'Tile',role='background',ground='Tropical Water Deep')
    for c in d:
        if c.tag in ('GlobalReplaceTile','GlobalPlaceObject'):
            f={'perlinProb':'perlin-prob','perlinMod':'perlin-mod','aroundSpawn':'around-spawn','randomLinesX':'random-lines-x','randomLinesY':'random-lines-y','cgNoise':'cg-noise'}[c.attrib['func']]
            a={k:v for k,v in c.attrib.items() if k not in ('func','groundRestrict')}
            a['distribution']=f
            if c.tag=='GlobalReplaceTile':
                a['ground']=c.text.strip()
                if 'groundRestrict' in c.attrib:a['from']=c.attrib['groundRestrict']
                X.SubElement(g,'Replace',**a)
            else:
                a.update(object=c.text.strip(),max='4096')
                if 'groundRestrict' in c.attrib:a['on']=c.attrib['groundRestrict']
                X.SubElement(g,'Scatter',**a)
        if c.tag=='AltWallType':
            a=dict(c.attrib)
            a['object']=c.text.strip()
            X.SubElement(g,'WallVariant',**a)
    room_counts=collections.Counter()
    for room in d.findall('Room'):
        base=room.attrib['id']
        index=room_counts[base];room_counts[base]+=1
        rid=base if index==0 else f'{base}-{index}'
        types=room.findall('Type')
        if len(types)>1:
            # Split the explicitly weighted shape alternatives without discarding either.
            variants=[(c.text.strip(),c.attrib['prob']) for c in types]
        else:
            variants=[(text(room,'Type'), '1')]
        for shape_i,(shape,shape_prob) in enumerate(variants):
            a=dict(id=rid if shape_i==0 else f'{rid}-shape-{shape_i}',weight='1' if kind=='beach' else str(round(float(text(room,'Prob','1'))*float(shape_prob)*10000)),maxCount=text(room,'MaxCount','1' if base in ('start','end') else '4096'),padding=text(room,'MinPadding','1'))
            if room.find('MinHallwayLen') is not None or room.find('MaxHallwayLen') is not None:
                assert room.find('MinHallwayLen') is not None and room.find('MaxHallwayLen') is not None, (source_name,rid,'unpaired corridor override')
                a.update(corridorLengthMin=text(room,'MinHallwayLen'),corridorLengthMax=text(room,'MaxHallwayLen'))
            local=[c.text.strip() for c in room if c.tag in ('LocalJM','AltLocalJM')]
            if local:
                a['shape']='template'
            elif shape=='Circle':
                a.update(shape='circle',radiusMin=str(int(float(text(room,'MinCircleRadius')))),radiusMax=str(int(float(text(room,'MaxCircleRadius')))),floor=text(room,'FloorTile'))
            elif shape=='Rectangle':
                a.update(shape='rectangle',widthMin=text(room,'MinRectLen'),widthMax=text(room,'MaxRectLen'),heightMin=text(room,'MinRectLen'),heightMax=text(room,'MaxRectLen'),floor=text(room,'FloorTile'))
            else:
                raise ValueError((source_name,room.attrib,shape))
            r=X.SubElement(g,'Room',**a)
            for resource,count in collections.Counter(local).items():
                assert resource in missing_bindings, resource
                X.SubElement(r,'Variant',template=resource,weight=str(count))
            for group in room.findall('Enemies'):
                mass=sum(float(n.attrib.get('prob','1')) for n in group)
                pg=X.SubElement(r,'PopulationGroup',selection='weighted' if len(group)>1 and mass<=1.00000001 else 'independent')
                if len(group)>1 and mass>1.00000001:
                    g.set('reason',g.attrib['reason']+' Authored group allocation: source Enemies mass'+str(mass)+' is overfull, so retain rows as independent draws (not renormalized weights); original engine allocation unavailable.')
                for member in group:population(pg,member)
    services=X.SubElement(metadata,'World',id=slug,**q(f'data/reference/release-7.0/xml/dungeons.xml:{source_name}', 'Recovered world-service settings reserved for explicit world integration, not inert geometry parameters.','recovered'))
    for tag in ('Visibility','Difficulty','Background','UsesBreath','HelloNotification'):
        if d.find(tag) is not None:
            node=X.SubElement(services,tag);node.text=text(d,tag)

# Build the selected actor closure after geometry contributes its actual placement identities.
fixture=json.loads(Path('tests/fixtures/xml_runtime/admission.json').read_text())
ids={int(k,16) for k in fixture['profiles']}
for k in map(lambda v:int(v,16),fixture['placed']):
    if selected[k].find('Enemy') is not None or any(c.tag in SERVER-{'XpMult'} for c in source[k][0][1]):ids.add(k)
for n in geometry.iter():
    for field in ('object',):
        if field in n.attrib:
            name=n.attrib[field]
            if name in by_name:
                k=by_name[name]
                if k in source and (selected[k].find('Enemy') is not None or any(c.tag in SERVER-{'XpMult'} for c in source[k][0][1])):ids.add(k)
    if n.tag=='Population':
        if 'group' in n.attrib:
            group=n.attrib['group']
            members={k for k, entries in source.items() if len(entries)==1 and any(c.text.strip()==group for c in entries[0][1].findall('Group'))}
            assert members, group
            ids.update(members)
ids.add(SCRIPTED_FACTORY_ROOT)
actor_root, corrections=actors(ids)
save('geometry.xml',geometry)
Path('tests/fixtures/xml_runtime/corrections.json').write_text(json.dumps(corrections,indent=2)+'\n')
print('selected actor definitions',len(actor_root),'geometry bindings',len(geometry))
catalog_roots=append_native(actor_root,geometry)
save('actors.xml',actor_root)
print('explicit native definitions',sum(n.attrib['disposition']=='native' for n in actor_root))

manifest=X.Element('Game',schema='1')
X.SubElement(manifest,'Include',file='actors.xml')
X.SubElement(manifest,'Include',file='strings.xml')
X.SubElement(manifest,'Geometry',file='geometry.xml')
# Normalization must not regenerate or discard manually reviewed placement/route intent.
authored_game = X.parse(OUT / 'game.xml').getroot()
manifest.append(copy.deepcopy(authored_game.find('RealmRules')))
authored_worlds = {world.attrib['id']: world for world in authored_game.findall('World')}
roots={int(line,16) for line in Path('tests/fixtures/xml_contract/ordinary-roots.txt').read_text().split() if line and not line.startswith('#')}
roots.update(int(k,16) for k in '601 604 953 60d 617 61a 609 614 652 905 62c 61f 649 640 900 645 650 632 633 62d 62e 62f 631 623 947 945 637 206 20e 950 957 903 7f00 91b 91f 928 675 678 925 668 67d d56 d59 d5f d50 d54 d61 e37 dc2 6db e55 935'.split())
for k in sorted(roots|catalog_roots):X.SubElement(manifest,'Root',object=obj(k))
progress=json.loads(Path('tests/fixtures/xml_contract/progress.json').read_text())
clear={slug:key for slug,boss,key in progress}
exits={'pirate-cave','spider-den','forbidden-jungle','snake-pit','sprite-world','undead-lair','abyss-of-demons','tomb-of-the-ancients','ocean-trench','manor-of-the-immortals'}
for slug in static+['realm']+[slug for _,slug,_ in world_sources]:
    attrs=dict(id=slug,map=slug)
    if slug in clear:attrs['clearKey']=clear[slug]
    if slug in exits:attrs['exitKey']=slug.replace('-','_')
    w=X.SubElement(manifest,'World',**attrs)
    for rule in authored_worlds[slug]:
        if rule.tag in {'Instance', 'Clear'}:
            w.append(copy.deepcopy(rule))
    services=next((n for n in metadata if n.attrib['id']==slug),None)
    if services is not None:
        for field in services:
            child=copy.deepcopy(field)
            child.attrib.update(q(services.attrib['origin'],services.attrib['reason'],'recovered'))
            w.append(child)
    if slug=='realm':
        for k in sorted(roots):
            X.SubElement(w,'Population',object=obj(k),min='0',max='1')
# Landmark chests preserve the reviewed pre-XML Rust tables (src/loot/legacy.rs chest_table at 3e6c5507) as authored rows.
SLOTS={'Weapon':[1,2,3,8,17],'Armor':[6,7,14],'Ability':[4,5,11,12,13,15,16,18,19,20,21,22,23],'Ring':[9],'Potion':[10]}
def tier_pool(category,tier):
    return [k for k in sorted(selected) if selected[k].findtext('Tier')==str(tier) and int(selected[k].findtext('SlotType') or 0) in SLOTS[category]]
REALM_SRC='https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/realm/setpieces/'
for landmark,(weapon,armor,rates,ring,ring_rates,ability_rates) in {0x675:(5,5,[.75,.375,.167],1,[.6,.15],[.3,.075,0.]),0x678:(5,4,[.75,.375,.167],1,[.6,.15],[.3,.075,0.]),0x67d:(6,6,[.5,.25,.125],2,[.2,.05],[.4,.1,.025]),0x668:(7,7,[.5,.25,.125],2,[.4,.1],[.8,.2,.05])}.items():
    chest=X.SubElement(manifest,'LandmarkChest',landmark=obj(landmark),container='Treasure Chest',**q('https://web.archive.org/web/20120217151558/http://forums.wildshadow.com:80/wiki/realm-mad-god/oasis-giant; src/loot.rs chest_table@79a4c92b','Monster-page chest tier rates (only the Oasis Giant page URL was recorded); one independent public roll per row with uniform tier choice. Authored reconstruction, not recovered server XML.'))
    rows=[(rates[i],'Weapon',weapon+i) for i in range(3)]+[(rates[i],'Armor',armor+i) for i in range(3)]+[(ability_rates[i],'Ability',2+i) for i in range(3)]+[(ring_rates[i],'Ring',ring+i) for i in range(2)]+[(1.,'Potion',1)]*2
    for p,category,tier in rows:
        if p>0:X.SubElement(chest,'Loot',prob=repr(p),tier=str(tier)).text=category
for landmark,origin,rows in [(0x925,'https://web.archive.org/web/20120610073636id_/http://forums.wildshadow.com:80/wiki/realm-mad-god/deathmage (tiers); '+REALM_SRC+'Graveyard.cs (rates, GetLoots(3,8))',[(.3,'Weapon',4),(.2,'Weapon',5),(.1,'Weapon',6),(.3,'Armor',3),(.2,'Armor',4),(.1,'Armor',5),(.3,'Ability',1),(.2,'Ability',2),(.2,'Ability',3),(.25,'Ring',1),(.15,'Ring',2),(.5,'Potion',1)]),
                             (0xdc2,REALM_SRC+'TempleA.cs; '+REALM_SRC+'TempleB.cs (rates, GetLoots(3,8))',[(.3,'Weapon',4),(.2,'Weapon',5),(.3,'Armor',4),(.2,'Armor',5),(.25,'Ability',1),(.15,'Ability',2),(.3,'Ring',2),(.2,'Ring',3)]+[(.5,'Potion',1)]*3)]:
    chest=X.SubElement(manifest,'LandmarkChest',landmark=obj(landmark),container='Treasure Chest',ceilingMin='3',ceilingMax='7',**q(origin+'; data/REALM-SRC-LICENSE; src/loot.rs chest_table@79a4c92b','realm-src MobDrops expands each tier row to per-item p/n rows in selected-kind order (selected order, not authenticated source order); successes stop at a uniform 3..7 ceiling drawn first. Authored reconstruction, not recovered server XML.'))
    for p,category,tier in rows:
        pool=tier_pool(category,tier)
        for k in pool:X.SubElement(chest,'Loot',prob=repr(p/len(pool))).text=obj(k)
for file in ['realm-catalog.json','ALLOY-LICENSE','FSOD-NOTICE','REALM-SRC-LICENSE','TERRAIN-GEOMETRY-LICENSES','realms/NOTICE']:
    X.SubElement(manifest,'Evidence',file=file)
save('game.xml',manifest)
# OryxTaunt names its list through the containing selected actor, not a listId attribute.
# Oryx names remaining Realm quest families from their one/many lists (src/world/realm_voice.rs family_name).
remaining=[(name,'native Oryx remaining-quest taunt') for name in ['Lich','Ent Ancient','Ghost King','Phoenix Lord','Oasis Giant','Red Demon','Cyclops God']]
strings=select_string_lists(X.parse(OUT/'actors.xml').getroot(),
                            X.parse(ARCHIVE/'stringlist.xml').getroot(), str(ARCHIVE/'stringlist.xml'), remaining)
save('strings.xml',strings)
