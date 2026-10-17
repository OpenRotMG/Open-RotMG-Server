use super::{tests, *};

fn player(w: &mut World, class: u16, account: i32, pos: Pos) -> i32 {
    let mut ch = tests::ch(w);
    ch.id = account;
    ch.account = account;
    ch.class = class;
    ch.base = w.content.classes[&class].caps;
    ch.hp = ch.base[0];
    ch.mp = ch.base[1];
    ch.items = w.content.get(class).unwrap().equipment.clone();
    let id = w
        .enter_at(
            ch,
            None,
            Some(pos),
            AccountPresentation {
                guild: None,
                credits: 100,
                fame: 0,
                best_fame: BTreeMap::new(),
            },
        )
        .unwrap();
    w.players.get_mut(&id).unwrap().controller = Controller::Human;
    id
}
fn fixture(class: u16) -> (World, i32) {
    let mut w = tests::world();
    w.encounter = true;
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut w.map).cells {
        cell.ground = 0x61;
    }
    let id = player(&mut w, class, 1, Pos { x: 20., y: 20. });
    w.now = 20000;
    (w, id)
}
fn cast(w: &mut World, id: i32) -> Result<Option<InventoryPlan>> {
    let p = &w.players[&id];
    w.native_use_item(
        id,
        w.now as i32,
        Slot {
            object: id,
            slot: 1,
            kind: p.character.items[1],
        },
        p.pos,
    )
}
fn shot(w: &mut World, id: i32, time: i32, kind: u16, angle: f32, aim: f32) -> Result<NativeInput> {
    let p = &w.players[&id];
    let start = Pos {
        x: p.pos.x + 0.3 * aim.cos(),
        y: p.pos.y + 0.3 * aim.sin(),
    };
    w.native_shoot_received(
        id,
        (time, Instant::now()),
        p.next_bullet,
        kind,
        start,
        angle,
    )
}
#[test]
fn rogue_cloak_removes_target_without_granting_invulnerability_and_expires() {
    let (mut w, id) = fixture(0x300);
    let enemy = w
        .spawn_entity(0x646, Pos { x: 24., y: 20. }, vec![])
        .unwrap();
    w.npc_step();
    let held = w.entities[&enemy].pos;
    let mp = w.players[&id].character.mp;
    cast(&mut w, id).unwrap();
    assert_eq!(w.players[&id].character.mp, mp - 60);
    assert_ne!(
        w.players[&id].conditions(w.now) & crate::condition::bit(13),
        0
    );
    assert_eq!(
        crate::condition::damage(25, 0, w.players[&id].conditions(w.now), false),
        25
    );
    let bullets = w.bullets.len();
    for _ in 0..5 {
        w.npc_step();
    }
    assert_eq!(w.entities[&enemy].pos.distance(held), 0.);
    assert_eq!(w.bullets.len(), bullets);
    w.now += 3000;
    assert_eq!(
        w.players[&id].conditions(w.now) & crate::condition::bit(13),
        0
    );
    w.npc_step();
    assert!(w.entities[&enemy].pos.distance(held) > 0.);
}
#[test]
fn invisible_rogue_does_not_hide_visible_teammate_from_enemy() {
    let (mut w, id) = fixture(0x300);
    let friend = player(&mut w, crate::content::WIZARD, 2, Pos { x: 24.9, y: 20. });
    let enemy = w
        .spawn_entity(0x646, Pos { x: 22., y: 20. }, vec![])
        .unwrap();
    cast(&mut w, id).unwrap();
    w.npc_step();
    assert!(w.entities[&enemy].pos.x > 22.);
    assert_eq!(
        w.players[&friend].conditions(w.now) & crate::condition::bit(13),
        0
    );
}
#[test]
fn warrior_helm_buffs_nearby_allies_but_only_speeds_caster_and_expires() {
    let (mut w, id) = fixture(0x31d);
    let friend = player(&mut w, crate::content::WIZARD, 2, Pos { x: 23.9, y: 20. });
    let distant = player(&mut w, crate::content::WIZARD, 3, Pos { x: 24.1, y: 20. });
    let cooldown =
        reconciliation::attack_cooldown(&w.players[&friend], &w.content, w.now, 1.).unwrap();
    let speed = w.players[&id].speed(&w.content, w.now).unwrap();
    cast(&mut w, id).unwrap();
    assert_ne!(w.players[&id].conditions(w.now) & SPEEDY, 0);
    assert_eq!(w.players[&friend].conditions(w.now) & SPEEDY, 0);
    assert_ne!(
        w.players[&friend].conditions(w.now) & crate::condition::bit(20),
        0
    );
    assert_eq!(
        w.players[&distant].conditions(w.now) & crate::condition::bit(20),
        0
    );
    assert!(
        reconciliation::attack_cooldown(&w.players[&friend], &w.content, w.now, 1.).unwrap()
            <= cooldown / 1.5 + 1.
    );
    assert!((w.players[&id].speed(&w.content, w.now).unwrap() / speed - 1.5).abs() < 0.001);
    w.apply_condition(friend, crate::condition::DAZED, 1000)
        .unwrap();
    assert_eq!(
        reconciliation::attack_cooldown(&w.players[&friend], &w.content, w.now, 1.).unwrap(),
        666.
    );
    w.now += 3000;
    assert_eq!(
        w.players[&id].conditions(w.now) & (SPEEDY | crate::condition::bit(20)),
        0
    );
}
#[test]
fn paladin_seal_heals_and_increases_weapon_damage_not_ability_damage() {
    let (mut w, id) = fixture(0x31f);
    let friend = player(&mut w, crate::content::WIZARD, 2, Pos { x: 23., y: 20. });
    let distant = player(&mut w, crate::content::WIZARD, 3, Pos { x: 16., y: 24. });
    w.players.get_mut(&friend).unwrap().character.hp = 100;
    let seed = w.players[&friend].seed;
    let kind = w.players[&friend].character.items[0] as u16;
    shot(&mut w, friend, 20000, kind, 0., 0.).unwrap();
    let base_damage = w.bullets.last().unwrap().damage;
    cast(&mut w, id).unwrap();
    assert_eq!(
        w.players[&distant].conditions(w.now)
            & (crate::condition::bit(18) | crate::condition::bit(19)),
        0
    );
    w.players.get_mut(&friend).unwrap().seed = seed;
    shot(&mut w, friend, 21000, kind, 0., 0.).unwrap();
    assert!(w.bullets.last().unwrap().damage >= base_damage * 3 / 2);
    w.step();
    assert!(w.players[&friend].character.hp >= 101);
    w.apply_condition(friend, crate::condition::SICK, 1000)
        .unwrap();
    let hp = w.players[&friend].character.hp;
    w.step();
    assert_eq!(w.players[&friend].character.hp, hp);
    w.now = 25000;
    assert_eq!(
        w.players[&friend].conditions(w.now)
            & (crate::condition::bit(18) | crate::condition::bit(19)),
        0
    );
}
#[test]
fn tiered_shield_volley_consumes_one_cast_and_shared_rng_then_allows_sword() {
    for kind in [0xa08, 0xa09, 0xa0a, 0xacf, 0xa0b, 0xa0c, 0xb22] {
        let (mut w, id) = fixture(0x31e);
        w.players.get_mut(&id).unwrap().character.items[1] = kind;
        let item = w.content.get(kind as u16).unwrap().clone();
        let mp = w.players[&id].character.mp;
        w.apply_condition(id, crate::condition::DAMAGING, 2000)
            .unwrap();
        w.apply_condition(id, crate::condition::WEAK, 2000).unwrap();
        cast(&mut w, id).unwrap();
        let before_seed = w.players[&id].seed;
        let mut seed = before_seed;
        for i in 0..item.shots {
            let angle = item.arc.to_radians() * (i as f32 - (item.shots - 1) as f32 / 2.);
            shot(&mut w, id, 20000, kind as u16, angle, 0.).unwrap();
            seed = ((u64::from(seed) * 16807) % 2147483647) as u32;
        }
        assert_eq!(w.players[&id].character.mp, mp - item.mana_cost);
        assert_eq!(w.players[&id].seed, seed);
        assert_eq!(w.bullets.len(), item.shots as usize);
        assert!(
            w.bullets
                .iter()
                .all(|b| (item.projectiles[&0].min..=item.projectiles[&0].max).contains(&b.damage))
        );
        assert!(shot(&mut w, id, 20000, kind as u16, 0., 0.).is_err());
        shot(&mut w, id, 20000, 0xa00, 0., 0.).unwrap();
    }
}
#[test]
fn first_class_ability_is_ready_at_world_zero_but_same_tick_repeat_is_not() {
    for class in [0x300, 0x31d, 0x31e, 0x31f] {
        let (mut w, id) = fixture(class);
        w.now = 0;
        let mp = w.players[&id].character.mp;
        cast(&mut w, id).unwrap();
        let spent = w
            .content
            .get(w.players[&id].character.items[1] as u16)
            .unwrap()
            .mana_cost;
        assert_eq!(w.players[&id].character.mp, mp - spent);
        assert!(cast(&mut w, id).is_err());
        assert_eq!(w.players[&id].character.mp, mp - spent);
    }
}
#[test]
fn class_ability_failures_are_atomic() {
    for class in [0x300, 0x31d, 0x31e, 0x31f] {
        for cause in 0..5 {
            let (mut w, id) = fixture(class);
            match cause {
                0 => w.players.get_mut(&id).unwrap().character.mp = 0,
                1 => w
                    .apply_condition(id, crate::condition::QUIET, 1000)
                    .unwrap(),
                2 => w.players.get_mut(&id).unwrap().last_ability = Some(w.now),
                3 => w.players.get_mut(&id).unwrap().character.items[1] = -1,
                4 => w.players.get_mut(&id).unwrap().character.class = crate::content::WIZARD,
                _ => unreachable!(),
            }
            let p = &w.players[&id];
            let before = (p.character.mp, p.last_ability, p.statuses.clone());
            assert!(cast(&mut w, id).is_err());
            let p = &w.players[&id];
            assert_eq!((p.character.mp, p.last_ability, p.statuses.clone()), before);
            assert!(w.bullets.is_empty());
        }
    }
}

