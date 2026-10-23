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
                guild: None,
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
fn all_death_fame_phase3_projectile_and_taxonomy() {
    let (mut objects, caster) = fixture(crate::content::WIZARD, 0xb24);
    // A prepared metadata-negative fixture exercises destructibility independently of Enemy.
    let kind = 0x650;
    Arc::make_mut(&mut objects.content)
        .objects
        .get_mut(&kind)
        .unwrap()
        .enemy = false;
    objects
        .spawn_entity(kind, Pos { x: 14., y: 12. }, vec![])
        .unwrap();
    cast(&mut objects, caster, Pos { x: 14., y: 12. }).unwrap();
    for b in &mut objects.bullets {
        b.spec.speed = 0.;
    }
    objects.step();
    assert_eq!(objects.players[&caster].character.lifetime.shots, 20);
    assert_eq!(
        objects.players[&caster].character.lifetime.projectile_hits, 0,
        "destructible objects are not enemies"
    );
    assert_eq!(objects.players[&caster].character.lifetime.kills, 0);
    assert!(
        objects.players[&caster].character.hits > 0,
        "fixture must actually collide with the destructible non-enemy"
    );
    let (mut w, id) = fixture(crate::content::WIZARD, 0xb24);
    let target = enemy(&mut w, 14., 12.);
    w.entities.get_mut(&target).unwrap().condition =
        PARALYZED | crate::condition::bit(crate::condition::INVULNERABLE);
    cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
    assert_eq!(w.players[&id].character.lifetime.shots, 20);
    for b in &mut w.bullets {
        b.spec.multi_hit = true;
        b.spec.speed = 0.;
    }
    w.step();
    assert_eq!(
        w.players[&id].character.lifetime.projectile_hits, 20,
        "zero damage still collides"
    );
    w.step();
    assert_eq!(w.players[&id].character.lifetime.projectile_hits, 20);
    assert!(w.entities.contains_key(&target));
    let second = enemy(&mut w, 14., 12.);
    w.entities.get_mut(&second).unwrap().condition =
        PARALYZED | crate::condition::bit(crate::condition::INVULNERABLE);
    w.step();
    assert_eq!(
        w.players[&id].character.lifetime.projectile_hits, 20,
        "piercing another enemy is not another projectile hit"
    );
    for _ in 0..15 {
        w.now += 1001;
        w.players.get_mut(&id).unwrap().character.mp = 385;
        cast(&mut w, id, Pos { x: 14., y: 12. }).unwrap();
        for b in &mut w.bullets {
            b.spec.multi_hit = true;
            b.spec.speed = 0.;
        }
        w.step();
    }
    assert_eq!(w.players[&id].character.lifetime.shots, 320);
    assert_eq!(
        w.players[&id].character.lifetime.projectile_hits, 320,
        "live projectile state survives >256 reused IDs"
    );
    let mut other = tests::ch(&w);
    other.id = 2;
    other.account = 2;
    let other = w.enter(other, None).unwrap();
    let before = w.players[&id].character.lifetime.clone();
    for kind in [0x651, 0x60e, 0x1740, 0x932] {
        let enemy = w
            .spawn_entity(kind, Pos { x: 14., y: 12. }, vec![])
            .unwrap();
        let hp = w.entities[&enemy].hp;
        w.damage_enemy(other, enemy, (hp - 1).max(1), true, &[], None)
            .unwrap();
        w.damage_enemy(id, enemy, hp + 100, true, &[], None)
            .unwrap();
    }
    let life = &w.players[&id].character.lifetime;
    assert_eq!(life.kills - before.kills, 4);
    assert_eq!(life.god_kills - before.god_kills, 3);
    assert_eq!(w.content.objects.values().filter(|o| o.god).count(), 54);
    assert_eq!(w.content.objects.values().filter(|o| o.cube).count(), 7);
    assert_eq!(w.content.objects.values().filter(|o| o.oryx).count(), 2);
    assert_eq!(life.oryx_kills - before.oryx_kills, 2);
    assert_eq!(life.cube_kills - before.cube_kills, 1);
    assert_eq!(
        life.projectile_hits, before.projectile_hits,
        "nonprojectile/delayed damage does not create hits"
    );
    assert_eq!(
        w.players[&other].character.lifetime.oryx_kills, 0,
        "largest damage is not final blow"
    );
    let enemy = w
        .spawn_entity(0x932, Pos { x: 14., y: 12. }, vec![])
        .unwrap();
    w.damage_enemy(id, enemy, 1, true, &[], None).unwrap();
    w.finish_timed_defeat(enemy).unwrap();
    assert_eq!(
        w.players[&id].character.lifetime.oryx_kills, 2,
        "self destruction has no player Oryx final blow"
    );
    let before = w.players[&id].character.lifetime.kills;
    let enemy = w
        .spawn_entity(0x650, Pos { x: 14., y: 12. }, vec![])
        .unwrap();
    w.remove_entity(enemy);
    assert_eq!(
        w.players[&id].character.lifetime.kills, before,
        "despawn is not death"
    );
}

#[test]
fn all_death_fame_phase3_successful_actions() {
    for failure in ["quiet", "target"] {
        let (mut invalid, id) = fixture(crate::content::WIZARD, 0xb24);
        let pos = if failure == "target" {
            Pos { x: -10., y: -10. }
        } else {
            invalid.players[&id].pos
        };
        if failure == "quiet" {
            invalid
                .apply_condition(id, crate::condition::QUIET, 1000)
                .unwrap();
        }
        assert!(cast(&mut invalid, id, pos).is_err());
        assert!(!invalid.players[&id].character.lifetime.ability_used);
        assert_eq!(invalid.players[&id].character.lifetime.shots, 0);
    }
    let (mut w, id) = fixture(crate::content::WIZARD, 0xb24);
    let pos = w.players[&id].pos;
    w.players.get_mut(&id).unwrap().character.mp = 0;
    assert!(cast(&mut w, id, pos).is_err());
    assert!(!w.players[&id].character.lifetime.ability_used);
    w.players.get_mut(&id).unwrap().character.mp = 100;
    cast(&mut w, id, pos).unwrap();
    assert!(w.players[&id].character.lifetime.ability_used);
    assert!(!w.players[&id].character.lifetime.teleported);
}
