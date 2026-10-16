use super::*;
use crate::{
    condition,
    encounter::{self, Anchor, Motion},
};
fn directory(seed: u64) -> Directory {
    Directory::prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        seed,
    )
    .unwrap()
}
fn camp(w: &World, kind: u16) -> usize {
    w.camps
        .iter()
        .position(|c| c.family.leader == kind)
        .unwrap()
}
fn member(w: &World, kind: u16) -> i32 {
    let c = camp(
        w,
        if matches!(kind, 0x900..=0x902) {
            0x900
        } else {
            kind
        },
    );
    w.population[w.camps[c].start].entity.unwrap()
}
fn player(w: &mut World, pos: Pos) -> i32 {
    let ch = tests::ch(w);
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = pos;
    p
}
fn all_members(w: &World, c: usize) -> Vec<i32> {
    w.entities
        .values()
        .filter(|e| e.camp == Some(c))
        .map(|e| e.id)
        .collect()
}

#[test]
fn selected_pack_is_complete_but_fragmented_or_absent_biomes_never_get_fake_ground() {
    let d = directory(12351);
    let w = &d.worlds[&-3];
    for kind in [0x6c6, 0x62c, 0x61f, 0x649, 0x640, 0x900, 0x645, 0x650] {
        assert!(w.entities.values().any(|e| e.kind == kind), "{kind:#x}");
    }
    assert!(w.entities.values().any(|e| e.kind == 0x646));
    for kind in [0x624, 0x625, 0x626] {
        assert!(w.entities.values().any(|e| e.kind == kind));
    }
    for seed in [1, 42] {
        let d = directory(seed);
        let w = &d.worlds[&-3];
        for terrain in 2..=7 {
            assert!(w.population_count(terrain) <= 32);
        }
        for s in w
            .population
            .iter()
            .filter(|s| (2..=7).contains(&w.content.objects[&s.kind].terrain))
        {
            let cell = w.map.cell(s.pos.x, s.pos.y).unwrap();
            assert_eq!(cell.terrain, w.content.objects[&s.kind].terrain);
            assert!(cell.object.is_none());
            assert!(w.walkable(s.pos));
        }
        if seed == 1 {
            assert_eq!(
                w.camps.iter().filter(|c| c.family.leader == 0x61f).count(),
                2
            );
        }
        if seed == 42 {
            assert_eq!(w.population_count(4), 0);
        }
    }
}

#[test]
fn ordinary_seed_density_stays_within_real_body_budgets_and_preserves_closure() {
    let mut w = tests::world();
    w.populate_solos(2, &[], &[0x646]).unwrap();
    assert!(w.population.is_empty());
    assert!(w.populate_solos(1, &[], &[0x651]).is_err());
    assert!(w.populate_solos(2, &[], &[0x651]).is_err());
    for seed in 2..=20 {
        let d = directory(seed);
        let w = &d.worlds[&-3];
        for terrain in 1..=12 {
            assert!(
                w.population_count(terrain) <= if terrain >= 11 { 24 } else { 32 },
                "seed{seed} terrain{terrain}"
            );
        }
        assert_eq!(w.hero_identities().len(), 10);
        assert!(w.critical_deaths.is_empty());
    }
}

#[test]
fn royal_healing_and_mage_orbits_cannot_adopt_another_orc_camp() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let king = member(w, 0x62c);
    let queen = w.camp_member(king, 0x62b).unwrap().unwrap();
    let mage = w.camp_member(king, 0x62a).unwrap().unwrap();
    let other = w
        .entities
        .values()
        .find(|e| e.kind == 0x62c && e.id != king)
        .unwrap()
        .id;
    w.entities.get_mut(&king).unwrap().hp = 100;
    w.entities.get_mut(&other).unwrap().hp = 100;
    w.midland_mechanics(queen).unwrap();
    assert_eq!(w.entities[&king].hp, 200);
    assert_eq!(w.entities[&other].hp, 100);
    w.now = 499;
    w.midland_mechanics(queen).unwrap();
    assert_eq!(w.entities[&king].hp, 200);
    w.now = 500;
    w.midland_mechanics(queen).unwrap();
    assert_eq!(w.entities[&king].hp, 240);
    w.entities.get_mut(&king).unwrap().hp = 100;
    w.apply_condition(king, condition::SICK, 1000).unwrap();
    w.now = 1000;
    w.midland_mechanics(queen).unwrap();
    assert_eq!(w.entities[&king].hp, 100);
    assert!(matches!(
        w.midland_movement(mage, w.entities[&king].pos, 0)
            .unwrap()
            .0,
        Motion::Orbit {
            anchor: Anchor::CampMember(0x62b),
            ..
        }
    ));
    let c = w.entities[&king].camp.unwrap();
    let queens: Vec<_> = all_members(w, c)
        .into_iter()
        .filter(|id| w.entities[id].kind == 0x62b)
        .collect();
    for q in queens {
        w.remove_entity(q);
    }
    assert!(w.camp_member(mage, 0x62b).unwrap().is_none());
    assert!(matches!(
        w.midland_movement(mage, w.entities[&king].pos, 0)
            .unwrap()
            .0,
        Motion::Wander
    ));
}