#[test]
fn every_tiered_cloak_helm_and_seal_charges_once_and_uses_xml_effects() {
    let content = tests::world().content;
    for item in content
        .objects
        .values()
        .filter(|o| o.tier.is_some() && matches!(o.slot_type, 12 | 13 | 16))
    {
        let class = match item.slot_type {
            12 => 0x31f,
            13 => 0x300,
            16 => 0x31d,
            _ => unreachable!(),
        };
        let (mut w, id) = fixture(class);
        w.players.get_mut(&id).unwrap().character.items[1] = item.kind as i32;
        let friend = player(&mut w, crate::content::WIZARD, 2, Pos { x: 21., y: 20. });
        let mp = w.players[&id].character.mp;
        cast(&mut w, id).unwrap();
        assert_eq!(w.players[&id].character.mp, mp - item.mana_cost);
        for ability in &item.abilities {
            let crate::content::Ability::Condition {
                effect,
                duration,
                range,
            } = ability
            else {
                panic!("tiered buff has a non-condition activation")
            };
            assert_eq!(w.players[&id].statuses[effect], (20000, 20000 + duration));
            assert_eq!(
                w.players[&friend].statuses.contains_key(effect),
                range.is_some()
            );
            assert_ne!(
                w.players[&id].conditions(20000 + duration - 1) & crate::condition::bit(*effect),
                0
            );
            assert_eq!(
                w.players[&id].conditions(20000 + duration) & crate::condition::bit(*effect),
                0
            );
        }
        assert!(cast(&mut w, id).is_err());
        assert_eq!(w.players[&id].character.mp, mp - item.mana_cost);
    }
}
#[test]
fn berserk_native_cadence_survives_expiry_without_dropping_draws_or_disconnect() {
    let (mut w, id) = fixture(0x31d);
    cast(&mut w, id).unwrap();
    let period =
        reconciliation::attack_cooldown(&w.players[&id], &w.content, w.now, 1.).unwrap() as i32;
    shot(&mut w, id, 20000, 0xa00, 0., 0.).unwrap();
    shot(&mut w, id, 20000 + period, 0xa00, 0., 0.).unwrap();
    assert_eq!(w.players[&id].character.shots, 2);
    w.now = 23000;
    shot(&mut w, id, 20000 + period * 2, 0xa00, 0., 0.).unwrap();
    assert_eq!(w.players[&id].character.shots, 3);
    w.now = 24001;
    let normal =
        reconciliation::attack_cooldown(&w.players[&id], &w.content, w.now, 1.).unwrap() as i32;
    shot(&mut w, id, 20000 + period * 2 + normal, 0xa00, 0., 0.).unwrap();
    assert_eq!(w.players[&id].character.shots, 4);
}
#[test]
fn shield_stuns_on_contact_then_firing_resumes_and_immunity_is_respected() {
    for immune in [false, true] {
        let (mut w, id) = fixture(0x31e);
        let enemy = w
            .spawn_entity(0x650, Pos { x: 22., y: 20. }, vec![])
            .unwrap();
        if immune {
            w.entities.get_mut(&enemy).unwrap().condition = crate::condition::bit(12);
        }
        cast(&mut w, id).unwrap();
        shot(&mut w, id, 20000, 0xa08, 0., 0.).unwrap();
        for _ in 0..5 {
            w.step();
        }
        assert!(w.entities[&enemy].hp < 900);
        assert_eq!(w.entities[&enemy].conditions(w.now) & STUNNED != 0, !immune);
        if !immune {
            assert!(w.npc_step().is_empty());
            w.now += 3000;
            assert!(!w.npc_step().is_empty());
        }
    }
}
#[test]
fn rejected_shield_predictions_drain_entire_volley_and_keep_next_weapon_aligned() {
    for quiet in [false, true] {
        let (mut w, id) = fixture(0x31e);
        w.players.get_mut(&id).unwrap().character.items[1] = 0xb22;
        if quiet {
            w.apply_condition(id, crate::condition::QUIET, 1000)
                .unwrap();
        } else {
            w.players.get_mut(&id).unwrap().character.mp = 0;
        }
        assert!(cast(&mut w, id).is_err());
        let first = w.players[&id].next_bullet;
        for i in 0..5 {
            shot(
                &mut w,
                id,
                20000,
                0xb22,
                (i as f32 - 2.) * 11.25_f32.to_radians(),
                0.,
            )
            .unwrap();
        }
        assert_eq!(w.players[&id].next_bullet, first + 5);
        assert_eq!(w.players[&id].character.shots, 0);
        assert!(w.bullets.is_empty());
        shot(&mut w, id, 20000, 0xa00, 0., 0.).unwrap();
        assert_eq!(w.players[&id].character.shots, 1);
    }
}
