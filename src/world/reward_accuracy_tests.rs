use super::*;
use crate::{
    loot::evaluator::{self, Owner},
    store::world_events::{LIMITS, TerminalPayload},
};

async fn reward_world() -> (World, crate::store::Store, Vec<(i32, i32)>) {
    let content = Arc::new(crate::world::test_content());
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let mut w = World::in_domain(content, map, WorldDomain::NonRealm).unwrap();
    for id in w.entities.keys().copied().collect::<Vec<_>>() {
        w.remove_entity(id);
    }
    w.encounter = true;
    let map = Arc::make_mut(&mut w.map);
    map.spawns = vec![(1.5, 1.5)];
    for cell in &mut map.cells {
        cell.ground = 0x61;
        cell.object = None;
    }
    for i in 0..6 {
        let account = store.list(format!("reward-accuracy-{i}")).await.unwrap();
        let character = store
            .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        w.enter(character, None).unwrap();
    }
    let owners = w
        .players
        .values()
        .map(|p| (p.character.account, p.character.id))
        .collect();
    (w, store, owners)
}

/// Mirrors the journaled publication packaging so pickup-source tests start from a real native bag.
fn source_bag(w: &mut World, pos: Pos, owner: Option<i32>, items: Vec<i32>, source: u16) {
    let count = items.len();
    let bag = w.loot_bag(pos, owner, items).unwrap();
    let name = w.content.objects[&source].name.clone();
    let e = w.entities.get_mut(&bag).unwrap();
    e.drop_slots = (0..count).collect();
    e.drop_source = Some(name);
}

fn clear_bags(w: &mut World) {
    // Expire published bags through their real lifetime so the journal settles each case before the next.
    if let Some(deadline) = w.bag_expiry.values().max().copied() {
        w.now = w.now.max(deadline);
        w.step();
    }
    crate::world::xml::tests::drain_events(w).unwrap();
    assert!(w.bag_expiry.is_empty() && w.delivery_events().is_empty());
}

/// Defeat through the real terminal path and shared real-journal drain; returns the journaled payload.
fn defeat(w: &mut World, kind: u16, damage: &[(i32, i32)], seed: u64) -> TerminalPayload {
    defeat_at(w, kind, Pos { x: 3.5, y: 3.5 }, damage, seed)
}
fn defeat_at(
    w: &mut World,
    kind: u16,
    pos: Pos,
    damage: &[(i32, i32)],
    seed: u64,
) -> TerminalPayload {
    clear_bags(w);
    let id = w.spawn_entity(kind, pos, vec![]).unwrap();
    w.entities.get_mut(&id).unwrap().damage = damage.iter().copied().collect();
    w.rng = rand::rngs::StdRng::seed_from_u64(seed);
    w.defeat_entity(id).unwrap();
    let event = w
        .pending_terminal_events()
        .into_iter()
        .next()
        .expect("terminal journaled");
    assert!(
        w.bag_expiry.is_empty(),
        "loot visible before durable receipt"
    );
    let payload = event.terminal_payload().unwrap().clone();
    // Publication emits at most four bags per drain; repeat until the whole plan is visible.
    for _ in 0..8 {
        crate::world::xml::tests::drain_events(w).unwrap();
    }
    assert!(!w.entities.contains_key(&id), "kind={kind:#x}");
    payload
}

