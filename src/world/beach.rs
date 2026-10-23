use super::*;
use crate::{
    condition,
    dungeon::beach::{BOSS, BUM, TAUNTS},
};

impl World {
    pub(super) fn beach_notice(&mut self) {
        if self.players.is_empty() {
            return;
        }
        let unannounced = self
            .actors()
            .filter(|e| e.kind == BUM && e.stage.is_none())
            .map(|e| e.id)
            .collect::<Vec<_>>();
        for id in unannounced {
            self.entities.get_mut(&id).unwrap().stage = Some(0);
            self.broadcast_text(
                "Oryx the Mad God",
                "An elusive Beach Bum is hiding somewhere in the Realm.",
            );
        }
    }
    pub(super) fn populate_beach_bum(&mut self) -> Result<()> {
        let pos = self.place_named_setpiece(BUM, &[3, 4, 5, 6, 7])?;
        self.spawn_entity(BUM, pos, vec![])?;
        tracing::info!(
            x = pos.x,
            y = pos.y,
            "rare Beach Bum placed without quest marker"
        );
        Ok(())
    }

    // Silent recovery is described by the period guide; the later reference supplies the 5s reconstruction timer.
    // https://web.archive.org/web/20121201000000id_/http://forums.wildshadow.com/wiki/realm-mad-god/masked-party-god
    pub(super) fn beach_mechanics(&mut self, id: i32) -> Result<()> {
        if self.entities[&id].kind == BUM {
            return Ok(());
        }
        let e = self
            .entities
            .get_mut(&id)
            .context("Beachzone actor disappeared")?;
        ensure!(e.kind == BOSS, "unregistered Beachzone actor");
        let started = *e.activated.get_or_insert(self.now);
        if e.stage.is_none() {
            e.next_aux = self.now + 5000;
        }
        let beat = (self.now - started) / 500;
        let changed = e.stage != Some(beat);
        e.stage = Some(beat);
        if self.now >= e.next_aux
            && e.conditions(self.now) & condition::bit(condition::STUNNED) == 0
        {
            let before = e.hp;
            e.hp = self.content.objects[&BOSS].hp;
            e.next_aux = self.now + 5000;
            tracing::debug!(
                entity = id,
                before,
                after = e.hp,
                "Party God silently recovered"
            );
        }
        if changed {
            self.landmark_texture(id, 1 + (beat % 3) as i32);
            if let Some(text) = TAUNTS.get((beat % 12) as usize) {
                let packet = chat::text_packet("#Masked Party God", id, 5, text);
                for p in self.players.values().filter(|p| p.known.contains(&id)) {
                    send(p, packet.id, packet.body.clone());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn beach_dance_silent_recovery_statuses_and_defeat() {
        let mut w = super::super::tests::arena(1);
        let boss = w
            .spawn_entity(BOSS, Pos { x: 12.5, y: 12.5 }, vec![])
            .unwrap();
        w.beach_mechanics(boss).unwrap();
        assert_eq!(w.content.objects[&BOSS].defense, 300);
        assert_eq!(w.content.objects[&BUM].defense, 100);
        let home = w.entities[&boss].pos;
        for t in [0, 500, 1000, 1500, 2000, 2500, 3000, 3500, 4000, 4500] {
            w.now = t;
            w.beach_mechanics(boss).unwrap();
            assert_eq!(
                w.entities[&boss]
                    .extra_stats
                    .iter()
                    .find(|(s, _)| *s == 61)
                    .unwrap()
                    .1,
                1 + (t / 500 % 3) as i32
            );
            assert_eq!(w.entities[&boss].pos.distance(home), 0.);
        }
        w.damage_enemy(0, boss, 10000, true, &[], None).unwrap();
        w.now = 4999;
        w.beach_mechanics(boss).unwrap();
        assert_eq!(w.entities[&boss].hp, 20000);
        w.now = 5000;
        w.beach_mechanics(boss).unwrap();
        assert_eq!(w.entities[&boss].hp, 30000);
        assert!(!w.cleared);
        w.apply_condition(boss, condition::STUNNED, 1000).unwrap();
        w.damage_enemy(0, boss, 1000, true, &[], None).unwrap();
        w.now = 10000;
        w.apply_condition(boss, condition::STUNNED, 1000).unwrap();
        w.beach_mechanics(boss).unwrap();
        assert_eq!(w.entities[&boss].hp, 29000);
        w.now = 11000;
        w.beach_mechanics(boss).unwrap();
        assert_eq!(w.entities[&boss].hp, 30000);
        w.name = "Beachzone".into();
        for _ in 0..3 {
            w.damage_enemy(0, boss, 9000, true, &[], None).unwrap();
        }
        w.damage_enemy(0, boss, 3000, true, &[], None).unwrap();
        assert!(!w.entities.contains_key(&boss));
        assert!(w.cleared);
        assert_eq!(w.entities[&w.clear_exit.unwrap()].kind, 0x704);
    }
    #[test]
    fn beach_rare_realm_spawn_and_bum_defeat_earn_entrance() {
        let c = Arc::new(Content::load(Path::new("data")).unwrap());
        let seed = (0..10000)
            .find(|seed| rand::rngs::StdRng::seed_from_u64(seed ^ 1).gen_ratio(1, 200))
            .unwrap();
        eprintln!("rare Beach Bum prepared Realm seed={seed}");
        let mut d = Directory::fixture(Path::new("data"), c, 16, seed, Some(seed)).unwrap();
        let w = d.worlds.get_mut(&-3).unwrap();
        let bum = w
            .actors()
            .filter(|e| e.kind == BUM)
            .map(|e| e.id)
            .collect::<Vec<_>>();
        assert_eq!(bum.len(), 1);
        let id = bum[0];
        let home = w.entities[&id].home;
        assert!(w.content.objects[&BUM].quest_level.is_none());
        let mut ch = super::super::tests::ch(w);
        ch.id = 22;
        let player = w
            .enter_at(
                ch,
                None,
                Some(Pos {
                    x: home.x + 1.,
                    y: home.y,
                }),
                AccountPresentation {
                    guild: None,
                    credits: 100,
                    fame: 0,
                    best_fame: BTreeMap::new(),
                },
            )
            .unwrap();
        w.beach_notice();
        assert_eq!(w.entities[&id].stage, Some(0));
        assert_eq!(
            w.damage_enemy(player, id, 1000, true, &[], None).unwrap(),
            1000
        );
        let keys = w
            .entities
            .values()
            .filter(|e| e.items.contains(&0xc2e))
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].extra_stats.contains(&(54, 1)));
        assert_eq!(crate::dungeon::portal_drop(BUM), Some((0x742, 0.5)));
        assert!(d.instance("beachzone", None).is_ok());
    }
    #[test]
    fn beach_island_is_safe_and_definitions_and_rewards_are_selected() {
        let w = super::super::tests::arena(1);
        let map = crate::dungeon::beach::generate(&w.content).unwrap();
        assert_eq!((map.width, map.height), (136, 127));
        assert_eq!(
            map.cells.iter().filter(|c| c.object == Some(0x703)).count(),
            1
        );
        assert!(
            map.cells
                .iter()
                .filter_map(|c| c.object)
                .all(|k| { w.content.objects[&k].class != "Equipment" })
        );
        assert_eq!(
            map.cells.iter().filter(|c| c.object == Some(BOSS)).count(),
            1
        );
        assert_eq!(
            map.cells.iter().filter(|c| c.object == Some(0xdc6)).count(),
            4
        );
        assert!(
            map.cells
                .iter()
                .all(|c| !w.content.hazards.contains_key(&c.ground))
        );
        assert_eq!(
            map.cells
                .iter()
                .filter_map(|c| c.object)
                .filter(|k| w.content.objects[k].enemy)
                .collect::<Vec<_>>(),
            vec![BOSS]
        );
        let mut rng = rand::rngs::StdRng::seed_from_u64(23);
        let mut found = BTreeSet::new();
        for _ in 0..1000 {
            found.extend(crate::loot::rewards(&w.content, BOSS, &mut rng));
        }
        assert_eq!(found, crate::dungeon::beach::DRINKS.into_iter().collect());
        assert_eq!(crate::loot::rewards(&w.content, BUM, &mut rng), vec![0xc2e]);
        assert!(crate::loot::private(BOSS));
        assert!(!crate::loot::private(BUM));
    }
}
