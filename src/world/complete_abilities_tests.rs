use super::{tests, *};

fn fixture(class: u16, ability: i32) -> (World, i32) {
    let mut w = tests::world();
    w.encounter = true;
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    for cell in &mut Arc::make_mut(&mut w.map).cells {
        cell.ground = 0x61;
    }
    let mut ch = tests::ch(&w);
    ch.class = class;
    ch.base = w.content.classes[&class].caps;
    ch.hp = ch.base[0];
    ch.mp = ch.base[1];
    ch.items = w.content.get(class).unwrap().equipment.clone();
    ch.items[1] = ability;
    let id = w
        .enter_at(
            ch,
            None,
            Some(Pos { x: 12., y: 12. }),
            AccountPresentation {
                credits: 100,
                fame: 0,
                best_fame: BTreeMap::new(),
            },
        )
        .unwrap();
    w.players.get_mut(&id).unwrap().controller = Controller::Human;
    w.now = 20000;
    (w, id)
}
fn cast(w: &mut World, id: i32, target: Pos) -> Result<Option<InventoryPlan>> {
    w.native_use_item(
        id,
        w.now as i32,
        Slot {
            object: id,
            slot: 1,
            kind: w.players[&id].character.items[1],
        },
        target,
    )
}
fn enemy(w: &mut World, x: f32, y: f32) -> i32 {
    w.spawn_entity(0x650, Pos { x, y }, vec![]).unwrap()
}

#[test]
fn every_recovered_class_ability_item_executes_all_activations_for_one_mana_charge() {
    let content = tests::world().content;
    let items: Vec<_> = content
        .objects
        .values()
        .filter(|o| {
            !o.activate.is_empty()
                && content
                    .classes
                    .keys()
                    .any(|c| content.objects[c].slots[1] == o.slot_type)
        })
        .cloned()
        .collect();
    assert_eq!(items.len(), 108);
    for item in items {
        let class = *content
            .classes
            .keys()
            .find(|c| content.objects[c].slots[1] == item.slot_type)
            .unwrap();
        let (mut w, id) = fixture(class, item.kind as i32);
        enemy(&mut w, 14., 12.);
        let before = w.players[&id].character.mp;
        cast(&mut w, id, Pos { x: 14., y: 12. })
            .unwrap_or_else(|e| panic!("{} ({:#x}): {e:#}", item.name, item.kind));
        assert_eq!(
            w.players[&id].character.mp,
            before - item.mana_cost,
            "{}",
            item.name
        );
    }
}
#[test]
fn poison_lands_after_throw_and_deals_total_damage_over_time_with_defense_once() {
    let (mut w, id) = fixture(0x320, 0xaa3);
    let target = enemy(&mut w, 14., 12.);
    w.entities.get_mut(&target).unwrap().condition = PARALYZED;
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    for _ in 0..29 {
        w.step();
    }
    assert_eq!(w.entities[&target].hp, 900);
    for _ in 0..110 {
        w.step();
    }
    assert_eq!(w.entities[&target].hp, 900 - (150 - 12));
}
#[test]
fn skull_drains_actual_enemy_health_and_respects_sick() {
    for sick in [false, true] {
        let (mut w, id) = fixture(0x321, 0xaaa);
        let target = enemy(&mut w, 14., 12.);
        w.players.get_mut(&id).unwrap().character.hp = 100;
        if sick {
            w.apply_condition(id, crate::condition::SICK, 1000).unwrap();
        }
        cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
        assert_eq!(w.entities[&target].hp, 887);
        assert_eq!(w.players[&id].character.hp, if sick { 100 } else { 113 });
    }
}
#[test]
fn stasis_freezes_and_protects_then_grants_temporary_immunity() {
    let (mut w, id) = fixture(0x323, 0xae4);
    let target = enemy(&mut w, 14., 12.);
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    assert_ne!(
        w.entities[&target].conditions(w.now) & crate::condition::bit(22),
        0
    );
    let held = w.entities[&target].pos;
    for _ in 0..20 {
        w.step();
    }
    assert_eq!(w.entities[&target].pos.distance(held), 0.);
    assert_eq!(
        crate::condition::damage(100, 12, w.entities[&target].conditions(w.now), false),
        0
    );
    w.now = 23000;
    assert_eq!(
        w.entities[&target].conditions(w.now) & crate::condition::bit(22),
        0
    );
    assert_ne!(
        w.entities[&target].conditions(w.now) & crate::condition::bit(23),
        0
    );
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    assert_eq!(
        w.entities[&target].conditions(w.now) & crate::condition::bit(22),
        0
    );
    w.now = 26000;
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    assert_ne!(
        w.entities[&target].conditions(w.now) & crate::condition::bit(22),
        0
    );
}
#[test]
fn trickster_leaves_decoy_before_teleport_and_decoy_draws_enemy_targeting() {
    let (mut w, id) = fixture(0x324, 0xae3);
    let target = enemy(&mut w, 10., 12.);
    w.players.get_mut(&id).unwrap().last_motion = Some((std::f32::consts::PI, w.now));
    cast(&mut w, id, Pos { x: 20., y: 12. }).unwrap();
    assert_eq!(w.players[&id].pos.distance(Pos { x: 20., y: 12. }), 0.);
    let decoy = w.entities.values().find(|e| e.kind == 0x715).unwrap().id;
    assert!(w.entities[&decoy].pos.x < 14.);
    for _ in 0..20 {
        w.step();
    }
    assert!(w.entities[&target].pos.x < 10.);
    for _ in 0..61 {
        w.step();
    }
    assert!(!w.entities.contains_key(&decoy));
}
#[test]
fn godlands_predictive_shooters_accept_live_decoys_without_inventing_player_motion() {
    let (mut w, id) = fixture(0x324, 0xae3);
    let god = w
        .spawn_entity(0x651, Pos { x: 10., y: 12. }, vec![])
        .unwrap();
    w.players.get_mut(&id).unwrap().last_motion = Some((std::f32::consts::PI, w.now));
    cast(&mut w, id, Pos { x: 20., y: 12. }).unwrap();
    for _ in 0..30 {
        w.step();
    }
    assert!(
        w.bullets
            .iter()
            .any(|b| b.owner == god && b.angle.cos() < 0.)
    );
    assert!(w.god_shots(god, Pos { x: 90., y: 90. }, 0., 0).is_err());
}

