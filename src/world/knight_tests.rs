use super::*;
use crate::condition;

fn fixture(shield: u16) -> (World, i32) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
        width: 16,
        height: 16,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            256
        ],
        spawns: vec![(8.5, 8.5)],
    };
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.clock.set_paused(true);
    let mut ch = Character::fresh(w.content.get(0x31e).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.base[1] = 252;
    ch.base[2] = 50;
    ch.mp = 252;
    ch.items[1] = shield.into();
    let id = w.enter(ch, None).unwrap();
    (w, id)
}

fn cast(w: &mut World, id: i32) -> Result<()> {
    let kind = w.players[&id].character.items[1];
    w.use_item(
        id,
        Slot {
            object: id,
            slot: 1,
            kind,
        },
        Pos { x: 11.5, y: 8.5 },
    )
    .map(|_| ())
}

#[tokio::test]
async fn knight_unlock_requires_warrior_twenty_after_committed_death() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for level in [19, 20] {
        let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let guid = format!("knight-unlock-{level}");
        let account = store.list(guid.clone()).await.unwrap();
        let knight = content.get(0x31e).unwrap();
        assert!(store.create(account.id, knight).await.is_err());
        for (class, target) in [
            (0x30e, 5),
            (0x310, 5),
            (0x307, 5),
            (0x300, 5),
            (0x31d, level),
        ] {
            let mut ch = store
                .create(account.id, content.get(class).unwrap())
                .await
                .unwrap();
            ch.gain_experience(
                &content,
                crate::store::level_start(target).unwrap(),
                &mut rand::thread_rng(),
            )
            .unwrap();
            assert_eq!(ch.level, target);
            store.progress(ch.clone()).await.unwrap();
            ch.hp = 0;
            ch.alive = false;
            store
                .death(ch, "prepared unlock test".into())
                .await
                .unwrap();
        }
        let listed = store.list(guid.clone()).await.unwrap();
        assert_eq!(listed.best_levels[&0x31d], level);
        assert!(listed.characters.is_empty());
        let created = store.create(account.id, knight).await;
        if level == 19 {
            assert!(created.is_err());
        } else {
            let ch = created.unwrap();
            assert_eq!(ch.level, 1);
            assert_eq!(ch.items, knight.equipment);
            assert_eq!(ch.base, [200, 100, 15, 0, 7, 10, 10, 10]);
            assert_eq!(&knight.slots[..4], &[1, 5, 7, 9]);
            assert_eq!(store.list(guid).await.unwrap().characters[0].id, ch.id);
        }
    }
}