/// The visible bags are exactly the journaled plan, which is the evaluator's plan for the captured facts.
fn assert_published_plan(w: &World, payload: &TerminalPayload, owners: &[(i32, i32)]) -> usize {
    let game = w.xml_game();
    assert_eq!(
        payload.allocation.as_ref(),
        Some(
            &evaluator::evaluate(game, payload.defeat, &payload.ledger, payload.seed, LIMITS)
                .unwrap()
        )
    );
    let account = |character: i32| owners.iter().find(|(_, c)| *c == character).unwrap().0;
    let rows = &game.definitions[&payload.defeat.kind].loot;
    let mut expected: Vec<_> = payload
        .allocations()
        .iter()
        .map(|a| match a.owner() {
            Owner::Public => (None, i32::from(a.item())),
            Owner::Character(c) => (Some(account(c)), i32::from(a.item())),
        })
        .collect();
    for (row, r) in rows.iter().enumerate() {
        if matches!(r.ownership, crate::content::game::LootOwnership::Public) {
            let count = payload
                .allocations()
                .iter()
                .filter(|a| a.row() == row)
                .count();
            assert!(
                count <= (r.min as usize).max(1),
                "public row {row} rolled per contributor"
            );
        }
    }
    let mut actual = Vec::new();
    for bag in w.bag_expiry.keys().map(|id| &w.entities[id]) {
        assert_eq!(
            bag.drop_source.as_deref(),
            Some(w.content.objects[&payload.defeat.kind].name.as_str())
        );
        let owner = bag
            .extra_stats
            .iter()
            .find(|(stat, _)| *stat == 54)
            .map(|(_, o)| *o);
        for item in bag.items.iter().copied().filter(|item| *item >= 0) {
            assert!(owner.is_some() || !w.content.objects[&(item as u16)].soulbound);
            actual.push((owner, item));
        }
        for (other, _) in owners {
            assert_eq!(bag.accessible(*other), owner.is_none_or(|o| o == *other));
        }
    }
    expected.sort_unstable();
    actual.sort_unstable();
    assert_eq!(
        actual, expected,
        "kind={:#x} seed={}",
        payload.defeat.kind, payload.seed
    );
    expected.len()
}

#[tokio::test]
async fn original_source_survives_pickup_but_player_redeposits_do_not_create_discoveries() {
    let (mut w, store, _) = reward_world().await;
    let player = *w.players.keys().next().unwrap();
    w.players.get_mut(&player).unwrap().controller = Controller::Human;
    let pos = w.players[&player].pos;
    let account = w.players[&player].character.account;
    for items in [vec![0xa22; 8], vec![0xa22]] {
        source_bag(&mut w, pos, Some(account), items, 0x91f);
    }
    for bag in w.bag_expiry.keys().map(|id| &w.entities[id]) {
        assert_eq!(
            bag.drop_source.as_deref(),
            Some(w.content.objects[&0x91f].name.as_str())
        );
    }
    let bag = *w.bag_expiry.keys().next().unwrap();
    let ground = Slot {
        object: bag,
        slot: 0,
        kind: 0xa22,
    };
    let inventory = Slot {
        object: player,
        slot: 11,
        kind: -1,
    };
    let plan = w.swap(player, ground, inventory).unwrap();
    assert_eq!(
        plan.collected,
        Some(crate::store::CollectedItem {
            item: 0xa22,
            source: w.content.objects[&0x91f].name.clone()
        })
    );
    let revision = store
        .exchange_collected(
            vec![crate::store::InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision,
                items: plan.items.clone(),
            }],
            vec![],
            plan.collected.clone(),
        )
        .await
        .unwrap()[0];
    w.finish_inventory(plan, Ok(revision)).unwrap();
    assert!(!w.entities[&bag].drop_slots.contains(&0));
    let plan = w
        .swap(
            player,
            Slot {
                kind: 0xa22,
                ..inventory
            },
            Slot { kind: -1, ..ground },
        )
        .unwrap();
    assert!(plan.collected.is_none());
    w.finish_inventory(plan, Ok(revision + 1)).unwrap();
    let plan = w.swap(player, ground, inventory).unwrap();
    assert!(plan.collected.is_none());
    w.finish_inventory(plan, Ok(revision + 2)).unwrap();
    w.players.get_mut(&player).unwrap().character.items[11] = -1;
    for container in [
        w.loot_bag(pos, None, vec![0xa22]).unwrap(),
        w.insert_native_entity(
            0x501,
            NativeService::Container,
            pos,
            vec![0xa22, -1, -1, -1, -1, -1, -1, -1],
            "prepared public treasure-container ownership regression",
        )
        .unwrap(),
    ] {
        let plan = w
            .swap(
                player,
                Slot {
                    object: container,
                    ..ground
                },
                inventory,
            )
            .unwrap();
        assert!(plan.collected.is_none());
        let revision = plan.revision + 1;
        w.finish_inventory(plan, Ok(revision)).unwrap();
        w.players.get_mut(&player).unwrap().character.items[11] = -1;
    }
}

