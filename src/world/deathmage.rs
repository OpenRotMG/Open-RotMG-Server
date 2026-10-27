use super::*;
use crate::encounter::{Anchor, Motion, nr_speed};

impl World {
    // Formation reconstruction: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Deathmage.cs#L15-L202
    pub(super) fn deathmage_mechanics(&mut self, id: i32, target: Option<Pos>) -> Result<()> {
        let e = &self.entities[&id];
        if e.kind != 0x925 {
            let stage = match e.quest.and_then(|owner| self.entities.get(&owner)) {
                Some(owner) if owner.kind == 0x925 && e.pos.distance(owner.pos) < 10. => owner
                    .stage
                    .context("living Deathmage controller phase missing")?,
                _ => 0,
            };
            ensure!(stage <= 3, "invalid Deathmage order {stage}");
            if e.stage != Some(stage) {
                self.landmark_enter(id, stage);
            }
            return Ok(());
        }
        ensure!(e.quest == Some(id), "Deathmage lacks self-owned encounter");
        if matches!(e.stage, None | Some(0)) {
            self.landmark_enter(id, 1);
        }
        let e = &self.entities[&id];
        let stage = e.stage.unwrap();
        let next = match stage {
            1 if target.is_some_and(|p| e.pos.distance(p) < 15.) => Some(2),
            2 | 3 if target.is_none() => Some(1),
            2 | 3 if self.now - e.activated.context("Deathmage clock missing")? >= 2000 => {
                Some(5 - stage)
            }
            1..=3 => None,
            _ => bail!("invalid Deathmage phase {stage}"),
        };
        if let Some(next) = next {
            self.landmark_enter(id, next);
        }
        if self.now >= self.entities[&id].next_aux {
            for (child, min, max) in [
                (0x65a, 3, 5),
                (0x65b, 2, 4),
                (0x65c, 1, 3),
                (0x65d, 1, 2),
                (0x65e, 1, 1),
            ] {
                let count = self.rng.gen_range(min..=max);
                self.landmark_birth(id, child, max, count, 1.)?;
            }
            self.entities.get_mut(&id).unwrap().next_aux = self.now + 10000;
        }
        let e = &self.entities[&id];
        let (pos, stage) = (e.pos, e.stage.unwrap());
        let children: Vec<_> = self
            .quest_actors(id)
            .filter(|e| {
                matches!(e.kind, 0x65a..=0x65d)
                    && e.pos.distance(pos) < 10.
                    && e.stage != Some(stage)
            })
            .map(|e| e.id)
            .collect();
        for child in children {
            self.landmark_enter(child, stage);
        }
        Ok(())
    }
    pub(super) fn deathmage_movement(&self, id: i32, target: Option<Pos>) -> Result<(Motion, f32)> {
        let e = &self.entities[&id];
        let stage = e.stage.context("Deathmage movement phase missing")?;
        let follow = |sight, stop, speed| {
            (
                Motion::Follow {
                    sight,
                    stop,
                    idle: nr_speed(0.4),
                    duration: 0,
                    cooldown: 0,
                },
                nr_speed(speed),
            )
        };
        if e.kind == 0x925 {
            if e.pos.distance(e.home) > 5. {
                return Ok((
                    Motion::Protect {
                        anchor: Anchor::Home,
                        distance: 5.,
                    },
                    nr_speed(0.8),
                ));
            }
            return Ok(match stage {
                1 => (Motion::Wander, nr_speed(0.4)),
                2 if target.is_some_and(|p| e.pos.distance(p) < 10.) => (
                    Motion::Orbit {
                        anchor: Anchor::Player,
                        radius: 5.,
                    },
                    nr_speed(0.8),
                ),
                2 | 3 => follow(10., 8., 0.8),
                _ => bail!("invalid Deathmage movement phase {stage}"),
            });
        }
        Ok(match stage {
            0 => follow(10., if e.kind == 0x65d { 7. } else { 1. }, 1.),
            1 => {
                let owner = self.orbit_anchor(id, Anchor::QuestMember(0x925), target)?;
                if e.pos.distance(owner) > 2.
                    || matches!(e.motion, Some(Motion::Protect { .. }))
                        && e.pos.distance(owner) > 1.
                {
                    (
                        Motion::Protect {
                            anchor: Anchor::QuestMember(0x925),
                            distance: 1.,
                        },
                        nr_speed(1.),
                    )
                } else {
                    (Motion::Wander, nr_speed(0.4))
                }
            }
            2 if target.is_some_and(|p| e.pos.distance(p) < 10.) => (
                Motion::Orbit {
                    anchor: Anchor::Player,
                    radius: 10.,
                },
                nr_speed(1.),
            ),
            2 => (Motion::Wander, nr_speed(0.4)),
            3 => follow(15., 1., 1.),
            _ => bail!("invalid skeleton movement phase {stage}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition;

    fn world() -> World {
        let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
        let map = Map {
            width: 64,
            height: 64,
            cells: vec![
                crate::content::Cell {
                    ground: 0x48,
                    object: None,
                    config: String::new(),
                    terrain: 0,
                    region: 0
                };
                64 * 64
            ],
            spawns: vec![(1.5, 1.5)],
        };
        let mut w = World::new(content, Arc::new(map)).unwrap();
        w.encounter = true;
        w
    }
    fn root(w: &mut World, x: f32) -> i32 {
        let id = w.spawn_entity(0x925, Pos { x, y: 20. }, vec![]).unwrap();
        w.landmark_bind(id).unwrap();
        w.entities.get_mut(&id).unwrap().next_aux = u64::MAX;
        w.entities.get_mut(&id).unwrap().stage = Some(0);
        id
    }
    fn child(w: &mut World, owner: i32, kind: u16) -> i32 {
        w.landmark_birth(owner, kind, 1, 1, 1.).unwrap()[0]
    }
    #[test]
    fn deathmage_orders_are_timed_owned_and_released_without_adoption() {
        let mut w = world();
        let a = root(&mut w, 20.);
        let b = root(&mut w, 25.);
        let ca = child(&mut w, a, 0x65a);
        let cb = child(&mut w, b, 0x65d);
        let target = Some(Pos { x: 36., y: 20. });
        w.landmark_mechanics(a, target, 0).unwrap();
        w.landmark_mechanics(b, target, 0).unwrap();
        assert_eq!(w.entities[&a].stage, Some(1));
        assert_eq!(w.entities[&ca].stage, Some(1));
        assert_eq!(w.entities[&b].stage, Some(2));
        assert_eq!(w.entities[&cb].stage, Some(2));
        w.now = 1999;
        w.landmark_mechanics(b, target, 1999).unwrap();
        assert_eq!(w.entities[&cb].stage, Some(2));
        w.now = 2000;
        w.landmark_mechanics(b, target, 2000).unwrap();
        assert_eq!(w.entities[&cb].stage, Some(3));
        assert_eq!(w.entities[&ca].stage, Some(1));
        w.now = 4000;
        w.landmark_mechanics(b, target, 4000).unwrap();
        assert_eq!(w.entities[&cb].stage, Some(2));
        w.landmark_mechanics(b, None, 4000).unwrap();
        assert_eq!(w.entities[&cb].stage, Some(1));
        w.entities.get_mut(&ca).unwrap().pos.x = 31.;
        w.landmark_mechanics(ca, target, 4000).unwrap();
        assert_eq!(w.entities[&ca].stage, Some(0));
        w.entities.get_mut(&ca).unwrap().pos.x = 21.;
        w.landmark_mechanics(ca, target, 4000).unwrap();
        assert_eq!(w.entities[&ca].stage, Some(1));
        w.remove_entity(a);
        w.landmark_mechanics(ca, target, 4000).unwrap();
        assert_eq!(w.entities[&ca].stage, Some(0));
        assert_eq!(w.entities[&ca].quest, Some(a));
        assert_eq!(w.entities[&cb].quest, Some(b));
        let unowned = w
            .spawn_entity(0x65a, Pos { x: 25., y: 20. }, vec![])
            .unwrap();
        w.landmark_mechanics(unowned, target, 4000).unwrap();
        assert_eq!(w.entities[&unowned].quest, None);
        assert_eq!(w.entities[&unowned].stage, Some(0));
        w.entities.get_mut(&b).unwrap().stage = None;
        assert!(w.landmark_mechanics(cb, target, 4000).is_err());
    }
    #[test]
    fn deathmage_target_window_keeps_attack_hysteresis_and_home_tether() {
        let mut w = world();
        let a = root(&mut w, 20.);
        w.entities.get_mut(&a).unwrap().condition = condition::bit(condition::PARALYZED);
        let character: Character = serde_json::from_value(serde_json::json!({
            "schema":1,"lifetime":crate::store::FameLifetime::complete(),
            "class":0x300,"base":[200,100,15,0,75,15,15,10],
            "id":1,"account":1,"name":"Formation","level":20,"exp":0,
            "fame":0,"hp":200,"mp":100,"revision":0,"alive":true,
            "items":[0xa14,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
        }))
        .unwrap();
        let player = w.enter(character, None).unwrap();
        for (x, stage) in [(35.5, 1), (34.5, 2), (45., 2), (51., 1)] {
            w.players.get_mut(&player).unwrap().pos = Pos { x, y: 20. };
            w.now += 50;
            w.npc_step();
            assert_eq!(w.entities[&a].stage, Some(stage), "target at x={x}");
            assert_eq!(w.entities[&a].pos.distance(Pos { x: 20., y: 20. }), 0.);
            assert_eq!(w.entities[&a].bullet, 0);
        }
        w.entities.get_mut(&a).unwrap().pos.x = 26.;
        assert_eq!(
            w.deathmage_movement(a, None).unwrap(),
            (
                Motion::Protect {
                    anchor: Anchor::Home,
                    distance: 5.
                },
                nr_speed(0.8)
            )
        );
    }
    #[test]
    fn deathmage_roles_attacks_and_existing_roster_remain_distinct() {
        let mut w = world();
        let a = root(&mut w, 20.);
        let melee = child(&mut w, a, 0x65a);
        let mage = child(&mut w, a, 0x65d);
        w.remove_entity(a);
        for (id, stop) in [(melee, 1.), (mage, 7.)] {
            w.landmark_mechanics(id, None, 0).unwrap();
            assert!(
                matches!(w.deathmage_movement(id, None).unwrap(), (Motion::Follow { sight: 10., stop: s, .. }, speed) if s == stop && speed == nr_speed(1.))
            );
        }
        let a = root(&mut w, 40.);
        w.entities.get_mut(&a).unwrap().next_aux = 0;
        w.landmark_mechanics(a, None, 0).unwrap();
        for (kind, min, max) in [
            (0x65a, 3, 5),
            (0x65b, 2, 4),
            (0x65c, 1, 3),
            (0x65d, 1, 2),
            (0x65e, 1, 1),
        ] {
            assert!((min..=max).contains(&w.landmark_members(a, kind).len()));
        }
        assert_eq!(w.entities[&a].next_aux, 10000);
        let attack =
            |kind, stage| crate::encounter::landmarks::schedule(kind, 900, stage, 0, 0).attacks[0];
        assert!(!attack(0x925, 1).enabled);
        let volley = attack(0x925, 2);
        assert!(volley.enabled && volley.count == 3 && volley.range == 10.);
        assert_eq!(volley.gap, 15_f32.to_radians());
        assert_eq!(attack(0x65a, 0).range, 3.);
        assert_eq!(attack(0x65d, 0).range, 10.);
        assert_eq!(attack(0x65e, 0).interval, Some(1000));
    }
}