#[test]
fn scepter_chain_is_bounded_unique_and_special_variant_slows() {
    for item in [0xae5, 0xc30] {
        let (mut w, id) = fixture(0x325, item);
        let enemies: Vec<_> = (0..8).map(|i| enemy(&mut w, 14. + i as f32, 12.)).collect();
        cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
        let damaged: Vec<_> = enemies.iter().filter(|e| w.entities[e].hp < 900).collect();
        assert_eq!(damaged.len(), if item == 0xae5 { 3 } else { 7 });
        if item == 0xc30 {
            assert!(
                damaged
                    .iter()
                    .all(|e| w.entities[e].conditions(w.now) & SLOWED != 0)
            );
        }
    }
}

#[test]
fn huntress_trap_lands_triggers_once_slows_then_expires() {
    let (mut w, id) = fixture(0x322, 0xab1);
    let target = enemy(&mut w, 14., 12.);
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    for _ in 0..29 {
        w.step();
    }
    assert_eq!(w.entities[&target].hp, 900);
    w.step();
    assert_eq!(w.entities[&target].hp, 862);
    assert_ne!(w.entities[&target].conditions(w.now) & SLOWED, 0);
    for _ in 0..60 {
        w.step();
    }
    assert_eq!(w.entities[&target].hp, 862);
    assert_eq!(w.entities[&target].conditions(w.now) & SLOWED, 0);
    assert!(w.entities.values().all(|e| e.kind != 0x711));
}
#[test]
fn special_native_volleys_preserve_damage_and_all_predicted_projectiles() {
    for item in [0xc07, 0xc0f, 0xa40] {
        let class = if item == 0xc07 {
            crate::content::ARCHER
        } else {
            crate::content::KNIGHT
        };
        let (mut w, id) = fixture(class, item);
        let object = w.content.get(item as u16).unwrap().clone();
        cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
        for i in 0..object.shots {
            let angle = (i as f32 - (object.shots - 1) as f32 / 2.) * object.arc.to_radians();
            let pos = w.players[&id].pos;
            w.native_shoot_received(
                id,
                (w.now as i32, Instant::now()),
                w.players[&id].next_bullet,
                item as u16,
                Pos {
                    x: pos.x + 0.3 * angle.cos(),
                    y: pos.y + 0.3 * angle.sin(),
                },
                angle,
            )
            .unwrap();
        }
        assert_eq!(w.bullets.len(), object.shots as usize);
        assert!(w.bullets.iter().all(|b| {
            (object.projectiles[&0].min..=object.projectiles[&0].max).contains(&b.damage)
                && b.spec.effects == object.projectiles[&0].effects
        }));
    }
}
#[test]
fn dazed_reduces_enemy_volley_and_stasis_immune_definitions_cannot_be_frozen() {
    for dazed in [false, true] {
        let (mut w, id) = fixture(crate::content::ARCHER, 0xc07);
        let target = enemy(&mut w, 14., 12.);
        if dazed {
            w.apply_condition(target, crate::condition::DAZED, 3000)
                .unwrap();
        }
        w.npc_step();
        assert_eq!(w.bullets.len(), if dazed { 2 } else { 5 });
        let _ = id;
    }
    let (mut w, id) = fixture(0x323, 0xae4);
    let kind = w
        .content
        .objects
        .values()
        .find(|o| o.enemy && o.stasis_immune)
        .unwrap()
        .kind;
    let target = w
        .spawn_entity(kind, Pos { x: 14., y: 12. }, vec![])
        .unwrap();
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    assert_eq!(
        w.entities[&target].conditions(w.now) & crate::condition::bit(crate::condition::STASIS),
        0
    );
}
#[test]
fn juggernaut_and_blasphemous_seal_apply_every_distinct_effect() {
    for (class, item) in [
        (crate::content::WARRIOR, 0xc08),
        (crate::content::PALADIN, 0xc06),
    ] {
        let (mut w, id) = fixture(class, item);
        cast(&mut w, id, Pos { x: 12., y: 12. }).unwrap();
        let flags = w.players[&id].conditions(w.now);
        if item == 0xc08 {
            assert_ne!(flags & crate::condition::bit(crate::condition::BERSERK), 0);
            assert_ne!(flags & crate::condition::bit(crate::condition::ARMORED), 0);
            assert_eq!(flags & crate::condition::bit(15), 0);
        } else {
            assert_ne!(flags & crate::condition::bit(crate::condition::HEALING), 0);
            assert_ne!(flags & crate::condition::bit(crate::condition::DAMAGING), 0);
            assert_eq!(crate::condition::damage(100, 0, flags, false), 0);
            w.now += 1400;
            assert!(crate::condition::damage(100, 0, w.players[&id].conditions(w.now), false) > 0);
        }
    }
}
#[test]
fn dancing_prism_has_opposing_decoys_without_teleport_and_departure_cleans_up() {
    let (mut w, id) = fixture(0x324, 0xc0d);
    let pos = w.players[&id].pos;
    w.players.get_mut(&id).unwrap().last_motion = Some((0., w.now));
    cast(&mut w, id, Pos { x: 18., y: 12. }).unwrap();
    assert_eq!(w.players[&id].pos.distance(pos), 0.);
    w.step();
    let decoys: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == 0x715)
        .map(|e| e.pos)
        .collect();
    assert_eq!(decoys.len(), 2);
    assert!(decoys.iter().any(|p| p.x > pos.x) && decoys.iter().any(|p| p.x < pos.x));
    w.leave(id).unwrap();
    assert!(w.entities.values().all(|e| e.kind != 0x715));
}
#[test]
fn ghostly_prism_uses_preserved_mana_teleports_and_explodes_without_luring() {
    let (mut w, id) = fixture(0x324, 0xc2a);
    let target = enemy(&mut w, 14., 12.);
    w.entities.get_mut(&target).unwrap().condition = PARALYZED;
    let mp = w.players[&id].character.mp;
    cast(&mut w, id, Pos { x: 20., y: 12. }).unwrap();
    assert_eq!(w.players[&id].character.mp, mp - 90);
    assert_eq!(w.decoy_positions().count(), 0);
    assert!(w.entities.values().any(|e| e.kind == 0x1409));
    for _ in 0..59 {
        w.step();
    }
    assert_eq!(w.entities[&target].hp, 900);
    w.step();
    assert_eq!(w.entities[&target].hp, 612);
    assert!(w.entities.values().all(|e| e.kind != 0x1409));
}
#[test]
fn invalid_targets_never_partially_spend_or_apply_mixed_item_effects() {
    for item in [0xa5a, 0xc2a, 0xae2] {
        let class = match item {
            0xa5a => crate::content::ROGUE,
            0xc2a => 0x324,
            _ => crate::content::PRIEST,
        };
        let (mut w, id) = fixture(class, item);
        let before = (w.players[&id].character.mp, w.players[&id].pos);
        assert!(
            cast(
                &mut w,
                id,
                Pos {
                    x: f32::NAN,
                    y: 12.
                }
            )
            .is_err()
        );
        assert!(cast(&mut w, id, Pos { x: 99., y: 99. }).is_err());
        assert_eq!(w.players[&id].character.mp, before.0);
        assert_eq!(w.players[&id].pos.distance(before.1), 0.);
        assert_eq!(w.players[&id].conditions(w.now), 0);
        assert_eq!(w.players[&id].last_ability, None);
        assert!(w.entities.is_empty());
    }
}
#[test]
fn direct_ability_damage_uses_shared_kill_credit_without_fabricating_weapon_accuracy() {
    let (mut w, id) = fixture(0x321, 0xaaa);
    let target = enemy(&mut w, 14., 12.);
    w.entities.get_mut(&target).unwrap().hp = 10;
    w.players.get_mut(&id).unwrap().character.hp = 100;
    let result = w.damage_enemy(id, target, 25, false, &[], None).unwrap();
    assert_eq!(result, 10);
    assert!(!w.entities.contains_key(&target));
    assert_eq!(w.players[&id].character.kills, 1);
    assert!(w.players[&id].experience_queued > 0);
    assert_eq!(
        (
            w.players[&id].character.shots,
            w.players[&id].character.hits
        ),
        (0, 0)
    );
}