#[test]
fn shield_orc_commander_heals_its_group_then_telegraphs_all_final_bursts_and_leaves_survivors() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let key = member(w, 0x900);
    let c = w.entities[&key].camp.unwrap();
    let ids = all_members(w, c);
    assert_eq!(ids.len(), 4);
    let shield = w.camp_member(key, 0x902).unwrap().unwrap();
    let flood = w.camp_member(key, 0x901).unwrap().unwrap();
    let pos = w.entities[&key].pos;
    let p = player(
        w,
        Pos {
            x: pos.x + 5.,
            y: pos.y,
        },
    );
    assert_eq!(w.entities[&shield].hp, 5000);
    assert_eq!(w.entities[&flood].hp, 2000);
    assert_eq!(w.content.objects[&0x901].defense, 60);
    w.entities.get_mut(&key).unwrap().hp = 700;
    w.entities.get_mut(&shield).unwrap().hp = 2000;
    w.midland_mechanics(shield).unwrap();
    assert_eq!(w.entities[&key].hp, 950);
    assert_eq!(w.entities[&shield].hp, 2250);
    w.now = 100;
    let char_id = w.players[&p].character.id;
    let e = w.entities.get_mut(&key).unwrap();
    e.hp = 499;
    e.damage.insert(char_id, 501);
    w.midland_mechanics(key).unwrap();
    assert_eq!(w.entities[&key].expires, Some(1100));
    assert!(ids.iter().all(|id| w.entities[id].stage == Some(2)));
    assert!(w.shield_shots(key, 0.).unwrap().is_empty());
    w.players.get_mut(&p).unwrap().paused = true;
    for id in &ids {
        w.apply_condition(*id, condition::STUNNED, 2000).unwrap();
    }
    w.now = 1050;
    let before = w.bullets.iter().filter(|b| ids.contains(&b.owner)).count();
    w.npc_step();
    assert_eq!(
        w.bullets.iter().filter(|b| ids.contains(&b.owner)).count() - before,
        40
    );
    w.npc_step();
    assert_eq!(
        w.bullets.iter().filter(|b| ids.contains(&b.owner)).count() - before,
        40
    );
    w.now = 1100;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&key));
    assert!(
        ids.iter()
            .filter(|id| **id != key)
            .all(|id| w.entities.contains_key(id))
    );
    assert_eq!(w.players[&p].character.kills, 1);
    let xp = w.players[&p].experience_queued;
    assert!(xp > 0);
    w.mechanic_expiry();
    assert_eq!(w.players[&p].experience_queued, xp);
    for id in ids.into_iter().filter(|id| *id != key) {
        w.midland_mechanics(id).unwrap();
        assert!(w.shield_shots(id, 0.).unwrap().is_empty());
        assert!(w.entities[&id].parent.is_none() && w.entities[&id].expires.is_none());
    }
    w.now = 100000;
    w.repopulate().unwrap();
    assert!(w.population[w.camps[c].start].entity.is_none());
}

#[test]
fn lesser_gods_own_reinforcement_not_their_nonreproducing_kings() {
    for god_kind in [0x640, 0x650] {
        let mut d = directory(12351);
        let w = d.worlds.get_mut(&-3).unwrap();
        let god = member(w, god_kind);
        let c = w.entities[&god].camp.unwrap();
        let kind = if god_kind == 0x640 { 0x63e } else { 0x64f };
        let king = w.camp_member(god, kind).unwrap().unwrap();
        let followers = all_members(w, c);
        player(w, w.entities[&god].pos);
        w.remove_entity(king);
        w.now = 10000;
        w.repopulate().unwrap();
        let next = w.camp_member(god, kind).unwrap().unwrap();
        assert_ne!(king, next);
        assert!(
            followers
                .into_iter()
                .filter(|id| *id != king)
                .all(|id| w.entities.contains_key(&id))
        );
        w.remove_entity(god);
        w.remove_entity(next);
        w.now = 30000;
        w.repopulate().unwrap();
        assert!(
            !w.entities
                .values()
                .any(|e| e.camp == Some(c) && e.kind == kind)
        );
        assert!(
            all_members(w, c)
                .iter()
                .all(|id| w.entities[id].parent.is_none())
        );
    }
}

