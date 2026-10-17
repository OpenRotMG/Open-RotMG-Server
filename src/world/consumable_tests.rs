use super::{tests, *};
use crate::store::{InventoryWrite, Store};

#[test]
fn selected_dyes_and_cloths_change_only_their_own_channel_after_one_commit() {
    let mut w = tests::world();
    let id = w.enter(tests::ch(&w), None).unwrap();
    let dyes: Vec<_> = w
        .content
        .objects
        .values()
        .filter(|item| item.class == "Dye")
        .map(|item| (item.kind, item.textures))
        .collect();
    assert_eq!(dyes.len(), 424);
    for (kind, textures) in dyes {
        let p = w.players.get_mut(&id).unwrap();
        p.character.items[5] = i32::from(kind);
        let before = p.character.clone();
        let mut expected = [before.tex1, before.tex2];
        for i in 0..2 {
            if let Some(value) = textures[i] {
                expected[i] = value;
            }
        }
        let pos = p.pos;
        let plan = w
            .use_item(
                id,
                Slot {
                    object: id,
                    slot: 5,
                    kind: i32::from(kind),
                },
                pos,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            [w.players[&id].character.tex1, w.players[&id].character.tex2],
            [before.tex1, before.tex2]
        );
        assert_eq!(w.players[&id].character.items[5], i32::from(kind));
        w.finish_inventory(plan, Ok(before.revision + 1)).unwrap();
        let after = &w.players[&id].character;
        assert_eq!([after.tex1, after.tex2], expected);
        assert_eq!(after.items[5], -1);
        assert_eq!(after.base, before.base);
    }
}

#[tokio::test]
async fn dye_consumption_and_portrait_persist_together_and_failed_writes_publish_neither() {
    for fail in [false, true] {
        let mut w = tests::world();
        let path =
            std::env::temp_dir().join(format!("openrotmg-dye-{:016x}.db", rand::random::<u64>()));
        let store = Store::open(&path, &w.content).unwrap();
        let account = store.list("dye-fixture".into()).await.unwrap().id;
        let mut ch = store
            .create(account, w.content.get(0x030e).unwrap())
            .await
            .unwrap();
        ch.items[5] = 0x1175;
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                None,
            )
            .await
            .unwrap()[0];
        let character = ch.id;
        let id = w.enter(ch, None).unwrap();
        let plan = w
            .use_item(
                id,
                Slot {
                    object: id,
                    slot: 5,
                    kind: 0x1175,
                },
                w.players[&id].pos,
            )
            .unwrap()
            .unwrap();
        let (_, tex1, tex2) = plan.appearance.unwrap();
        assert_ne!(tex2, 0);
        if fail {
            rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER fail_dye BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'injected dye save failure'); END;").unwrap();
        }
        let result = store
            .consume_dye(
                InventoryWrite {
                    id: character,
                    account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                },
                5,
                0x1175,
                [tex1, tex2],
            )
            .await;
        assert_eq!(result.is_err(), fail);
        assert_eq!(w.finish_inventory(plan, result).is_err(), fail);
        drop(store);
        let restarted = Store::open(&path, &w.content).unwrap();
        let saved = restarted.load(account, character).await.unwrap();
        if fail {
            assert_eq!(saved.items[5], 0x1175);
            assert_eq!([saved.tex1, saved.tex2], [0, 0]);
            assert_eq!(w.players[&id].character.tex2, 0);
            assert!(w.players[&id].persistence_failure.is_some());
        } else {
            assert_eq!(saved.items[5], -1);
            assert_eq!([saved.tex1, saved.tex2], [tex1, tex2]);
            let listed = restarted.list("dye-fixture".into()).await.unwrap();
            assert!(listed.xml("").contains(&format!("<Tex2>{tex2}</Tex2>")));
            assert!(w.players[&id].persistence_failure.is_none());
        }
    }
}

fn staged(w: &mut World, id: i32) -> InventoryPlan {
    let kind = w.players[&id].character.items[5];
    w.use_item(
        id,
        Slot {
            object: id,
            slot: 5,
            kind,
        },
        w.players[&id].pos,
    )
    .unwrap()
    .unwrap()
}
fn finish(w: &mut World, plan: InventoryPlan) {
    let revision = plan.revision + 1;
    w.finish_inventory(plan, Ok(revision)).unwrap();
}