#[tokio::test]
async fn generated_landmark_chest_loot_has_its_chest_source() {
    let (mut w, _store, _) = reward_world().await;
    let player = *w.players.keys().next().unwrap();
    w.players.get_mut(&player).unwrap().controller = Controller::Human;
    let pos = w.players[&player].pos;
    for kind in [
        crate::dungeon::jungle::COIL,
        0x925,
        0x675,
        0x678,
        0x67d,
        0x668,
    ] {
        let mut picked = false;
        for seed in 0..64 {
            for id in w.entities.keys().copied().collect::<Vec<_>>() {
                w.remove_entity(id);
            }
            w.rng = rand::rngs::StdRng::seed_from_u64(seed);
            w.spawn_landmark_chests(kind, pos).unwrap();
            let source = format!("{} chest", w.content.objects[&kind].name);
            for chest in w.entities.values().filter(|e| e.kind == 0x501) {
                assert_eq!(chest.kind, 0x501);
                assert_eq!(chest.drop_source.as_deref(), Some(source.as_str()));
                assert_eq!(
                    chest.drop_slots.len(),
                    chest.items.iter().filter(|i| **i >= 0).count()
                );
            }
            if let Some((id, slot, item)) = w.entities.values().find_map(|e| {
                e.items
                    .iter()
                    .enumerate()
                    .find(|(_, i)| {
                        **i >= 0 && w.content.objects[&(**i as u16)].class == "Equipment"
                    })
                    .map(|(slot, item)| (e.id, slot, *item))
            }) {
                let plan = w
                    .swap(
                        player,
                        Slot {
                            object: id,
                            slot,
                            kind: item,
                        },
                        Slot {
                            object: player,
                            slot: 11,
                            kind: -1,
                        },
                    )
                    .unwrap();
                assert_eq!(
                    plan.collected,
                    Some(crate::store::CollectedItem {
                        item: item as u16,
                        source
                    })
                );
                let revision = plan.revision + 1;
                w.finish_inventory(plan, Ok(revision)).unwrap();
                w.players.get_mut(&player).unwrap().character.items[11] = -1;
                picked = true;
                break;
            }
        }
        assert!(picked, "no equipment generated for chest {kind:#x}");
    }
}

#[tokio::test]
async fn private_loot_reentry_deadline_and_committed_pickup() {
    let (mut w, store, owners) = reward_world().await;
    let (account, character) = owners[0];
    let player = w
        .players
        .iter()
        .find(|(_, p)| p.character.id == character)
        .unwrap()
        .0
        .to_owned();
    let outsider = *w
        .players
        .iter()
        .find(|(_, p)| p.character.id == owners[1].1)
        .unwrap()
        .0;
    let pos = w.players[&player].pos;
    let bag = w.loot_bag(pos, Some(account), vec![0xb0c, 0xb0c]).unwrap();
    let deadline = w.bag_expiry[&bag];
    let source = Slot {
        object: bag,
        slot: 0,
        kind: 0xb0c,
    };
    let destination = |id| Slot {
        object: id,
        slot: 11,
        kind: -1,
    };
    assert_eq!(w.players[&outsider].character.items[11], -1);
    assert!(w.swap(outsider, source, destination(outsider)).is_err());
    assert_eq!(w.entities[&bag].items[0], 0xb0c);
    assert!(!w.entities[&bag].reserved);
    w.leave(player).unwrap();
    let loaded = store.load(account, character).await.unwrap();
    let reentered = w.enter(loaded, None).unwrap();
    assert_ne!(reentered, player);
    assert_eq!(w.bag_expiry[&bag], deadline);
    assert!(w.entities[&bag].accessible(account));
    assert!(!w.entities[&bag].accessible(owners[1].0));
    w.now = deadline - 50;
    assert_eq!(w.players[&reentered].character.items[11], -1);
    let plan = w.swap(reentered, source, destination(reentered)).unwrap();
    w.step();
    assert_eq!(w.now, deadline);
    assert!(w.entities[&bag].reserved);
    assert_eq!(w.players[&reentered].character.items[11], -1);
    let revisions = store
        .exchange_collected(
            vec![crate::store::InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision,
                items: plan.items.clone(),
            }],
            plan.vaults.clone(),
            plan.collected.clone(),
        )
        .await
        .unwrap();
    w.finish_inventory(plan, Ok(revisions[0])).unwrap();
    assert!(
        w.swap(
            reentered,
            source,
            Slot {
                object: reentered,
                slot: 10,
                kind: -1
            }
        )
        .is_err()
    );
    w.step();
    assert!(!w.entities.contains_key(&bag));
    assert!(!w.bag_expiry.contains_key(&bag));
    assert_eq!(
        store.load(account, character).await.unwrap().items[11],
        0xb0c
    );
    let saved = w.leave(reentered).unwrap();
    let mut restarted = World::new(w.content.clone(), w.map.clone()).unwrap();
    let id = restarted.enter(saved, None).unwrap();
    assert_eq!(restarted.players[&id].character.items[11], 0xb0c);
    assert!(restarted.bag_expiry.is_empty());
}