#[test]
fn nonreproducing_ogre_bands_use_melee_king_and_ranged_wizard_without_free_replacements() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let king = member(w, 0x645);
    let c = w.entities[&king].camp.unwrap();
    assert_eq!(all_members(w, c).len(), 9);
    assert!(w.camps[c].family.roles.iter().all(|r| r.interval.is_none()));
    assert_eq!(w.entities[&king].hp, 500);
    assert!(matches!(
        w.midland_movement(king, w.entities[&king].pos, 0).unwrap(),
        (Motion::Chase { stop: 1., .. }, 12.)
    ));
    assert_eq!(encounter::phased_attacks(0x644, 300, 0., 0).len(), 3);
    let wizard = w.camp_member(king, 0x644).unwrap().unwrap();
    w.remove_entity(wizard);
    player(w, w.entities[&king].pos);
    w.now = 60000;
    w.repopulate().unwrap();
    assert_eq!(all_members(w, c).len(), 8);
}

#[test]
fn phases_prediction_and_zero_damage_conditions_stay_selected_and_use_the_shared_pipeline() {
    let mut d = directory(12351);
    let w = d.worlds.get_mut(&-3).unwrap();
    let dwarf = member(w, 0x61f);
    let home = w.entities[&dwarf].pos;
    assert!(matches!(
        w.midland_movement(dwarf, home, 0).unwrap().0,
        Motion::Chase { .. }
    ));
    assert!(matches!(
        w.midland_movement(dwarf, home, 2500).unwrap().0,
        Motion::Wander
    ));
    assert_eq!(encounter::phased_attacks(0x6c6, 140, 0., 0).len(), 3);
    assert_eq!(
        encounter::phased_attacks(0x6c6, 140, 0., 1000),
        vec![(1, 0.)]
    );
    assert!(encounter::phased_attacks(0x6c6, 140, 0., 600).is_empty());
    for (kind, index, effect, duration) in [
        (
            0x6c6,
            1,
            condition::projectile_effect("Blind").unwrap(),
            1500,
        ),
        (0x649, 1, condition::QUIET, 10000),
        (
            0x640,
            1,
            condition::projectile_effect("Confused").unwrap(),
            3000,
        ),
    ] {
        assert_eq!(
            w.content.objects[&kind].projectiles[&index].effects,
            vec![(effect, duration)]
        );
    }
    let flayer = member(w, 0x649);
    let p = player(w, w.entities[&flayer].pos);
    w.apply_condition(flayer, condition::STUNNED, 1000).unwrap();
    w.apply_condition(flayer, condition::PARALYZED, 1000)
        .unwrap();
    let pos = w.entities[&flayer].pos;
    w.npc_step();
    assert_eq!(w.entities[&flayer].pos.distance(pos), 0.);
    assert!(!w.bullets.iter().any(|b| b.owner == flayer));
    assert!(w.players[&p].character.alive);
    let hp = w.players[&p].character.hp;
    let star = Bullet {
        owner: flayer,
        source_kind: 0x649,
        id: 1,
        friendly: false,
        start: pos,
        pos,
        angle: 0.,
        born: w.now,
        damage: 0,
        spec: w.content.objects[&0x649].projectiles[&1].clone(),
        hit: BTreeSet::new(),
    };
    w.damage_player(p, &star, true).unwrap();
    assert_eq!(w.players[&p].character.hp, hp);
    assert_ne!(
        w.players[&p].conditions(w.now) & condition::bit(condition::QUIET),
        0
    );
    w.players.get_mut(&p).unwrap().statuses.clear();
    w.apply_condition(p, condition::INVULNERABLE, 1000).unwrap();
    w.damage_player(p, &star, true).unwrap();
    assert_eq!(w.players[&p].character.hp, hp);
    assert_eq!(
        w.players[&p].conditions(w.now) & condition::bit(condition::QUIET),
        0
    );
    let before = w.entities[&dwarf].hp;
    assert_eq!(
        w.damage_enemy(p, dwarf, 0, false, &[(condition::STUNNED, 1000)], None)
            .unwrap(),
        0
    );
    assert_eq!(w.entities[&dwarf].hp, before);
    assert_ne!(
        w.entities[&dwarf].conditions(w.now) & condition::bit(condition::STUNNED),
        0
    );
    for kind in [0x640, 0x645, 0x649, 0x650] {
        assert_eq!(crate::dungeon::portal_drop(kind), Some((0x718, 0.17)));
    }
    assert!(crate::dungeon::portal_drop(0x62c).is_none());
}
