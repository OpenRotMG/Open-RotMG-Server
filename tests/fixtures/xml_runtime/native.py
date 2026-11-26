"""Explicit selected native-service consumers, not absence-of-AI classification."""
import collections
import struct
import zlib
import xml.etree.ElementTree as X
import json
from selection import DATA, selected, by_name, qualify, OBJECT_REFERENCES, source, SERVER

SCENE = {
    'scenery': ({'GameObject', 'Wall', 'ConnectedWall', 'CaveWall', 'SpiderWeb', 'Stalagmite', 'Sign'}, 'src/world.rs:World::with_ids; selected native object renderer/collision/NAME'),
    'portal': ({'Portal', 'GuildHallPortal'}, 'src/world.rs:Directory portal transfer; authenticated instance/guild binding'),
    'merchant': ({'Merchant', 'GuildMerchant'}, 'src/world/shop.rs:configure/buy; src/world/guilds.rs:guild upgrade'),
    'container': ({'Container', 'ClosedVaultChest'}, 'src/world.rs:loot_bag/load_vault; src/world/shop.rs:buy_vault; private owner/storage/sale lifecycle'),
    'interaction': ({'MoneyChanger', 'CharacterChanger', 'NameChanger', 'GuildRegister', 'GuildChronicle', 'GuildBoard'}, 'selected native client panels; src/crypto/gold.rs; src/server/name_claims.rs; src/server/guilds.rs; src/world/guilds.rs; existing handlers only, no new payments/features/prices'),
}
DEPLOYMENTS = {'Trap': (0x711, 'trap'), 'Decoy': (0x715, 'decoy'), 'SpiritBomb': (0x1409, 'spirit-bomb')}
SLOTS = {'Weapon': {1,2,3,8,17}, 'Ability': {4,5,11,12,13,15,16,18,19,20,21,22,23},
         'Armor': {6,7,14}, 'Ring': {9}, 'Potion': {10}}
SLOTS['ClassSpecial'] = SLOTS['Ability']
# Frozen generator operands are catalogue dependencies, not population or all-seed spawn promises.
GENERATED_SCENERY = {
    0x182: ('Broken Blue Pillar', 'src/world/realm_setpieces.rs:745,769'),
    0x184: ('Cross', 'src/world/realm_setpieces.rs:761'),
    0x181b: ('Death Grass', 'src/world/realm_setpieces.rs:727'),
    0x181c: ('Death Flower', 'src/world/realm_setpieces.rs:728'),
    0x183: ('Broken Red Pillar', 'src/dungeon/abyss.rs'),
    0xdc5: ('Jungle Spear', 'src/dungeon/jungle.rs'),
    0x170f: ('OceanRock', 'src/dungeon/trench.rs'),
    **{0x18ae+i: ('MadLab Warning '+d, 'src/dungeon/lab.rs; data/lab-templates.json')
       for i,d in enumerate(('R','L','U','D','UR','UL','DR','DL'))},
    **{0x18bc+i: ('MadLab Warning Inner '+d, 'src/dungeon/lab.rs; data/lab-templates.json')
       for i,d in enumerate(('UR','UL','DR','DL'))},
}
SCRIPTED_FACTORY_ROOT = 0x721


def reviewed_empty_source(kind, name, service):
    if service not in {'scenery','portal'}:
        raise ValueError(f'reviewed catalogue {kind:#x} {name}: unsupported reviewed role {service}')
    prop=selected[kind]
    if prop.attrib['id']!=name or prop.findtext('Class')!=('GameObject' if service=='scenery' else 'Portal'):
        raise ValueError(f'reviewed catalogue {kind:#x} {name}: incorrect selected identity/class for {service}')
    if service=='scenery' and (prop.find('Static') is None or prop.findtext('MaxHitPoints','0')!='0'):
        raise ValueError(f'reviewed catalogue {kind:#x} {name}: changed static scenery signature')
    candidates=source[kind]
    if len(candidates)!=1 or candidates[0][1].attrib['id']!=name:
        raise ValueError(f'reviewed catalogue {kind:#x} {name}: missing/ambiguous exact recovered source')
    file,original=candidates[0]
    if any(n.tag in SERVER|{'Transition'} for n in original.iter()):
        raise ValueError(f'{file}: reviewed catalogue {kind:#x} {name}: recovered server payload cannot be discarded into Native')
    return str(file)


def wmap(path):
    raw = zlib.decompress(path.read_bytes()); pos = 2
    def string():
        nonlocal pos
        count = 0
        for shift in range(0,35,7):
            b=raw[pos];pos+=1;count|=(b&127)<<shift
            if b<128:break
        value=raw[pos:pos+count].decode();pos+=count;return value
    cells=[]
    for _ in range(struct.unpack_from('<h',raw)[0]):
        pos+=2;name=string();config=string();pos+=2
        cells.append((name,config))
    width,height=struct.unpack_from('<ii',raw,pos);pos+=8
    assert len(raw)-pos==2*width*height, path
    return width,height,[cells[i] for i in sorted({v[0] for v in struct.iter_unpack('<h',raw[pos:])})]