#[test]
fn compound_consumables_execute_every_effect_in_xml_order_after_commit() {
    let mut w = tests::world();
    let id = w.enter(tests::ch(&w), None).unwrap();
    let p = w.players.get_mut(&id).unwrap();
    p.character.hp = 10;
    p.character.mp = 10;
    p.character.items[5] = 0xc12;
    let plan = staged(&mut w, id);
    assert_eq!(
        (w.players[&id].character.hp, w.players[&id].character.mp),
        (10, 10)
    );
    finish(&mut w, plan);
    let values = w.players[&id].effective(&w.content, w.now).unwrap();
    assert_eq!(
        (w.players[&id].character.hp, w.players[&id].character.mp),
        (values[0], values[1])
    );
    for effect in [
        crate::condition::SICK,
        crate::condition::WEAK,
        crate::condition::PARALYZED,
        15,
        28,
    ] {
        w.apply_condition(id, effect, 10000).unwrap();
    }
    w.players.get_mut(&id).unwrap().character.hp = 10;
    w.players.get_mut(&id).unwrap().character.items[5] = 0xc21;
    let plan = staged(&mut w, id);
    finish(&mut w, plan);
    assert_eq!(w.players[&id].conditions(w.now), 0);
    assert_eq!(w.players[&id].character.hp, 10);
    w.apply_condition(id, 15, 10000).unwrap();
    w.players.get_mut(&id).unwrap().character.items[5] = 0xc21;
    let plan = staged(&mut w, id);
    finish(&mut w, plan);
    assert_ne!(
        w.players[&id].conditions(w.now) & crate::condition::bit(15),
        0
    );
    for (kind, effect) in [(0xa50, 10), (0xa51, 9)] {
        w.players.get_mut(&id).unwrap().character.items[5] = kind;
        let plan = staged(&mut w, id);
        finish(&mut w, plan);
        assert_ne!(
            w.players[&id].conditions(w.now) & crate::condition::bit(effect),
            0
        );
    }
    w.now += 30000;
    assert_eq!(w.players[&id].conditions(w.now), 0);
}

#[test]
fn all_five_elixir_doses_replace_the_same_slot_until_the_last_dose() {
    let mut w = tests::world();
    let id = w.enter(tests::ch(&w), None).unwrap();
    w.players.get_mut(&id).unwrap().character.items[5] = 0xb16;
    for remaining in (1..=5).rev() {
        let kind = w.players[&id].character.items[5];
        let item = w.content.get(kind as u16).unwrap();
        assert_eq!(item.doses, Some(remaining));
        let next = item.successor.as_ref().map(|name| {
            w.content
                .objects
                .values()
                .find(|o| &o.name == name)
                .unwrap()
                .kind
        });
        w.players.get_mut(&id).unwrap().character.hp = 1;
        let plan = staged(&mut w, id);
        assert_eq!(w.players[&id].character.items[5], kind);
        finish(&mut w, plan);
        assert_eq!(
            w.players[&id].character.hp,
            w.players[&id].effective(&w.content, w.now).unwrap()[0]
        );
        assert_eq!(
            w.players[&id].character.items[5],
            next.map_or(-1, i32::from)
        );
    }
}

#[test]
fn boosts_are_reserved_invisibly_publish_together_expire_and_rollback_without_stat_gain() {
    for outcome in ["success", "write_error", "wrong_revision"] {
        let mut w = tests::world();
        let a = w.enter(tests::ch(&w), None).unwrap();
        let mut ch = tests::ch(&w);
        ch.id = 2;
        ch.account = 2;
        let b = w.enter(ch, None).unwrap();
        let mut ch = tests::ch(&w);
        ch.id = 3;
        ch.account = 3;
        let distant = w.enter(ch, None).unwrap();
        w.players.get_mut(&distant).unwrap().pos.x += 20.;
        w.players.get_mut(&a).unwrap().character.items[5] = 0xb11;
        let before = w.players[&a].effective(&w.content, w.now).unwrap();
        let plan = staged(&mut w, a);
        for id in [a, b, distant] {
            assert_eq!(w.players[&id].effective(&w.content, w.now).unwrap(), before);
        }
        if outcome != "success" {
            let result = if outcome == "write_error" {
                Err(anyhow::anyhow!("injected boost save failure"))
            } else {
                Ok(plan.revision + 2)
            };
            assert!(w.finish_inventory(plan, result).is_err());
            assert!(w.players[&a].persistence_failure.is_some());
            for id in [a, b] {
                assert_eq!(w.players[&id].effective(&w.content, w.now).unwrap(), before);
                assert!(w.players[&id].boosts.is_empty());
            }
            assert_eq!(w.players[&a].character.items[5], 0xb11);
        } else {
            w.now += 40000;
            finish(&mut w, plan);
            for id in [a, b] {
                assert_eq!(
                    w.players[&id].effective(&w.content, w.now).unwrap()[0],
                    before[0] + 150
                );
            }
            assert_eq!(
                w.players[&distant].effective(&w.content, w.now).unwrap(),
                before
            );
            w.players.get_mut(&a).unwrap().character.hp = before[0] + 150;
            w.now += 30000;
            w.expire_consumable_boosts();
            assert_eq!(w.players[&a].character.hp, before[0]);
            assert_eq!(w.players[&a].effective(&w.content, w.now).unwrap(), before);
            assert!(w.players[&a].boosts.is_empty());
        }
    }
}

