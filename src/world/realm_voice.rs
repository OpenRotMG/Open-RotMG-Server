// GPL-3.0 reference: realm-src@1855c017 wServer/realm/Oryx.cs; NR-Core and Skilly.
use super::*;
use crate::content::game::{TerminalAnnouncements, TextPart};
use rand::seq::SliceRandom;

/// Realm quest lineages share their root's StringList name (strings.xml one/many).
pub(super) fn family_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        0x668 => "Red Demon",
        0x67d => "Cyclops God",
        0x91f | 0x920 => "Ent Ancient",
        0x91b | 0x91c => "Lich",
        0x928 | 0x92d => "Ghost King",
        0x675 | 0x6c1 | 0x6c2 => "Phoenix Lord",
        0x678 => "Oasis Giant",
        0xd56 => "Skull Shrine",
        0xd59 => "Cube God",
        0xd5f => "Pentaract",
        0xd54 => "Grand Sphinx",
        0xd61 => "Hermit God",
        0xd50 => "Lord of the Lost Lands",
        0xe37 => "Ghost Ship",
        _ => return None,
    })
}
impl World {
    pub(super) fn realm_killed_notice(&self, kind: u16, player: &str) {
        let announced = self.xml_game().definitions.get(&kind).is_some_and(|d| {
            d.states.iter().flat_map(|s| &s.behaviors).any(|b| {
                b.announcement
                    .as_ref()
                    .is_some_and(|a| matches!(a.terminal, TerminalAnnouncements::Death { .. }))
            })
        });
        // Event bosses' OryxTaunt already published a strings.xml killed line naming the killer.
        if announced {
            return;
        }
        // Base 79a4c92b src/world/realm_voice.rs:43: recovered strings.xml has no killed lines for Heroes of Oryx.
        self.oryx_notice(&format!(
            "{player}, you will pay for defeating my {}!",
            family_name(kind).expect("tracked Realm hero family")
        ));
    }
    pub(super) fn last_realm_enemy_notice(&self, remaining: &BTreeSet<i32>) {
        if remaining.len() != 1 {
            return;
        }
        let root = *remaining.first().unwrap();
        if let Some(name) = self
            .actors()
            .filter(|e| e.hp > 0 && (e.id == root || e.quest == Some(root)))
            .find_map(|e| family_name(e.kind))
        {
            self.oryx_notice(&format!("A {name} still guards me!"));
        }
    }
    /// A family is taunted only through its recovered one/many category; Ghost Ship has no `many` lines.
    pub(super) fn remaining_taunt(
        &self,
        remaining: &BTreeSet<i32>,
    ) -> Result<Option<(u16, usize, String)>> {
        let mut families: BTreeMap<&str, (u16, BTreeSet<i32>)> = BTreeMap::new();
        for e in self.actors().filter(|e| e.hp > 0) {
            let tag = e.quest.unwrap_or(e.id);
            if let Some(name) = family_name(e.kind)
                && remaining.contains(&tag)
            {
                families
                    .entry(name)
                    .or_insert((e.kind, BTreeSet::new()))
                    .1
                    .insert(tag);
            }
        }
        let game = self.xml_game();
        let mut choices = Vec::new();
        for (name, (kind, tags)) in families {
            let list = game
                .string_lists
                .iter()
                .find(|l| l.name == name)
                .with_context(|| format!("Oryx remaining-quest StringList {name:?} absent"))?;
            let category = if tags.len() == 1 { "one" } else { "many" };
            if let Some(texts) = list.categories.get(category) {
                choices.push((kind, tags.len(), texts));
            }
        }
        let rng = &mut rand::thread_rng();
        let Some((kind, count, texts)) = choices.choose(rng) else {
            return Ok(None);
        };
        let mut text = String::new();
        for part in texts
            .choose(rng)
            .context("empty Oryx remaining-quest category")?
        {
            match part {
                TextPart::Literal(s) => text.push_str(s),
                TextPart::Count => text.push_str(&count.to_string()),
                part => bail!("Oryx remaining-quest taunt has unbound token {part:?}"),
            }
        }
        Ok(Some((*kind, *count, text)))
    }
}
