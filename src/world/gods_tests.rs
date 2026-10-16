use super::*;
use crate::{condition, encounter};
fn directory() -> Directory {
    Directory::prepared(
        std::path::Path::new("data"),
        tests::world().content,
        16,
        12351,
    )
    .unwrap()
}
fn enemy(w: &World, kind: u16) -> i32 {
    w.entities.values().find(|e| e.kind == kind).unwrap().id
}
fn player(w: &mut World, pos: Pos) -> i32 {
    let mut ch = tests::ch(w);
    ch.id = w.players.len() as i32 + 1;
    ch.account = ch.id;
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos = pos;
    p
}

#[test]
fn complete_godlands_counts_all_real_children_and_trio_inside_32_bodies() {
    let mut d = directory();
    let w = d.worlds.get_mut(&-3).unwrap();
    for kind in gods::GODS
        .iter()
        .chain(gods::CONSTRUCTS)
        .chain([0x653].iter())
    {
        assert!(w.entities.values().any(|e| e.kind == *kind));
    }
    assert_eq!(w.population_count(1), 28);
    assert!(!w.population.iter().any(|s| s.kind == 0x935));
    let leaders: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == 0x652)
        .map(|e| (e.id, e.pos))
        .collect();
    for (_, pos) in &leaders {
        player(w, *pos);
    }
    for time in [10000, 20000, 30000] {
        w.now = time;
        w.repopulate().unwrap();
        assert!(w.population_count(1) <= 32);
    }
    assert_eq!(w.population_count(1), 32);
    for (id, _) in leaders {
        let camp = w.entities[&id].camp.unwrap();
        let children: Vec<_> = w
            .entities
            .values()
            .filter(|e| e.camp == Some(camp) && e.kind == 0x653)
            .collect();
        assert_eq!(children.len(), 5);
        assert!(
            children
                .iter()
                .all(|e| e.parent.is_none() && e.expires.is_none() && e.size == 75)
        );
    }
    assert_eq!(w.hero_identities().len(), 10);
}

#[test]
fn independent_god_volley_clocks_and_leviathan_fans_use_selected_types() {
    let shots = |kind, ms| encounter::phased_attacks(kind, 2000, 0.2, ms);
    let mut counts = [0; 2];
    for ms in (0..3000).step_by(encounter::profile(0x656).unwrap().interval as usize) {
        for (index, _) in shots(0x656, ms) {
            counts[index as usize] += 1;
        }
    }
    assert_eq!(counts, [20, 3]);
    let mut counts = [0; 2];
    for ms in (0..13000).step_by(50) {
        for (index, _) in shots(0x658, ms) {
            counts[index as usize] += 1;
        }
    }
    assert_eq!(counts, [65, 20]);
    for (ms, count, index) in [(0, 9, 0), (1500, 12, 0), (3000, 2, 1)] {
        let volley = shots(0x6d8, ms);
        assert_eq!(volley.len(), count);
        assert!(volley.iter().all(|s| s.0 == index));
    }
    assert!(shots(0x91a, 2500).is_empty());
    for ms in [2700, 2900, 3100, 3300, 3500] {
        assert_eq!(shots(0x91a, ms).len(), 8);
    }
    assert!(shots(0x91a, 3700).is_empty());
    let volley = shots(0x656, 750);
    assert_eq!(volley.len(), 5);
    assert!((volley[1].1 - volley[0].1 - std::f32::consts::TAU / 5.).abs() < 0.0001);
}

#[test]
fn native_observed_motion_leads_god_shots_without_changing_input_authority() {
    let mut d = directory();
    let w = d.worlds.get_mut(&-3).unwrap();
    let god = enemy(w, 0x651);
    let pos = w.entities[&god].pos;
    let p = player(
        w,
        Pos {
            x: pos.x + 5.,
            y: pos.y,
        },
    );
    let credit = w.players[&p].movement_credit;
    w.native_move_to(
        p,
        Pos {
            x: pos.x + 5.,
            y: pos.y,
        },
        1000,
    )
    .unwrap();
    w.native_move_to(
        p,
        Pos {
            x: pos.x + 5.,
            y: pos.y + 0.4,
        },
        1100,
    )
    .unwrap();
    assert!((w.players[&p].motion.velocity.y - 4.).abs() < 0.001);
    assert_eq!(w.players[&p].movement_credit, credit);
    let target = w.players[&p].pos;
    let heading = (target.y - pos.y).atan2(target.x - pos.x);
    let predicted = w.god_shots(god, target, heading, 0).unwrap();
    assert!(predicted[1].1 > heading + 0.3);
    w.now = 201;
    let stale = w.god_shots(god, target, heading, 0).unwrap();
    assert!((stale[1].1 - heading).abs() < 0.0001);
    w.native_move_to(
        p,
        Pos {
            x: pos.x + 15.,
            y: pos.y + 5.,
        },
        1200,
    )
    .unwrap();
    assert_eq!(w.players[&p].motion.velocity.x, 0.);
    assert_eq!(w.players[&p].motion.velocity.y, 0.);
}

