import copy
import unittest
import xml.etree.ElementTree as X

from string_lists import select_string_lists


class StringListSelectionTests(unittest.TestCase):
    def setUp(self):
        self.actors = X.parse('data/game/123.5.1/actors.xml').getroot()
        self.path = 'data/reference/release-7.0/xml/stringlist.xml'
        self.source = X.parse(self.path).getroot()

    def test_actual_implicit_and_explicit_dependencies_preserve_source_payload(self):
        result = select_string_lists(self.actors, self.source, self.path)
        implicit = {o.attrib['id'] for o in self.actors if any((b.text or '').strip() == 'OryxTaunt' for b in o.iter('Behavior'))}
        explicit = {b.attrib['listId'] for b in self.actors.iter('Behavior') if 'listId' in b.attrib}
        self.assertEqual(len(implicit), 8)
        self.assertEqual(len(result), 17)
        self.assertEqual({n.attrib['id'] for n in result}, implicit | explicit)
        for actual in result:
            expected = next(n for n in self.source if n.attrib['id'] == actual.attrib['id'])
            self.assertEqual(X.tostring(actual), X.tostring(expected))
        beach = next(n for n in result if n.attrib['id'] == 'Beach Bum')
        self.assertEqual([n.attrib['id'] for n in beach], ['new'])
        self.assertNotIn('Beach Bun', X.tostring(beach, encoding='unicode'))

    def test_actual_required_source_missing_duplicate_or_empty_is_a_contextual_error(self):
        for defect in ('missing', 'duplicate', 'empty-list', 'empty-category', 'empty-message'):
            with self.subTest(defect=defect):
                source = copy.deepcopy(self.source)
                beach = next(n for n in source if n.attrib['id'] == 'Beach Bum')
                if defect == 'missing':
                    source.remove(beach)
                elif defect == 'duplicate':
                    source.append(copy.deepcopy(beach))
                elif defect == 'empty-list':
                    for category in list(beach):
                        beach.remove(category)
                elif defect == 'empty-category':
                    beach[0].remove(beach[0][0])
                    beach[0].remove(beach[0][0])
                else:
                    beach[0][0].text = ' '
                with self.assertRaisesRegex(ValueError, 'stringlist.xml: StringList.*Beach Bum.*Object 0x0e55 Beach Bum / OryxTaunt'):
                    select_string_lists(self.actors, source, self.path)

    def test_nested_states_and_embeds_keep_exact_containing_actor_context(self):
        actors = X.fromstring('<Objects><Object type="0xffff" id="Exact Synthetic Actor"><State id="Not An Actor"><Behavior>OryxTaunt</Behavior></State><DefineEmbed name="Not A List"><Behavior>OryxTaunt</Behavior></DefineEmbed><Embed name="Not A List"/></Object></Objects>')
        source = X.fromstring('<StringLists><StringList id="Exact Synthetic Actor"><Strings id="new"><String>{COUNT} exact messages</String></Strings></StringList></StringLists>')
        result = select_string_lists(actors, source, 'nested.xml')
        self.assertEqual([n.attrib['id'] for n in result], ['Exact Synthetic Actor'])
        source[0].set('id', 'Exact Synthetic Actors')
        with self.assertRaisesRegex(ValueError, 'expected exactly one source list, found 0'):
            select_string_lists(actors, source, 'nested.xml')


if __name__ == '__main__':
    unittest.main()