#[test]
fn knight_shields_spend_once_and_keep_selected_projectile_parameters() {
    // Per-pellet defense, not one deduction per volley: https://web.archive.org/web/20120603070630id_/http://forums.wildshadow.com/wiki/realm-mad-god/shields
    for (kind, cost, shots, low, high, effect, duration) in [
        (0xa08, 80, 1, 55, 90, 7, 3000),
        (0xa09, 85, 2, 100, 140, 7, 3000),
        (0xa0a, 90, 3, 150, 190, 7, 3000),
        (0xacf, 95, 3, 190, 240, 7, 3000),
        (0xa0b, 100, 4, 230, 280, 7, 3000),
        (0xa0c, 100, 5, 270, 330, 7, 3000),
        (0xb22, 100, 5, 300, 360, 7, 3000),
        (0xa40, 100, 4, 250, 300, 7, 3000),
        (0xc0f, 100, 5, 300, 400, 27, 4000),
    ] {
        let (mut w, id) = fixture(kind);
        cast(&mut w, id).unwrap();
        assert_eq!(w.players[&id].character.mp, 252 - cost);
        assert_eq!(w.bullets.len(), shots);
        assert!(
            cast(&mut w, id)
                .unwrap_err()
                .to_string()
                .contains("cooldown")
        );
        assert_eq!(w.players[&id].character.mp, 252 - cost);
        assert_eq!(w.bullets.len(), shots);
        let target = w
            .spawn_entity(0x655, Pos { x: 12.5, y: 8.5 }, vec![])
            .unwrap();
        w.entities.get_mut(&target).unwrap().hp = 10000;
        let defense = w.content.objects[&0x655].defense;
        let bullets = w.bullets.clone();
        let mut total = 0;
        for b in bullets {
            assert!((low..=high).contains(&b.damage));
            assert_eq!((b.spec.min, b.spec.max), (low, high));
            assert_eq!(b.spec.effects, [(effect, duration)]);
            assert!(b.spec.multi_hit && b.spec.passes_cover);
            assert_eq!((b.spec.speed, b.spec.lifetime), (160., 200.));
            let effective_defense = if effect == condition::ARMOR_BROKEN && total > 0 {
                0
            } else {
                defense
            };
            let dealt = (b.damage - effective_defense).max(b.damage * 15 / 100);
            assert_eq!(
                w.damage_enemy(id, target, b.damage, false, &b.spec.effects, Some(b.id))
                    .unwrap(),
                dealt
            );
            total += dealt;
        }
        assert_eq!(w.entities[&target].hp, 10000 - total);
        assert_eq!(w.entities[&target].damage[&1], total);
        assert_eq!(
            w.entities[&target].statuses[&effect],
            (w.now, w.now + duration)
        );
        w.players.get_mut(&id).unwrap().ability_ready_ms = 0;
        w.players.get_mut(&id).unwrap().character.mp = cost - 1;
        assert!(cast(&mut w, id).unwrap_err().to_string().contains("mana"));
        assert_eq!(w.players[&id].character.mp, cost - 1);
        assert_eq!(w.bullets.len(), shots);
    }
}

#[test]
fn knight_piercing_shield_hits_each_target_once_and_respects_condition_gates() {
    let (mut w, id) = fixture(0xa08);
    let targets = [9., 10.].map(|x| {
        let target = w.spawn_entity(0x655, Pos { x, y: 8.5 }, vec![]).unwrap();
        let e = w.entities.get_mut(&target).unwrap();
        e.hp = 10000;
        e.condition = condition::bit(condition::PARALYZED) | condition::bit(condition::STUNNED);
        target
    });
    cast(&mut w, id).unwrap();
    let raw = w.bullets[0].damage;
    for _ in 0..6 {
        w.step();
    }
    for target in targets {
        let expected = (raw - w.content.objects[&0x655].defense).max(raw * 15 / 100);
        assert_eq!(w.entities[&target].hp, 10000 - expected);
        assert_eq!(w.entities[&target].damage[&1], expected);
    }
    assert!(w.bullets.is_empty());
    assert_eq!(w.players[&id].character.lifetime.shots, 1);
    assert_eq!(w.players[&id].character.lifetime.projectile_hits, 1);
    for flag in [
        0,
        12,
        condition::INVULNERABLE,
        condition::INVINCIBLE,
        condition::STASIS,
    ] {
        for effect in [condition::STUNNED, condition::ARMOR_BROKEN] {
            let target = targets[0];
            let e = w.entities.get_mut(&target).unwrap();
            e.statuses.clear();
            e.condition = if flag == 0 { 0 } else { condition::bit(flag) };
            let old = e.hp;
            let dealt = w
                .damage_enemy(id, target, 100, false, &[(effect, 3000)], None)
                .unwrap();
            assert_eq!(
                w.entities[&target].statuses.contains_key(&effect),
                match flag {
                    condition::INVINCIBLE | condition::STASIS => false,
                    12 => effect != condition::STUNNED,
                    _ => true,
                }
            );
            assert_eq!(w.entities[&target].hp, old - dealt);
            assert_eq!(
                dealt == 0,
                [
                    condition::INVULNERABLE,
                    condition::INVINCIBLE,
                    condition::STASIS
                ]
                .contains(&flag)
            );
        }
    }
}