#[test]
fn djinn_has_real_shields_telegraph_final_burst_and_once_only_normal_rewards() {
    let mut d = directory();
    let w = d.worlds.get_mut(&-3).unwrap();
    let id = enemy(w, 0x91a);
    let home = w.entities[&id].pos;
    let p = player(
        w,
        Pos {
            x: home.x + 5.,
            y: home.y,
        },
    );
    w.entities.get_mut(&id).unwrap().activated = Some(0);
    w.god_mechanics(id, 0).unwrap();
    assert_ne!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.god_mechanics(id, 2500).unwrap();
    assert_eq!(
        w.entities[&id].conditions(w.now) & condition::bit(condition::INVULNERABLE),
        0
    );
    w.now = 100;
    let char_id = w.players[&p].character.id;
    let e = w.entities.get_mut(&id).unwrap();
    e.hp = 399;
    e.damage.insert(char_id, 601);
    w.god_mechanics(id, 2700).unwrap();
    assert_eq!(w.entities[&id].expires, Some(3100));
    assert!(
        w.god_shots(id, w.players[&p].pos, 0., 2700)
            .unwrap()
            .is_empty()
    );
    w.now = 3050;
    w.players.get_mut(&p).unwrap().paused = true;
    w.apply_condition(id, condition::STUNNED, 1000).unwrap();
    let packets = w.npc_step();
    assert!(packets.iter().any(|(owner, _)| *owner == id));
    assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 12);
    assert!(w.entities.contains_key(&id));
    w.npc_step();
    assert_eq!(w.bullets.iter().filter(|b| b.owner == id).count(), 12);
    w.players.get_mut(&p).unwrap().paused = false;
    w.now = 3100;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&id));
    let earned = w.players[&p].experience_queued;
    assert!(earned > 0);
    assert_eq!(w.players[&p].character.kills, 1);
    w.mechanic_expiry();
    assert_eq!(w.players[&p].experience_queued, earned);
    assert_eq!(w.players[&p].character.kills, 1);
    assert!(
        w.bullets
            .iter()
            .filter(|b| b.owner == id)
            .all(|b| b.source_kind == 0x91a && b.damage == 60)
    );
}

#[test]
fn constructs_heal_only_their_own_partner_and_withdraw_without_fake_kill_rewards() {
    let mut d = directory();
    let w = d.worlds.get_mut(&-3).unwrap();
    let rock = enemy(w, 0x905);
    let paper = enemy(w, 0x906);
    let steel = enemy(w, 0x907);
    let pos = w.entities[&rock].pos;
    let p = player(
        w,
        Pos {
            x: pos.x + 5.,
            y: pos.y,
        },
    );
    w.entities.get_mut(&steel).unwrap().hp = 500;
    let other = w
        .spawn_entity(0x907, w.entities[&steel].pos, vec![])
        .unwrap();
    w.entities.get_mut(&other).unwrap().hp = 500;
    w.god_mechanics(rock, 0).unwrap();
    assert_eq!(w.entities[&steel].hp, 2000);
    assert_eq!(w.entities[&other].hp, 500);
    w.entities.get_mut(&steel).unwrap().hp = 600;
    w.now = 999;
    w.god_mechanics(rock, 0).unwrap();
    assert_eq!(w.entities[&steel].hp, 600);
    w.now = 1000;
    w.god_mechanics(rock, 0).unwrap();
    assert_eq!(w.entities[&steel].hp, 2000);
    let original = w.players[&p].character.kills;
    w.entities.get_mut(&paper).unwrap().hp = 399;
    w.god_mechanics(paper, 0).unwrap();
    assert_eq!(
        w.god_shots(paper, w.players[&p].pos, 0., 0).unwrap().len(),
        8
    );
    w.now = 1050;
    w.mechanic_expiry();
    assert!(!w.entities.contains_key(&paper));
    assert_eq!(w.players[&p].character.kills, original);
    assert_eq!(w.players[&p].experience_queued, 0);
    w.now = 30000;
    w.repopulate().unwrap();
    assert!(!w.entities.values().any(|e| e.kind == 0x906));
}

#[test]
fn leviathan_magic_eye_retains_cloaked_targets_without_giving_other_gods_their_position() {
    let mut d = directory();
    let w = d.worlds.get_mut(&-3).unwrap();
    let levi = enemy(w, 0x6d8);
    let home = w.entities[&levi].pos;
    let p = player(
        w,
        Pos {
            x: home.x + 5.,
            y: home.y,
        },
    );
    w.apply_condition(p, condition::INVISIBLE, 1000).unwrap();
    w.npc_step();
    assert!(w.bullets.iter().any(|b| b.source_kind == 0x6d8));
    assert!(w.bullets.iter().all(|b| b.source_kind == 0x6d8));
}

#[test]
fn researched_stat_potions_are_independent_normal_drops_without_incantations() {
    let w = tests::world();
    for (kind, potion) in [
        (0x651, 0xa1f),
        (0x652, 0xa1f),
        (0x654, 0xa21),
        (0x655, 0xa20),
        (0x656, 0xa20),
        (0x657, 0xa1f),
        (0x658, 0xa20),
        (0x659, 0xa21),
        (0x91a, 0xa21),
        (0x6d8, 0xa20),
    ] {
        crate::loot::validate(&w.content, kind).unwrap();
        let mut rng = rand::rngs::mock::StepRng::new(0, 0);
        let drops = crate::loot::rewards(&w.content, kind, &mut rng);
        assert!(drops.contains(&potion));
        assert!(!drops.contains(&crate::endgame::INCANTATION));
    }
}
