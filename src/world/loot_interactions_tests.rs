use super::{tests, *};

#[test]
fn ground_potions_reserve_the_source_and_leave_full_backpacks_unchanged() {
    for kind in [0xa22, 0xa23, 0xa1f, 0xa3f, 0xc12, 0xae9, 0xaea, 0xb16] {
        let mut w = tests::world();
        let mut ch = tests::ch(&w);
        ch.items[4..].fill(0xa22);
        ch.hp = 20;
        ch.mp = 0;
        ch.base[2] = 10;
        let id = w.enter(ch, None).unwrap();
        let before = w.players[&id].character.clone();
        let next = match &w.content.get(kind as u16).unwrap().successor {
            Some(name) => i32::from(
                w.content
                    .objects
                    .values()
                    .find(|item| &item.name == name)
                    .unwrap()
                    .kind,
            ),
            None => -1,
        };
        let owner = w
            .content
            .get(kind as u16)
            .unwrap()
            .soulbound
            .then_some(before.account);
        let bag = w.loot_bag(w.players[&id].pos, owner, vec![kind]).unwrap();
        let slot = Slot {
            object: bag,
            slot: 0,
            kind,
        };
        let plan = w.use_item(id, slot, w.players[&id].pos).unwrap().unwrap();
        assert!(w.entities[&bag].reserved && w.players[&id].pending);
        assert_eq!(plan.items, before.items);
        assert_eq!(plan.bag.as_ref().unwrap().1[0], next);
        assert_eq!(w.entities[&bag].items[0], kind);
        assert!(w.use_item(id, slot, w.players[&id].pos).is_err());
        let mut other = tests::ch(&w);
        other.account = 2;
        let other = w.enter(other, None).unwrap();
        assert!(w.use_item(other, slot, w.players[&id].pos).is_err());
        w.finish_inventory(plan, Ok(before.revision + 1)).unwrap();
        assert_eq!(w.players[&id].character.items, before.items);
        assert_eq!(w.entities[&bag].items[0], next);
        assert!(!w.entities[&bag].reserved);
        match kind {
            0xa22 | 0xa3f | 0xb16 => assert!(w.players[&id].character.hp > before.hp),
            0xa23 => assert!(w.players[&id].character.mp > before.mp),
            0xa1f => assert_eq!(w.players[&id].character.base[2], before.base[2] + 1),
            0xc12 => assert!(
                w.players[&id].character.hp > before.hp && w.players[&id].character.mp > before.mp
            ),
            0xae9 => assert_eq!(w.players[&id].character.base[0], before.base[0] + 5),
            0xaea => assert_eq!(w.players[&id].character.base[1], before.base[1] + 5),
            _ => unreachable!(),
        }
    }
}

#[test]
fn failed_ground_consumption_keeps_source_reserved_and_quarantines_owner() {
    for committed in [false, true] {
        let mut w = tests::world();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let bag = w.loot_bag(w.players[&id].pos, None, vec![0xa22]).unwrap();
        let plan = w
            .use_item(
                id,
                Slot {
                    object: bag,
                    slot: 0,
                    kind: 0xa22,
                },
                w.players[&id].pos,
            )
            .unwrap()
            .unwrap();
        let result = if committed {
            w.players.get_mut(&id).unwrap().character.base[0] = -1;
            Ok(1)
        } else {
            Err(anyhow::anyhow!("ground potion storage failure"))
        };
        assert!(w.finish_inventory(plan, result).is_err());
        assert_eq!(w.entities[&bag].items[0], 0xa22);
        assert!(w.entities[&bag].reserved);
        assert!(w.players[&id].pending && w.players[&id].persistence_failure.is_some());
    }
}

