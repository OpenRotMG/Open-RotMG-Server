"""Select explicit and actor-name StringList dependencies without changing source messages."""
import copy
import xml.etree.ElementTree as X


def select_string_lists(actors, source, source_path, services=()):
    required = {name: [context] for name, context in services}
    for actor in actors.findall('Object'):
        for behavior in actor.iter('Behavior'):
            name = actor.attrib['id'] if (behavior.text or '').strip() == 'OryxTaunt' else behavior.attrib.get('listId')
            if name is not None:
                context = f"Object {actor.attrib['type']} {actor.attrib['id']} / {(behavior.text or '').strip()}"
                required.setdefault(name, []).append(context)
    for name, contexts in required.items():
        context = f"{source_path}: StringList {name!r} required by {'; '.join(contexts)}"
        matches = [n for n in source if n.tag == 'StringList' and n.attrib.get('id') == name]
        if len(matches) != 1:
            raise ValueError(f'{context}: expected exactly one source list, found {len(matches)}')
        categories = list(matches[0])
        if not categories or len({c.attrib.get('id') for c in categories}) != len(categories):
            raise ValueError(f'{context}: empty list or duplicate categories')
        for category in categories:
            if category.tag != 'Strings' or not category.attrib.get('id') or not list(category):
                raise ValueError(f'{context}: invalid/empty category {category.attrib}')
            for message in category:
                if message.tag != 'String' or list(message) or not (message.text or '').strip():
                    raise ValueError(f'{context}: invalid/empty message in {category.attrib}')
    selected = X.Element('StringLists', source.attrib)
    selected.extend(copy.deepcopy(n) for n in source if n.tag == 'StringList' and n.attrib.get('id') in required)
    return selected