#[test]
fn invalid_compound_effects_and_full_boost_budgets_reject_before_consumption_or_reservation() {
    let mut w = tests::world();
    let id = w.enter(tests::ch(&w), None).unwrap();
    for _ in 0..128 {
        w.players.get_mut(&id).unwrap().character.items[5] = 0xb0d;
        let plan = staged(&mut w, id);
        finish(&mut w, plan);
    }
    w.players.get_mut(&id).unwrap().character.items[5] = 0xb0d;
    assert!(
        w.use_item(
            id,
            Slot {
                object: id,
                slot: 5,
                kind: 0xb0d
            },
            w.players[&id].pos
        )
        .is_err()
    );
    assert!(!w.players[&id].pending);
    assert_eq!(w.players[&id].character.items[5], 0xb0d);
    w.now += 30000;
    w.expire_consumable_boosts();
    let item = Arc::make_mut(&mut w.content)
        .objects
        .get_mut(&0xb0d)
        .unwrap();
    item.consumable_effects
        .push(crate::content::ConsumableEffect::Unsupported(
            "invalid".into(),
        ));
    item.activations += 1;
    assert!(
        w.use_item(
            id,
            Slot {
                object: id,
                slot: 5,
                kind: 0xb0d
            },
            w.players[&id].pos
        )
        .err()
        .unwrap()
        .to_string()
        .contains("invalid")
    );
    assert!(w.players[&id].boosts.is_empty());
    assert!(!w.players[&id].pending);
}

#[tokio::test]
async fn vault_life_and_mana_clamp_atomically_without_consuming_a_backpack_slot() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for (kind, index) in [(0xae9, 0), (0xaea, 1)] {
        let store = Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let account = store.list("vault-cap-fixture".into()).await.unwrap().id;
        let mut ch = store
            .create(account, content.get(0x030e).unwrap())
            .await
            .unwrap();
        let cap = content.classes[&ch.class].caps[index];
        ch.base[index] = cap - 3;
        store.progress(ch.clone()).await.unwrap();
        let mut vault = store.vault(account).await.unwrap();
        vault.items[0] = kind;
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                Some(vault),
            )
            .await
            .unwrap()[0];
        let before = store.vault(account).await.unwrap();
        let mut consumed = before.clone();
        consumed.items[0] = -1;
        let mut stale = consumed.clone();
        stale.revision += 1;
        assert!(
            store
                .consume_stat(ch.clone(), None, index, 3, Some(stale))
                .await
                .is_err()
        );
        assert_eq!(
            store.load(account, ch.id).await.unwrap().base[index],
            cap - 3
        );
        assert_eq!(store.vault(account).await.unwrap().items, before.items);
        let revision = store
            .consume_stat(ch.clone(), None, index, 3, Some(consumed.clone()))
            .await
            .unwrap();
        let saved = store.load(account, ch.id).await.unwrap();
        assert_eq!(saved.base[index], cap);
        assert_eq!(saved.items, ch.items);
        assert_eq!(
            (saved.hp, saved.mp, saved.revision),
            (ch.hp, ch.mp, revision)
        );
        let vault = store.vault(account).await.unwrap();
        assert_eq!((vault.items[0], vault.revision), (-1, before.revision + 1));
        assert!(
            store
                .consume_stat(ch.clone(), None, index, 3, Some(consumed))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn life_and_mana_potions_commit_five_or_the_remaining_cap_without_extra_healing() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    for (kind, index) in [(0xae9, 0), (0xaea, 1)] {
        let store = Store::open(std::path::Path::new(":memory:"), &content).unwrap();
        let account = store.list("prepared-cap-fixture".into()).await.unwrap().id;
        let mut ch = store
            .create(account, content.get(0x030e).unwrap())
            .await
            .unwrap();
        let cap = content.classes[&ch.class].caps[index];
        ch.base[index] = cap - 3;
        ch.hp = 10;
        ch.mp = 10;
        store.progress(ch.clone()).await.unwrap();
        ch.items[5] = kind;
        ch.revision = store
            .exchange(
                vec![InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                None,
            )
            .await
            .unwrap()[0];
        let mut w = tests::world();
        let id = w.enter(ch, None).unwrap();
        let plan = staged(&mut w, id);
        assert_eq!(plan.stat_growth.as_ref().unwrap().3, 3);
        let (snapshot, slot, stat, amount) = plan.stat_growth.clone().unwrap();
        let result = store
            .consume_stat(snapshot.clone(), Some(slot.slot), stat, amount, None)
            .await;
        w.finish_inventory(plan, result).unwrap();
        assert_eq!(w.players[&id].character.base[index], cap);
        assert_eq!(
            (w.players[&id].character.hp, w.players[&id].character.mp),
            (10, 10)
        );
        assert!(
            store
                .consume_stat(snapshot, Some(slot.slot), stat, amount, None)
                .await
                .is_err()
        );
        let saved = store
            .load(account, w.players[&id].character.id)
            .await
            .unwrap();
        assert_eq!(saved.base[index], cap);
        assert_eq!(saved.items[5], -1);
    }
}