#[test]
fn inventory_only_shop_activations_reject_ground_sources_without_mutation() {
    for kind in [0x1175, 0xaa9, 0x1513, 0x705] {
        let mut w = tests::world();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let before = w.players[&id].character.items.clone();
        let owner = w
            .content
            .get(kind as u16)
            .unwrap()
            .soulbound
            .then_some(w.players[&id].character.account);
        let bag = w.loot_bag(w.players[&id].pos, owner, vec![kind]).unwrap();
        let source = w.entities[&bag].items.clone();
        assert!(
            w.use_item(
                id,
                Slot {
                    object: bag,
                    slot: 0,
                    kind
                },
                w.players[&id].pos
            )
            .is_err()
        );
        assert_eq!(w.players[&id].character.items, before);
        assert_eq!(w.entities[&bag].items, source);
        assert!(!w.entities[&bag].reserved && !w.players[&id].pending);
    }
}

#[test]
fn ground_use_rejects_inaccessible_or_stale_sources_without_reserving_items() {
    for case in 0..8 {
        let mut w = tests::world();
        let id = w.enter(tests::ch(&w), None).unwrap();
        let bag = w.loot_bag(w.players[&id].pos, None, vec![0xa22]).unwrap();
        let mut slot = Slot {
            object: bag,
            slot: 0,
            kind: 0xa22,
        };
        match case {
            0 => w.entities.get_mut(&bag).unwrap().pos.x += 3.,
            1 => w
                .entities
                .get_mut(&bag)
                .unwrap()
                .extra_stats
                .push((54, 999)),
            2 => w.entities.get_mut(&bag).unwrap().reserved = true,
            3 => w.entities.get_mut(&bag).unwrap().published = false,
            4 => slot.slot = 8,
            5 => slot.kind = 0xa23,
            6 => {
                slot.slot = 1;
                slot.kind = -1;
            }
            7 => w.entities.get_mut(&bag).unwrap().kind = 0x704,
            _ => unreachable!(),
        }
        let before = w.players[&id].character.items.clone();
        assert!(
            w.use_item(id, slot, w.players[&id].pos).is_err(),
            "case {case}"
        );
        assert_eq!(w.players[&id].character.items, before);
        assert_eq!(w.entities[&bag].items[0], 0xa22);
        assert!(!w.players[&id].pending);
        assert_eq!(w.entities[&bag].reserved, case == 2);
    }
}

#[test]
fn occupied_ground_swaps_drop_incompatible_replacements_without_exposing_soulbound_items() {
    for (incoming, outgoing, private) in [(0xaeb, 0xa97, true), (0xa22, 0xaeb, false)] {
        for reverse in [false, true] {
            let mut w = tests::world();
            let mut ch = tests::ch(&w);
            ch.items[4] = outgoing;
            let id = w.enter(ch, None).unwrap();
            let account = w.players[&id].character.account;
            let bag = w
                .loot_bag(
                    w.players[&id].pos,
                    private.then_some(account),
                    vec![incoming],
                )
                .unwrap();
            if private {
                w.entities.get_mut(&bag).unwrap().kind = 0x503;
            }
            let source = Slot {
                object: bag,
                slot: 0,
                kind: incoming,
            };
            let target = Slot {
                object: id,
                slot: 4,
                kind: outgoing,
            };
            let plan = if reverse {
                w.swap(id, target, source)
            } else {
                w.swap(id, source, target)
            }
            .unwrap();
            let dropped = plan.dropped.unwrap();
            assert_eq!(plan.items[4], incoming);
            assert_eq!(plan.bag.as_ref().unwrap().1[0], -1);
            assert_eq!(w.entities[&dropped].items[0], outgoing);
            assert!(!w.entities[&dropped].published && w.entities[&dropped].reserved);
            assert!(w.entities[&bag].reserved);
            let soulbound = w.content.get(outgoing as u16).unwrap().soulbound;
            assert_eq!(
                w.entities[&dropped].kind,
                if soulbound { 0x503 } else { 0x500 }
            );
            assert_eq!(
                w.entities[&dropped].extra_stats.contains(&(54, account)),
                soulbound || private
            );
            w.finish_inventory(plan, Ok(1)).unwrap();
            assert_eq!(w.players[&id].character.items[4], incoming);
            assert_eq!(w.entities[&bag].items[0], -1);
            assert!(!w.entities[&bag].reserved);
            assert!(w.entities[&dropped].published && !w.entities[&dropped].reserved);
        }
    }
}