#[test]
fn stasis_projectiles_pass_through_and_decoys_absorb_ordinary_hostile_projectiles() {
    for friendly in [false, true] {
        let (mut w, id) = fixture(0x324, 0xae3);
        let first = enemy(&mut w, 14., 12.);
        if friendly {
            w.apply_condition(first, crate::condition::STASIS, 3000)
                .unwrap();
        } else {
            cast(&mut w, id, Pos { x: 20., y: 12. }).unwrap();
            w.now += 1500;
        }
        let second = enemy(&mut w, 16., 12.);
        w.entities.get_mut(&second).unwrap().condition = PARALYZED;
        let mut spec = w.content.objects[&0x650].projectiles[&0].clone();
        spec.speed = if friendly { 1000. } else { 0. };
        spec.multi_hit = false;
        let pos = Pos { x: 12., y: 12. };
        w.bullets.push(Bullet {
            owner: if friendly { id } else { first },
            source_kind: 0x650,
            id: 0,
            friendly,
            start: pos,
            pos,
            angle: 0.,
            born: w.now,
            damage: 50,
            spec,
            hit: BTreeSet::new(),
        });
        w.step();
        if friendly {
            assert_eq!(w.entities[&first].hp, 900);
            assert_eq!(w.entities[&second].hp, 862);
        } else {
            assert!(w.bullets.iter().all(|b| b.owner != first || b.id != 0));
        }
    }
}
#[test]
fn all_new_families_reject_quiet_mana_and_cooldown_without_side_effects() {
    for (class, item) in [
        (0x320, 0xaa3),
        (0x321, 0xaaa),
        (0x322, 0xab1),
        (0x323, 0xae4),
        (0x324, 0xae3),
        (0x325, 0xae5),
    ] {
        for denied in 0..3 {
            let (mut w, id) = fixture(class, item);
            match denied {
                0 => w
                    .apply_condition(id, crate::condition::QUIET, 1000)
                    .unwrap(),
                1 => w.players.get_mut(&id).unwrap().character.mp = 0,
                _ => w.players.get_mut(&id).unwrap().last_ability = Some(w.now),
            };
            let mp = w.players[&id].character.mp;
            assert!(cast(&mut w, id, Pos { x: 14., y: 12. }).is_err());
            assert_eq!(w.players[&id].character.mp, mp);
            assert!(w.entities.is_empty());
            assert_eq!(w.players[&id].pos.distance(Pos { x: 12., y: 12. }), 0.);
        }
    }
}

#[test]
fn custom_tomes_execute_healing_protection_and_selective_purification() {
    for item in [0xc1e, 0xc09] {
        let (mut w, id) = fixture(crate::content::PRIEST, item);
        w.players.get_mut(&id).unwrap().character.hp = 100;
        if item == 0xc09 {
            for effect in [crate::condition::SICK, 27, 28, 15, crate::condition::SLOWED] {
                w.apply_condition(id, effect, 5000).unwrap();
            }
        }
        cast(&mut w, id, Pos { x: 12., y: 12. }).unwrap();
        if item == 0xc1e {
            assert_eq!(w.players[&id].character.hp, 350);
            assert_ne!(
                w.players[&id].conditions(w.now) & crate::condition::bit(26),
                0
            );
        } else {
            assert_eq!(w.players[&id].conditions(w.now), 0);
        }
    }
}
