use super::*;
use crate::achievements::Achievement;

#[derive(Default)]
pub(super) struct DamageWindow(VecDeque<(Instant, i32, i32)>);
impl DamageWindow {
    pub(super) fn record(&mut self, now: Instant, damage: i32, drained: i32) -> Result<bool> {
        ensure!(
            damage >= 0 && drained >= 0 && drained <= damage,
            "invalid achievement damage"
        );
        self.0
            .retain(|(at, _, _)| now.saturating_duration_since(*at).as_millis() <= 1000);
        if damage > 0 {
            if let Some((at, total, loss)) = self.0.back_mut().filter(|(at, _, _)| *at == now) {
                let _ = at;
                *total = total
                    .checked_add(damage)
                    .context("achievement damage overflow")?;
                *loss = loss
                    .checked_add(drained)
                    .context("achievement HP loss overflow")?;
            } else {
                ensure!(self.0.len() < 4096, "achievement damage history exhausted");
                self.0.push_back((now, damage, drained));
            }
        }
        Ok(self
            .0
            .iter()
            .filter(|(at, _, _)| {
                now.saturating_duration_since(*at) <= std::time::Duration::from_millis(250)
            })
            .map(|(_, damage, _)| i64::from(*damage))
            .sum::<i64>()
            >= 500)
    }
    fn close(&self, now: Instant, ch: &Character) -> bool {
        ch.level == 20
            && ch.hp > 0
            && ch.hp < 30
            && self
                .0
                .iter()
                .filter(|(at, _, _)| {
                    now.saturating_duration_since(*at) <= std::time::Duration::from_millis(1000)
                })
                .map(|(_, _, loss)| i64::from(*loss))
                .sum::<i64>()
                > 200
    }
}
impl Player {
    pub(super) fn achievement_damage(
        &mut self,
        now: Instant,
        damage: i32,
        hp_before: i32,
    ) -> Result<()> {
        if self.controller != Controller::Autonomous
            && self.character.level == 20
            && hp_before > 0
            && damage >= hp_before
            && self.dungeon_entered.is_some_and(|entered| {
                now.checked_duration_since(entered)
                    .is_some_and(|elapsed| elapsed <= std::time::Duration::from_millis(1700))
            })
        {
            self.character.achievements.insert(Achievement::Ah7uck);
        }
        if self.controller != Controller::Autonomous
            && self
                .achievement_damage
                .record(now, damage, damage.min(hp_before).max(0))?
        {
            self.character.achievements.insert(Achievement::HitByTruck);
        }
        Ok(())
    }
}
impl Directory {
    pub(crate) fn update_achievement_actors(&self) -> Vec<(i32, i32, i32)> {
        self.worlds
            .values()
            .filter(|w| {
                matches!(
                    w.name.as_str(),
                    "Oryx's Castle" | "Oryx's Chamber" | "Wine Cellar"
                )
            })
            .flat_map(|w| {
                w.players
                    .iter()
                    .filter(|(_, p)| {
                        p.active() && p.character.hp > 0 && p.controller != Controller::Autonomous
                    })
                    .map(|(id, p)| (p.character.account, *id, p.character.id))
            })
            .collect()
    }
    pub(crate) fn update_achievement_characters(
        &self,
        actors: &[(i32, i32, i32)],
    ) -> Result<Vec<Character>> {
        actors.iter().map(|(account, avatar, character)| {
            let world = &self.worlds[&self.location(*avatar)?];
            let p = &world.players[avatar];
            ensure!(p.character.account == *account && p.character.id == *character && p.active() && p.character.hp > 0 && p.controller != Controller::Autonomous
                && matches!(world.name.as_str(), "Oryx's Castle" | "Oryx's Chamber" | "Wine Cellar"), "intentional update achievement actor changed before reservation: account={account} avatar={avatar} character={character}");
            Ok(p.character.clone())
        }).collect()
    }
    pub(crate) fn bind_achievement_escape(&mut self, id: i32, key: &[u8]) -> Result<()> {
        let source = self.location(id)?;
        let p = &self.worlds[&source].players[&id];
        let grant = self
            .grants
            .get_mut(key)
            .context("achievement escape grant missing")?;
        ensure!(
            grant.origin == Some(source)
                && grant.account == p.character.account
                && grant.character == p.character.id
                && grant.destination == -2,
            "achievement escape source changed"
        );
        grant.achievement_escape = p.controller != Controller::Autonomous
            && p.achievement_damage.close(self.clock.now(), &p.character);
        Ok(())
    }
    pub(crate) fn pending_achievement_credits(
        &self,
    ) -> Vec<crate::store::achievements::ClosureCredit> {
        self.achievement_credits.iter().take(85).cloned().collect()
    }
    pub(crate) fn finish_achievement_credit(
        &mut self,
        credit: &crate::store::achievements::ClosureCredit,
    ) -> Result<()> {
        let i = self
            .achievement_credits
            .iter()
            .position(|c| c == credit)
            .context("achievement closure reservation disappeared")?;
        self.achievement_credits.remove(i);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn achievement_native_hit_requires_ack_and_counts_overkill_once() {
        let (mut w, id) = super::super::movement_tests::fixture(0x18, false);
        let spec = w.content.get(0xdf0).unwrap().projectiles[&0].clone();
        let bullet = Bullet {
            owner: 999,
            source_kind: 0xdf0,
            id: 255,
            friendly: false,
            start: Pos { x: 8.5, y: 8.5 },
            pos: Pos { x: 8.5, y: 8.5 },
            angle: 0.,
            born: w.now,
            damage: 500,
            spec,
            hit: BTreeSet::new(),
            hit_enemy: false,
        };
        w.native_player_hit(id, 999, 255).unwrap();
        assert!(w.players[&id].character.achievements.is_empty());
        assert!(native_hits::expect_shoot_ack(
            w.players.get_mut(&id).unwrap(),
            vec![bullet],
            &w.clock
        ));
        w.acknowledge_shoot(id, 1000).unwrap();
        w.native_player_hit(id, 999, 255).unwrap();
        assert_eq!(w.players[&id].character.hp, 0);
        assert!(
            w.players[&id]
                .character
                .achievements
                .contains(&Achievement::HitByTruck)
        );
        assert_eq!(w.players[&id].achievement_damage.0.len(), 1);
        w.native_player_hit(id, 999, 255).unwrap();
        assert_eq!(w.players[&id].achievement_damage.0.len(), 1);
        assert!(
            w.pending_deaths()[0]
                .1
                .achievements
                .contains(&Achievement::HitByTruck)
        );
    }
    #[test]
    fn achievement_protection_area_and_suffocation_use_only_accepted_damage() {
        let (mut w, id) = super::super::movement_tests::fixture(0x18, false);
        w.players.get_mut(&id).unwrap().lab_conditions =
            crate::condition::bit(crate::condition::INVULNERABLE);
        w.area_damage(0xdf0, Pos { x: 8.5, y: 8.5 }, 2., 500)
            .unwrap();
        assert!(w.players[&id].character.achievements.is_empty());
        assert!(w.players[&id].achievement_damage.0.is_empty());
        w.players.get_mut(&id).unwrap().lab_conditions = 0;
        w.name = "Ocean Trench".into();
        w.now = 150;
        w.players.get_mut(&id).unwrap().oxygen = 0;
        w.ocean_step();
        assert_eq!(w.players[&id].achievement_damage.0[0].2, 10);
        w.area_damage(0xdf0, Pos { x: 8.5, y: 8.5 }, 2., 500)
            .unwrap();
        assert!(
            w.players[&id]
                .character
                .achievements
                .contains(&Achievement::HitByTruck)
        );
    }
    #[tokio::test]
    async fn achievement_escape_binds_qualified_damage_to_the_reserved_nexus_source() {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let account = store.list("achievement-escape".into()).await.unwrap().id;
        let mut ch = store
            .create_prepared(account, content.get(0x300).unwrap(), None)
            .await
            .unwrap();
        ch.level = 20;
        ch.base[0] = 250;
        ch.hp = 250;
        store.progress(ch.clone()).await.unwrap();
        let mut d =
            Directory::prepared(std::path::Path::new("data"), content, 16, 1376, 2).unwrap();
        let (mut w, id) = super::super::movement_tests::fixture(0x18, false);
        w.players.get_mut(&id).unwrap().character = ch.clone();
        w.area_damage(0xdf0, Pos { x: 8.5, y: 8.5 }, 2., 221)
            .unwrap();
        assert_eq!(w.players[&id].character.hp, 29);
        d.worlds.insert(-3, w);
        let (keytime, key, _) = d.issue(ch.account, ch.id, -2).unwrap();
        let mut reservation = d.reserve_transfer(key.clone()).unwrap();
        d.bind_achievement_escape(id, &key).unwrap();
        let snapshot = d.begin_source_departure(id, Some(&key)).unwrap().unwrap();
        assert!(snapshot.achievements.contains(&Achievement::NotEvenClose));
        assert!(!ch.achievements.contains(&Achievement::NotEvenClose));
        d.accept_source_write(id, &key).unwrap();
        store.progress(snapshot.clone()).await.unwrap();
        d.finish_source_departure(id, snapshot, Some(key.clone()))
            .unwrap();
        reservation.disarm();
        let (_, arrival) = d.consume(account, -2, keytime, key.clone()).unwrap();
        d.admit_transfer(arrival, account, ch.id, -2).unwrap();
        let loaded = store.load(account, ch.id).await.unwrap();
        let presentation = store.presentation(account).await.unwrap();
        assert_eq!(presentation.credits, 110);
        let next = d
            .worlds
            .get_mut(&-2)
            .unwrap()
            .enter_admitted(
                loaded,
                None,
                None,
                presentation,
                false,
                Some(key),
                Controller::Qa,
            )
            .unwrap();
        assert_eq!(d.location(next).unwrap(), -2);
        assert_eq!(d.worlds[&-2].players[&next].presentation.credits, 110);
        assert_eq!(
            store.gold(account).await.unwrap().rewards,
            vec![Achievement::NotEvenClose.reward()]
        );
    }
    #[tokio::test]
    async fn achievement_closure_credit_is_created_only_after_acceptance_for_the_actual_human_killer()
     {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let mut d =
            Directory::prepared(std::path::Path::new("data"), content, 16, 1376, 2).unwrap();
        let (mut w, id) = super::super::movement_tests::fixture(0x18, false);
        w.name = "Realm".into();
        w.encounter = true;
        let root = w
            .insert_entity(0x668, Pos { x: 12.5, y: 8.5 }, vec![])
            .unwrap();
        let other = w
            .insert_entity(0x67d, Pos { x: 20.5, y: 8.5 }, vec![])
            .unwrap();
        let ch = w.players[&id].character.clone();
        d.worlds.insert(-3, w);
        d.endgame.realms.remove(&-3);
        d.bind_realm_heroes(-3, BTreeSet::from([root, other]))
            .unwrap();
        d.worlds
            .get_mut(&-3)
            .unwrap()
            .damage_enemy(id, root, 100_000, true, &[], None)
            .unwrap();
        assert!(d.pending_achievement_credits().is_empty());
        d.endgame_step(&[-3]);
        let credits = d.pending_achievement_credits();
        assert_eq!(credits.len(), 1);
        assert_eq!(
            (credits[0].account, credits[0].character, credits[0].root),
            (ch.account, ch.id, root)
        );
        d.endgame_step(&[-3]);
        assert_eq!(d.pending_achievement_credits(), credits);
        d.worlds.get_mut(&-3).unwrap().defeat_entity(other).unwrap();
        d.endgame_step(&[-3]);
        assert_eq!(d.pending_achievement_credits(), credits);
        d.finish_achievement_credit(&credits[0]).unwrap();
        assert!(d.pending_achievement_credits().is_empty());
    }
    #[tokio::test]
    async fn achievement_main_token_forced_closure_never_creates_earned_realm_credit() {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let mut d =
            Directory::prepared(std::path::Path::new("data"), content, 16, 1376, 2).unwrap();
        let (mut w, player) = super::super::movement_tests::fixture(0x18, false);
        w.name = "Realm".into();
        w.encounter = true;
        let root = w
            .insert_entity(0x668, Pos { x: 12.5, y: 8.5 }, vec![])
            .unwrap();
        d.worlds.insert(-3, w);
        d.endgame.realms.remove(&-3);
        d.bind_realm_heroes(-3, BTreeSet::from([root])).unwrap();
        assert!(d.pending_achievement_credits().is_empty());
        d.force_realm_closure(player).unwrap();
        assert!(d.endgame.realms[&-3].cycle.remaining.is_empty());
        assert!(!d.endgame.realms[&-3].cycle.admits());
        d.worlds
            .get_mut(&-3)
            .unwrap()
            .damage_enemy(player, root, 100_000, true, &[], None)
            .unwrap();
        d.endgame_step(&[-3]);
        assert!(d.pending_achievement_credits().is_empty());
        assert!(
            !d.worlds[&-3].players[&player]
                .character
                .achievements
                .contains(&Achievement::RealmCloser)
        );
    }
    #[tokio::test]
    async fn achievement_death_attempt_stays_volatile_through_required_progress_and_atomic_settlement()
     {
        for resurrect in [false, true] {
            let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
            let dir =
                std::env::temp_dir().join(format!("achievement-order-{}", rand::random::<u64>()));
            std::fs::create_dir(&dir).unwrap();
            let path = dir.join("game.db");
            let store = crate::store::Store::open(&path, &content).unwrap();
            let account = store.list("owned-nexus-order".into()).await.unwrap().id;
            let mut ch = store
                .create(account, content.get(crate::content::WIZARD).unwrap())
                .await
                .unwrap();
            if resurrect {
                ch.items[3] = 0xb3e;
                ch.revision = store
                    .exchange(
                        vec![crate::store::InventoryWrite {
                            id: ch.id,
                            account,
                            revision: ch.revision,
                            items: ch.items.clone(),
                        }],
                        vec![],
                    )
                    .await
                    .unwrap()[0];
            }
            let (mut w, id) = super::super::movement_tests::fixture(0x18, false);
            let p = w.players.get_mut(&id).unwrap();
            p.character = ch.clone();
            p.character.hp = 0;
            p.death_pending = true;
            p.character
                .achievements
                .extend([Achievement::NexusAfterDeath, Achievement::HitByTruck]);
            p.queue_experience(1, None, false).unwrap();
            let progress = w.pending_progress_where(|_, _| true).pop().unwrap().1;
            assert!(
                !progress
                    .achievements
                    .contains(&Achievement::NexusAfterDeath)
            );
            assert!(progress.achievements.contains(&Achievement::HitByTruck));
            store.progress(progress.clone()).await.unwrap();
            w.finish_progress(id, progress).unwrap();
            assert!(
                w.players[&id]
                    .character
                    .achievements
                    .contains(&Achievement::NexusAfterDeath)
            );
            assert!(
                !store
                    .load(account, ch.id)
                    .await
                    .unwrap()
                    .achievements
                    .contains(&Achievement::NexusAfterDeath)
            );
            let fatal = w.pending_deaths().pop().unwrap().1;
            assert!(fatal.achievements.contains(&Achievement::NexusAfterDeath));
            if resurrect {
                let restored = store
                    .resurrect(fatal.clone(), 3, content.clone())
                    .await
                    .unwrap();
                assert!(
                    !restored
                        .achievements
                        .contains(&Achievement::NexusAfterDeath)
                );
                assert!(restored.achievements.contains(&Achievement::HitByTruck));
            } else {
                store
                    .death(fatal, "actual volatile attempt settlement".into())
                    .await
                    .unwrap();
            }
            assert_eq!(
                store.presentation(account).await.unwrap().credits,
                if resurrect { 125 } else { 135 }
            );
            assert_eq!(store.lifecycle_pending_jobs(), 0);
            drop(store);
            let reopened = crate::store::Store::open(&path, &content).unwrap();
            assert_eq!(
                reopened.presentation(account).await.unwrap().credits,
                if resurrect { 125 } else { 135 }
            );
            drop(reopened);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
    #[test]
    fn damage_windows_distinguish_overkill_from_hp_loss_and_exact_boundaries() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
        ch.level = 20;
        ch.hp = 29;
        let t = Instant::now();
        let mut w = DamageWindow::default();
        assert!(!w.record(t, 499, 200).unwrap());
        assert!(!w.close(t, &ch));
        assert!(
            w.record(t + std::time::Duration::from_millis(250), 1, 1)
                .unwrap()
        );
        assert!(w.close(t + std::time::Duration::from_millis(1000), &ch));
        assert!(!w.close(t + std::time::Duration::from_millis(1001), &ch));
        ch.hp = 30;
        assert!(!w.close(t, &ch));
        ch.hp = 29;
        ch.level = 19;
        assert!(!w.close(t, &ch));
        let mut w = DamageWindow::default();
        assert!(w.record(t, 500, 100).unwrap());
        assert!(
            !w.record(t + std::time::Duration::from_millis(251), 1, 1)
                .unwrap()
        );
    }
}