#[tokio::test]
async fn lair_slime_splits_birth_children_and_only_terminal_forms_have_potion_rows() {
    let (mut w, _store, owners) = reward_world().await;
    let game = w.xml_game();
    for kind in [0xd9b, 0xd9c, 0xd9e] {
        assert!(game.definitions[&kind].loot.is_empty());
    }
    for kind in [0xd9d, 0xd9f] {
        assert!(
            game.definitions[&kind]
                .loot
                .iter()
                .all(|row| row.pool == [0xa22, 0xa23])
        );
    }
    for (parent, child) in [(0xd9b, 0xd9c), (0xd9c, 0xd9d), (0xd9e, 0xd9f)] {
        for id in w.entities.keys().copied().collect::<Vec<_>>() {
            w.remove_entity(id);
        }
        let payload = defeat(&mut w, parent, &[(owners[0].1, 1)], 7);
        assert!(
            payload
                .allocation
                .as_ref()
                .is_none_or(|plan| plan.allocations().is_empty())
        );
        assert!(
            w.bag_expiry.is_empty(),
            "intermediate {parent:#x} rolled loot"
        );
        assert!(
            w.actors().any(|e| e.kind == child),
            "{parent:#x} births {child:#x}"
        );
    }
}

#[tokio::test]
async fn septavius_and_arachna_clear_stage_one_realm_exit_until_durable_commit() {
    let (mut w, _store, owners) = reward_world().await;
    for (template, boss, dens) in [
        ("undead-lair", 0xd90, 0),
        ("spider-den", crate::dungeon::spider::BOSS, 1),
    ] {
        w.xml.template = Some(template.into());
        w.cleared = false;
        let id = w
            .spawn_entity(boss, Pos { x: 3.5, y: 3.5 }, vec![])
            .unwrap();
        w.entities
            .get_mut(&id)
            .unwrap()
            .damage
            .insert(owners[0].1, 1);
        let before = w
            .players
            .values()
            .find(|p| p.character.id == owners[0].1)
            .unwrap()
            .character
            .spider_dens;
        w.defeat_entity(id).unwrap();
        assert!(!w.cleared && !w.actors().any(|e| e.kind == 0x704 && e.published));
        crate::world::xml::tests::drain_events(&mut w).unwrap();
        assert!(w.cleared && !w.entities.contains_key(&id));
        assert_eq!(
            w.actors().filter(|e| e.kind == 0x704).count(),
            1,
            "{template}"
        );
        assert!(!w.actors().any(|e| e.kind == 0x71d));
        let after = w
            .players
            .values()
            .find(|p| p.character.id == owners[0].1)
            .unwrap()
            .character
            .spider_dens;
        assert_eq!(after, before + dens);
        for e in w
            .actors()
            .filter(|e| e.kind == 0x704)
            .map(|e| e.id)
            .collect::<Vec<_>>()
        {
            w.remove_entity(e);
        }
    }
}