def append_native(actors, geometry):
    scripted={int(o.attrib['type'],16) for o in actors}
    if any(o.attrib['disposition']=='native' for o in actors):
        raise ValueError('append_native requires freshly normalized scripted declarations')
    declared={}; pending=set(scripted); roots=set()
    def add(kind, service, origin):
        if kind in scripted:
            if service not in ('scene','reference'):raise ValueError(f'{origin}: native/scripted collision {kind:#x}')
            return
        prop=selected[kind]; cls=prop.findtext('Class')
        if service in ('scene','reference'):
            matches=[s for s,(classes,_) in SCENE.items() if cls in classes]
            if len(matches)!=1:raise ValueError(f'{origin}: unsupported native scene/reference {kind:#x} {prop.attrib["id"]} class={cls}')
            service=matches[0]
        valid=(cls in SCENE[service][0] if service in SCENE else
               cls in {'Equipment','Dye'} if service=='inventory' else
               cls=='Player' if service=='player' else
               cls=='Projectile' if service=='projectile' else
               cls=='Character' if service in {'companion','trap','decoy','spirit-bomb'} else False)
        if not valid or prop.find('Enemy') is not None:
            raise ValueError(f'{origin}: incompatible explicit native {service} {kind:#x} {prop.attrib["id"]} class={cls}')
        old=declared.get(kind)
        if old and old[0]!=service:raise ValueError(f'{origin}: native service collision {kind:#x} {old[0]} vs {service}')
        if not old:declared[kind]=(service,origin);pending.add(kind)
    def scene(name, origin):add(by_name[name], 'scene', origin)
    for n in geometry.iter():
        if 'object' in n.attrib:scene(n.attrib['object'], 'game/123.5.1/geometry.xml:'+n.tag)
        if n.tag=='Map':
            for name,config in wmap(DATA/n.attrib['file'])[2]:
                if name:scene(name, 'data/'+n.attrib['file']+' native map cell')
                for setting in config.split(';'):
                    if setting.startswith('mtype:'):
                        item=int(setting.split(':',1)[1],0);add(item,'inventory','data/'+n.attrib['file']+' native merchant '+config);roots.add(item)
        if n.tag=='Reconstruction':
            recipe=json.loads((DATA/'realms'/n.attrib['recipe']).read_text())
            dictionary=json.loads((DATA/'realms'/recipe['inputs']['dictionary']['path']).read_text())
            raw=zlib.decompress((DATA/'realms'/recipe['inputs']['indices']['path']).read_bytes())
            for index in sorted({v[0] for v in struct.iter_unpack('<H',raw)}):
                cell=dictionary[index]
                if cell['object'] is not None:add(cell['object'],'scene','data/realms/'+n.attrib['recipe']+' reviewed cell')
    for kind,prop in selected.items():
        if prop.findtext('Class')=='Player':add(kind,'player','src/content.rs:Content.classes; native selected character creation/unlock');roots.add(kind)
        if prop.findtext('Class')=='Equipment' and prop.findtext('SlotType')=='10' and prop.find('Consumable') is not None and len(prop.findall('Activate'))==1 and prop.findtext('Activate')=='Pet':
            add(kind,'inventory','src/world/companions.rs:begin_companion/duration; actual selected temporary Pet consumable');roots.add(kind)
        if prop.findtext('Class') in ('Equipment','Dye'):
            add(kind,'inventory','data/objects.xml selected inventory item; earned/vault/trade inventories may hold any selected item');roots.add(kind)
    for kind in [0x500,0x503,0x507,0x508,0x509,0x504,0x505]:
        add(kind,'container','src/world.rs:loot_bag/load_vault; src/world/shop.rs:buy_vault');roots.add(kind)
    add(0x501,'container','src/world/realm_setpieces.rs:spawn_landmark_chests; actual selected Treasure Chest native container factory');roots.add(0x501)
    for kind in [0x723,0x724,0x725,*range(0x726,0x72c),0x734,0x735]:
        add(kind,'scenery','src/world.rs:finish_death committed native grave lifetime');roots.add(kind)
    add(0x712,'portal','src/world/realm_portal.rs:native named Realm portal');roots.add(0x712)
    for name,(kind,service) in DEPLOYMENTS.items():
        if selected[kind].attrib['id']!={'Trap':'Trap','Decoy':'Decoy','SpiritBomb':'Spirit Prism Bomb'}[name]:
            raise ValueError(f'explicit reviewed deployment binding {name}: selected identity mismatch {kind:#x}')
        add(kind,service,'src/world/abilities.rs:'+name+'; explicit reviewed authored target binding, not recovered literal activation target');roots.add(kind)
    groups=collections.defaultdict(set)
    for actor in actors:
        for g in actor.findall('Group'):groups[(g.text or '').strip()].add(int(actor.attrib['type'],16))
    for actor in actors:
        origin=actor.attrib['origin']
        for n in actor.iter():
            for field in sorted(OBJECT_REFERENCES & n.attrib.keys()):
                for name in n.attrib[field].split(','):
                    name=name.strip()
                    if '{' in name:continue
                    targets={by_name[name]} if name in by_name else groups[name]
                    if not targets:raise ValueError(f'{origin}: unresolved {field}={name}')
                    for target in sorted(targets):add(target,'reference',origin+' '+field)
            if n.tag in ('Loot','SoulboundLoot'):
                if 'tier' in n.attrib:
                    slots=SLOTS[(n.text or '').strip()];tier=n.attrib['tier']
                    targets=[k for k,p in selected.items() if p.findtext('Tier')==tier and int(p.findtext('SlotType','0')) in slots]
                else:targets=[by_name[(n.text or '').strip()]]
                for target in targets:
                    add(target,'portal' if selected[target].findtext('Class')=='Portal' else 'inventory',origin+' '+n.tag)
    done=set()
    while pending:
        kind=min(pending);pending.remove(kind)
        if kind in done:continue
        done.add(kind);prop=selected[kind];origin='data/objects.xml:'+prop.attrib['id']
        for n in (prop.findtext('Equipment') or '').split(','):
            if n.strip() and int(n.strip(),0)>=0:add(int(n.strip(),0),'inventory',origin+' Equipment')
        for n in prop.findall('Projectile'):
            if n.find('ObjectId') is not None:add(by_name[n.findtext('ObjectId').strip()],'projectile',origin+' Projectile/ObjectId')
        if prop.find('SuccessorId') is not None:add(by_name[prop.findtext('SuccessorId').strip()],'inventory',origin+' SuccessorId')
        for index,a in enumerate(prop.findall('Activate')):
            activation=(a.text or '').strip()
            if activation=='Create':scene(a.attrib['id'],origin+' Activate/Create')
            elif activation in ('Trap','Decoy'):
                target,service=DEPLOYMENTS[activation];add(target,service,origin+f' Activate[{index}]/{activation}')
            elif activation in ('Pet','PermaPet'):
                target=by_name[a.attrib['objectId']]
                if target==DEPLOYMENTS['SpiritBomb'][0]:
                    if prop.findtext('SlotType')!='22' or [a.text for a in prop.findall('Activate')]!=['Pet','Teleport']:
                        raise ValueError(origin+': selected SpiritBomb requires exact Pet[0]/Teleport[1] composite activation')
                    add(target,'spirit-bomb',origin+' selected Pet[0]/objectId; SpiritBomb then Teleport[1]')
                else:
                    if prop.findtext('Class')!='Equipment' or prop.findtext('SlotType')!='10' or len(prop.findall('Activate'))!=1 or not (prop.find('Consumable') is not None or prop.find('InvUse') is not None):
                        raise ValueError(origin+': missing exact selected companion activation entitlement')
                    add(target,'companion',origin+f' Activate[{index}]/{activation}/objectId')
    for kind,(name,consumer) in sorted(GENERATED_SCENERY.items()):
        original=reviewed_empty_source(kind,name,'scenery')
        add(kind,'scenery',consumer+'; reviewed finite generated operand; '+original+' exact source has no server payload')
        roots.add(kind)
    original=reviewed_empty_source(0x700,'Dungeon Portal','portal')
    add(0x700,'portal','src/world/endgame.rs:existing Incantation/locked Cellar factory; '+original+' exact empty Portal/IntergamePortal, not a new service')
    roots.add(0x700)
    if SCRIPTED_FACTORY_ROOT not in scripted:
        raise ValueError('Locked Wine Cellar Portal0x721 must retain recovered Scripted Decay via selection.actors')
    roots.add(SCRIPTED_FACTORY_ROOT)
    for kind,(service,origin) in sorted(declared.items()):
        prop=selected[kind]
        handler=SCENE[service][1] if service in SCENE else {
            'inventory':'native inventory/property service ONLY, not a World Entity',
            'projectile':'native projectile/property service ONLY, not a World Entity',
            'player':'native player/property service ONLY, not an ordinary World Entity',
            'companion':'src/world/companions.rs: exact selected item entitlement; runtime owner/reservation still required',
            'trap':'src/world/abilities.rs: scoped Trap deployment, never ordinary map/actor birth',
            'decoy':'src/world/abilities.rs: scoped Decoy deployment, never ordinary map/actor birth',
            'spirit-bomb':'src/world/abilities.rs: scoped SpiritBomb deployment; actual selected composite Pet/Teleport order retained',
        }[service]
        X.SubElement(actors,'Object',type=f'0x{kind:04x}',id=prop.attrib['id'],disposition='native',nativeService=service,
                     **qualify('authored',origin+'; data/objects.xml:'+prop.attrib['id'],
                               'Explicit reviewed selected native-service binding: '+handler+'. Not recovered server AI or historical fidelity; no property overrides.'))
    return roots
