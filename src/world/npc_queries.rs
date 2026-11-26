use super::*;

impl World {
    // Trees carry an XML death hook but no Enemy hit box: the hook can never run, so they stay shared scenery.
    pub(super) fn actor_kind(content: &Content, kind: u16) -> bool {
        let object = &content.objects[&kind];
        let scripted = (content.game.as_ref())
            .expect("validated runtime GameData")
            .definitions
            .get(&kind)
            .is_some_and(|d| {
                d.disposition == crate::content::game::Disposition::Active
                    && (object.destructible || !xml::admission::terminal_only(d))
            });
        scripted || (!object.static_object && !object.passive_destructible())
    }
}
