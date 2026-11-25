import copy
import hashlib
import unittest
import xml.etree.ElementTree as X
from unittest.mock import patch
import native


def signature(n):
    return n.tag, sorted(n.attrib.items()), (n.text or '').strip(), [signature(c) for c in n]


class NativeProducer(unittest.TestCase):
    def generate(self):
        actors=X.parse('data/game/123.5.1/actors.xml').getroot()
        actors[:]=[o for o in actors if o.attrib['disposition']!='native']
        before=signature(actors)
        roots=native.append_native(actors,X.parse('data/game/123.5.1/geometry.xml').getroot())
        original=copy.deepcopy(actors);original[:]=[o for o in original if o.attrib['disposition']!='native']
        self.assertEqual(before,signature(original))
        return actors,roots

    def test_actual_reviewed_native_closure_and_unchanged_scripted_bodies(self):
        actors,roots=self.generate()
        descriptors=[n for n in actors if n.attrib['disposition']=='native']
        self.assertEqual(len(descriptors),1460)
        material=''.join(f'{int(n.attrib["type"],16):04x}:{n.attrib["nativeService"]}\n' for n in descriptors)
        # Independent accepted per-kind inventory, not a runtime classification manifest.
        self.assertEqual(hashlib.sha256(material.encode()).hexdigest(),'44849f189e78251414fd1721b1e75e88c209b78a673775155aefa4134401f971')
        for n in descriptors:
            self.assertEqual(len(n),0)
            self.assertEqual(n.attrib['provenance'],'authored')
            self.assertTrue(n.attrib['origin'] and n.attrib['reason'])
            self.assertEqual(n.attrib['id'],native.selected[int(n.attrib['type'],16)].attrib['id'])
        pets={int(n.attrib['type'],16) for n in descriptors if n.attrib['nativeService']=='companion'}
        self.assertEqual(pets,{*range(0x1400,0x1406),0x1407,0x1408,0x140a,*range(0x1600,0x1642)})
        self.assertTrue({0xc2b,0xc2c,*range(0xc34,0xc39)}<=roots)
        manifest=X.parse('data/game/123.5.1/game.xml').getroot()
        native_ids={int(n.attrib['type'],16) for n in descriptors}
        self.assertTrue(all(native.by_name[n.attrib['object']] not in native_ids for n in manifest.iter('Population')))

    def test_generated_catalogue_exact_sources_and_scripted_locked_portal(self):
        actors,roots=self.generate()
        declarations={int(n.attrib['type'],16):n for n in actors}
        for kind,(name,_) in native.GENERATED_SCENERY.items():
            self.assertEqual(declarations[kind].attrib['id'],name)
            self.assertEqual(declarations[kind].attrib['nativeService'],'scenery')
            self.assertIn(kind,roots)
        self.assertEqual(declarations[0x700].attrib['nativeService'],'portal')
        locked=declarations[0x721]
        self.assertEqual(locked.attrib['disposition'],'active')
        self.assertEqual(locked.attrib['provenance'],'recovered')
        self.assertNotIn('nativeService',locked.attrib)
        self.assertEqual([(n.tag,n.attrib,n.text) for n in locked], [('Behavior',{'time':'120'},'Decay')])
        self.assertTrue({0x700,0x721}<=roots)
        with self.assertRaisesRegex(ValueError,'recovered server payload'):
            native.reviewed_empty_source(0x721,'Locked Wine Cellar Portal','portal')
        for candidates in [[],native.source[0x183]*2]:
            with patch.dict(native.source,{0x183:candidates}):
                with self.assertRaisesRegex(ValueError,'missing/ambiguous exact recovered source'):self.generate()
        with self.assertRaisesRegex(ValueError,'unsupported reviewed role'):
            native.reviewed_empty_source(0x183,'Broken Red Pillar','inventory')
        with self.assertRaisesRegex(ValueError,'incorrect selected identity/class'):
            native.reviewed_empty_source(0x183,'Broken Red Pillar','portal')
        collided=copy.deepcopy(actors);collided[:]=[n for n in collided if n.attrib['disposition']!='native']
        X.SubElement(collided,'Object',type='0x0183',id='Broken Red Pillar',disposition='active',origin='explicit collision regression')
        with self.assertRaisesRegex(ValueError,'native/scripted collision'):
            native.append_native(collided,X.parse('data/game/123.5.1/geometry.xml').getroot())
        recovered=copy.deepcopy(native.source[0x183][0][1]);X.SubElement(recovered,'Behavior',time='1').text='Decay'
        with patch.dict(native.source,{0x183:[(native.source[0x183][0][0],recovered)]}):
            with self.assertRaisesRegex(ValueError,'recovered server payload'):self.generate()

    def test_actual_source_metadata_refusals_are_contextual(self):
        cases=[(0x1c9,'Class','UnknownPanel','unsupported native scene'),
               (0x1513,'SlotType','22','missing exact selected companion'),
               (0xc2a,'SlotType','10','selected SpiritBomb requires exact'),
               (0x711,None,None,'selected identity mismatch')]
        for kind,field,value,message in cases:
            with self.subTest(kind=hex(kind),field=field):
                prop=copy.deepcopy(native.selected[kind])
                if field:prop.find(field).text=value
                else:prop.set('id','Not Trap')
                with patch.dict(native.selected,{kind:prop}):
                    with self.assertRaisesRegex(ValueError,message):self.generate()

    def test_repeated_generation_requires_fresh_scripted_input(self):
        actors=X.parse('data/game/123.5.1/actors.xml').getroot()
        with self.assertRaisesRegex(ValueError,'freshly normalized scripted'):
            native.append_native(actors,X.parse('data/game/123.5.1/geometry.xml').getroot())


if __name__=='__main__':unittest.main()
