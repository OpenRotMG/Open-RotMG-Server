use super::*;
use crate::encounter::tutorial::{CHEF, GOD, MINION, MINION_LIMIT, REPRODUCE_MS};

impl World {
    pub(super) fn tutorial_mechanics(&mut self, id: i32, target: Option<Pos>) -> Result<()> {
        let e = &self.entities[&id];
        if e.kind == GOD {
            if e.next_aux == 0 {
                self.entities.get_mut(&id).unwrap().next_aux = self.now + REPRODUCE_MS;
            } else if self.now >= e.next_aux {
                let count = self.summoned_children(id, MINION).count();
                self.spawn_children(id, MINION, (count + 1).min(MINION_LIMIT), 5.)?;
                self.entities.get_mut(&id).unwrap().next_aux = self.now + REPRODUCE_MS;
            }
        } else if e.kind == CHEF {
            let old = e.stage;
            let stage = match old {
                None | Some(0) => u64::from(target.is_some_and(|p| p.distance(e.pos) < 10.)),
                Some(phase) if matches!(phase % 4, 1 | 2) && e.hp < 90 => 3,
                Some(phase)
                    if phase % 4 == 1
                        && self.now - e.activated.context("Kitchen phase clock missing")?
                            >= 4500 =>
                {
                    phase + 1
                }
                Some(phase)
                    if phase % 4 == 2
                        && self.now - e.activated.context("Kitchen phase clock missing")?
                            >= 5200 =>
                {
                    phase + 3
                }
                Some(phase) if matches!(phase % 4, 1..=3) => phase,
                Some(phase) => bail!("invalid Kitchen boss phase {phase}"),
            };
            if old != Some(stage) {
                let e = self.entities.get_mut(&id).unwrap();
                e.stage = Some(stage);
                e.activated = Some(self.now);
                let line = match stage % 4 {
                    1 => Some("Ah, fresh meat for the minions!"),
                    3 => Some("The meat ain't supposed to bite back! Waaaaa!!"),
                    _ => None,
                };
                if let Some(line) = line {
                    self.broadcast_text("Bonegrind the Butcher", line);
                }
                if stage == 3 {
                    self.npc_flash(id, 0xff000000, 10., 100.);
                }
                tracing::info!(entity = id, stage, "Kitchen encounter phase entered");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encounter::{self, tutorial};
    fn world(template: &str) -> World {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let map = Map::load(
            &std::path::Path::new("data").join(format!("{template}.wmap")),
            &content,
        )
        .unwrap();
        let mut world = World::new(content, Arc::new(map)).unwrap();
        world.encounter = true;
        world
    }
    #[test]
    fn tutorial_roster_and_projectiles_validate_without_inventing_chicken_attacks() {
        let world = world("tutorial");
        for &kind in tutorial::ROSTER {
            encounter::validate(&world.content, kind).unwrap();
            crate::loot::validate(&world.content, kind).unwrap();
        }
        for kind in 0x6af..=0x6b2 {
            assert!(world.content.get(kind).unwrap().projectiles.is_empty());
            assert!(tutorial::schedule(kind, 0, 60050).attacks.is_empty());
        }
        for phase in 0..=3 {
            for attack in tutorial::schedule(CHEF, phase, 0).attacks {
                assert!(matches!(
                    attack.action,
                    encounter::attacks::Action::Shoot { projectile: 0, .. }
                ));
            }
        }
    }
    #[test]
    fn tutorial_reproduction_is_delayed_bounded_owned_and_retires_without_rewards() {
        let mut world = world("tutorial");
        let boss = world.entities.values().find(|e| e.kind == GOD).unwrap().id;
        world.tutorial_mechanics(boss, None).unwrap();
        assert_eq!(world.summoned_children(boss, MINION).count(), 0);
        world.now = REPRODUCE_MS - 50;
        world.tutorial_mechanics(boss, None).unwrap();
        assert_eq!(world.summoned_children(boss, MINION).count(), 0);
        for n in 1..=15 {
            world.now = n * REPRODUCE_MS;
            world.tutorial_mechanics(boss, None).unwrap();
            assert_eq!(
                world.summoned_children(boss, MINION).count(),
                (n as usize).min(MINION_LIMIT)
            );
        }
        let child = world.summoned_children(boss, MINION).next().unwrap().id;
        assert!(world.walkable(world.entities[&child].pos));
        world.remove_entity(child);
        world.now += REPRODUCE_MS;
        world.tutorial_mechanics(boss, None).unwrap();
        assert_eq!(world.summoned_children(boss, MINION).count(), MINION_LIMIT);
        world.remove_entity(boss);
        world.mechanic_expiry();
        assert!(!world.entities.values().any(|e| e.kind == MINION));
    }
    #[tokio::test]
    async fn kitchen_native_simulation_fires_and_rewards_each_death_once() {
        let mut world = world("kitchen");
        let store =
            crate::store::Store::open(std::path::Path::new(":memory:"), &world.content).unwrap();
        let account = store.list("kitchen-test".into()).await.unwrap();
        let ch = store
            .create(
                account.id,
                world.content.get(crate::content::WIZARD).unwrap(),
            )
            .await
            .unwrap();
        let player = world.enter(ch, None).unwrap();
        let boss = world.entities.values().find(|e| e.kind == CHEF).unwrap().id;
        let pos = world.entities[&boss].pos;
        world.players.get_mut(&player).unwrap().pos = Pos {
            x: pos.x,
            y: pos.y + 4.,
        };
        world.now = 50;
        world.npc_step();
        assert!(world.entities[&boss].bullet > 0);
        let character = world.players[&player].character.id;
        world
            .entities
            .get_mut(&boss)
            .unwrap()
            .damage
            .insert(character, 300);
        world.defeat_entity(boss).unwrap();
        let rewards: Vec<_> = world
            .entities
            .values()
            .filter(|e| e.kind == 0x500)
            .flat_map(|e| e.items.iter().copied())
            .filter(|i| *i >= 0)
            .collect();
        assert_eq!(rewards, vec![0xab0, 0xadd]);
        world.now += 50;
        world.npc_step();
        assert!(!world.entities.contains_key(&boss));
        assert_eq!(world.bag_expiry.len(), 1);
        assert!(!world.cleared);
    }
    #[test]
    fn kitchen_boss_acquires_cycles_and_permanently_flees_below_thirty_percent() {
        let mut world = world("kitchen");
        let boss = world.entities.values().find(|e| e.kind == CHEF).unwrap().id;
        world.tutorial_mechanics(boss, None).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(0));
        let pos = world.entities[&boss].pos;
        world.tutorial_mechanics(boss, Some(pos)).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(1));
        assert_eq!(
            world
                .npc_attack_schedule(boss, 0)
                .unwrap()
                .unwrap()
                .snapshot(0.)
                .len(),
            1
        );
        world.now += 4500;
        world.tutorial_mechanics(boss, Some(pos)).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(2));
        assert_eq!(
            world
                .npc_attack_schedule(boss, 0)
                .unwrap()
                .unwrap()
                .snapshot(0.)
                .len(),
            4
        );
        world.now += 5200;
        world.tutorial_mechanics(boss, Some(pos)).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(5));
        world.entities.get_mut(&boss).unwrap().hp = 90;
        world.tutorial_mechanics(boss, Some(pos)).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(5));
        world.entities.get_mut(&boss).unwrap().hp = 89;
        world.tutorial_mechanics(boss, Some(pos)).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(3));
        assert!(
            world
                .npc_attack_schedule(boss, 0)
                .unwrap()
                .unwrap()
                .attacks
                .is_empty()
        );
        world.entities.get_mut(&boss).unwrap().hp = 300;
        world.now += 10000;
        world.tutorial_mechanics(boss, Some(pos)).unwrap();
        assert_eq!(world.entities[&boss].stage, Some(3));
    }
}