#[tokio::test]
async fn kage_kami_and_flying_brain_defeats_birth_their_portal_through_the_terminal_path() {
    let (mut w, _store, owners) = reward_world().await;
    for (kind, portal) in [
        (0x6db, "Manor of the Immortals Portal"),
        (0x657, "Mad Lab Portal"),
    ] {
        let portal = w
            .content
            .objects
            .values()
            .find(|o| o.name == portal)
            .unwrap()
            .kind;
        let damage = [(owners[0].1, w.content.objects[&kind].hp)];
        let seed = (0..2000)
            .find(|seed| {
                defeat(&mut w, kind, &damage, *seed);
                let born = w.entities.values().filter(|e| e.kind == portal).count();
                for id in w
                    .entities
                    .values()
                    .filter(|e| e.kind == portal)
                    .map(|e| e.id)
                    .collect::<Vec<_>>()
                {
                    w.remove_entity(id);
                }
                assert!(born <= 1, "{kind:#x} seed={seed} born={born}");
                born == 1
            })
            .unwrap_or_else(|| panic!("{kind:#x} never birthed its portal in 2000 defeats"));
        assert!(seed < 2000);
    }
}

#[tokio::test]
async fn god_potion_and_equipment_bags_do_not_stack() {
    let (mut w, _store, owners) = reward_world().await;
    let hp = w.content.objects[&0x651].hp;
    let damage: Vec<_> = owners[..3].iter().map(|(_, c)| (*c, hp)).collect();
    let mut seed = 0;
    loop {
        let payload = defeat(&mut w, 0x651, &damage, seed);
        assert_published_plan(&w, &payload, &owners);
        if w.bag_expiry.len() > 1 {
            break;
        }
        seed += 1;
        assert!(
            seed < 256,
            "fixed seeds publish several bags from one defeat"
        );
    }
    let positions: BTreeSet<_> = w
        .bag_expiry
        .keys()
        .map(|id| {
            let p = w.entities[id].pos;
            (p.x.to_bits(), p.y.to_bits())
        })
        .collect();
    assert_eq!(
        positions.len(),
        w.bag_expiry.len(),
        "seed={seed}: bags stack"
    );
}

#[tokio::test]
async fn god_loot_chunks_scatter_inside_corpse_tile_without_changing_owners_or_rng() {
    let (mut w, _store, owners) = reward_world().await;
    let hp = w.content.objects[&0x651].hp;
    let damage: Vec<_> = owners[..3].iter().map(|(_, c)| (*c, hp)).collect();
    for corpse in [
        Pos { x: 3.001, y: 3.001 },
        Pos { x: 3.999, y: 3.999 },
        Pos { x: 3., y: 4. },
    ] {
        let mut published = 0;
        for seed in 0..16 {
            let payload = defeat_at(&mut w, 0x651, corpse, &damage, seed);
            assert_published_plan(&w, &payload, &owners);
            for id in w.bag_expiry.keys() {
                let pos = w.entities[id].pos;
                assert_eq!(
                    (pos.x.floor(), pos.y.floor()),
                    (corpse.x.floor(), corpse.y.floor())
                );
                assert!(
                    (pos.x - corpse.x).abs() <= 0.5
                        && (pos.y - corpse.y).abs() <= 0.5
                        && w.walkable(pos)
                );
            }
            let Some(&bag) = w.bag_expiry.keys().next() else {
                continue;
            };
            let (pos, items) = (w.entities[&bag].pos, w.entities[&bag].items.clone());
            w.remove_entity(bag);
            let rng = w.rng.clone();
            crate::world::xml::tests::drain_events(&mut w).unwrap();
            assert_eq!(
                rand::RngCore::next_u64(&mut w.rng),
                rand::RngCore::next_u64(&mut rng.clone())
            );
            let again = w
                .bag_expiry
                .keys()
                .map(|id| &w.entities[id])
                .find(|e| e.items == items)
                .expect("republished bag");
            assert_eq!(
                (again.pos.x.to_bits(), again.pos.y.to_bits()),
                (pos.x.to_bits(), pos.y.to_bits())
            );
            published += 1;
        }
        assert!(published > 0);
    }
}

#[tokio::test]
async fn level_one_players_receive_uncapped_xml_god_experience() {
    let (mut w, _store, _) = reward_world().await;
    assert!(w.players.values().all(|p| p.character.level == 1));
    defeat(&mut w, 0x67d, &[], 0);
    assert!(w.players.values().all(|p| p.character.exp == 450));
}
