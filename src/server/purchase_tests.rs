use super::*;

#[tokio::test]
async fn buy_packet_delivers_an_item_and_rejections_leave_the_session_usable() {
    let (dir, mut a) = account_actions::tests::fixture();
    let account = a
        .store
        .register(
            "packet-buyer".into(),
            "packet@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let presentation = a.store.presentation(account).await.unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let (lost, _) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    a.world = a.world.in_world(-2);
    let (id, merchant) = a
        .world
        .call(move |w| {
            let e = w
                .entities
                .values()
                .find(|e| e.kind == 0x1ca && e.extra_stats.contains(&(34, 0x705)))
                .unwrap();
            let merchant = e.id;
            let pos = Pos {
                x: e.pos.x,
                y: e.pos.y + 0.5,
            };
            let id = w.enter_at(ch, Some(output), Some(pos), presentation)?;
            w.players.get_mut(&id).unwrap().known.insert(merchant);
            Ok((id, merchant))
        })
        .await
        .unwrap();
    let owner = a.claim(account).unwrap();
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    for registered in [false, true] {
        session.account.as_mut().unwrap().registered = registered;
        let mut body = Writer::new();
        body.i32(if registered { -1 } else { merchant });
        handle(
            &mut a,
            Packet {
                id: 50,
                body: body.0,
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await
        .unwrap();
        let packet = std::iter::from_fn(|| rx.try_recv().ok())
            .find(|p| p.id == 27)
            .unwrap();
        assert_eq!(Reader::new(&packet.body).i32().unwrap(), 1);
        assert_eq!(a.store.presentation(account).await.unwrap().credits, 100);
    }
    let mut body = Writer::new();
    body.i32(merchant);
    handle(
        &mut a,
        Packet {
            id: 50,
            body: body.0,
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    let packet = std::iter::from_fn(|| rx.try_recv().ok())
        .find(|p| p.id == 27)
        .unwrap();
    let mut r = Reader::new(&packet.body);
    assert_eq!(r.i32().unwrap(), 0);
    assert_eq!(r.utf().unwrap(), "Item purchased!");
    r.done().unwrap();
    assert_eq!(a.store.presentation(account).await.unwrap().credits, 75);
    assert!(
        a.store
            .load(account, character)
            .await
            .unwrap()
            .items
            .contains(&0x705)
    );
    handle(
        &mut a,
        Packet {
            id: wire::UPDATE_ACK,
            body: vec![],
        },
        &mut session,
        &tx,
        &lost,
        std::time::Instant::now(),
    )
    .await
    .unwrap();
    assert_eq!(session.player, Some(id));
    assert!(!*lost.borrow());
    assert!(
        handle(
            &mut a,
            Packet {
                id: 50,
                body: vec![0; 3]
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now()
        )
        .await
        .is_err()
    );
    assert!(
        handle(
            &mut a,
            Packet {
                id: 50,
                body: vec![0; 5]
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now()
        )
        .await
        .is_err()
    );
    session.account.take().unwrap().release();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn xp_booster_shop_loads_real_stock_and_commits_only_funded_current_purchases() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-2);
    let merchant = a
        .world
        .call(|w| {
            assert!(
                !w.entities
                    .values()
                    .any(|e| e.extra_stats.contains(&(34, 0xc43)))
            );
            let e = w
                .entities
                .values()
                .find(|e| e.kind == 0x1ca && e.extra_stats.contains(&(34, 0xc42)))
                .context("normal XP Booster must be stocked in the loaded Nexus")?;
            assert!(e.extra_stats.contains(&(36, 100)) && e.extra_stats.contains(&(40, 0)));
            Ok(e.id)
        })
        .await
        .unwrap();
    for case in ["normal", "full", "level20", "boosted", "stale"] {
        let account = a
            .store
            .register(
                format!("xp-shop-{case}"),
                format!("{case}@example.test"),
                "test-password".into(),
            )
            .await
            .unwrap();
        let mut ch = a
            .store
            .create(account, a.content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        if case == "level20" {
            ch.gain_experience(&a.content, 100_000, &mut rand::thread_rng())
                .unwrap();
            assert_eq!(ch.level, 20);
        }
        ch.xp_boost.active = case == "boosted";
        a.store.progress(ch.clone()).await.unwrap();
        if case == "full" {
            ch.items[4..].fill(0xa50);
            ch.revision = a
                .store
                .exchange(
                    vec![crate::store::InventoryWrite {
                        id: ch.id,
                        account,
                        revision: ch.revision,
                        items: ch.items.clone(),
                    }],
                    Vec::new(),
                )
                .await
                .unwrap()[0];
        }
        let character = ch.id;
        let before = ch.clone();
        let presentation = a.store.presentation(account).await.unwrap();
        let (id, stock) = a
            .world
            .call(move |w| {
                let e = &w.entities[&merchant];
                let stock = e.extra_stats.iter().find(|(k, _)| *k == 42).unwrap().1;
                let pos = Pos {
                    x: e.pos.x,
                    y: e.pos.y + 0.6,
                };
                assert!(w.walkable(pos));
                let id = w.enter_at(ch, None, Some(pos), presentation)?;
                w.players.get_mut(&id).unwrap().known.insert(merchant);
                Ok((id, stock))
            })
            .await
            .unwrap();
        if case == "full" {
            assert!(
                a.world
                    .call(move |w| w.buy(id, merchant))
                    .await
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("inventory space")
            );
        } else {
            if case == "stale" {
                a.store
                    .exchange(
                        vec![crate::store::InventoryWrite {
                            id: character,
                            account,
                            revision: before.revision,
                            items: before.items.clone(),
                        }],
                        Vec::new(),
                    )
                    .await
                    .unwrap();
            }
            let plan = a.world.call(move |w| w.buy(id, merchant)).await.unwrap();
            let purchase = &plan.purchase.as_ref().unwrap().1;
            let slot = purchase.slot;
            assert_eq!(
                (purchase.item, purchase.price, purchase.currency),
                (0xc42, 100, 0)
            );
            assert_eq!(a.store.presentation(account).await.unwrap().credits, 100);
            assert_eq!(
                a.store.load(account, character).await.unwrap().items,
                before.items
            );
            let result = persist(&a, plan).await;
            if case == "stale" {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("purchase conflict")
                );
            } else {
                result.unwrap();
                let saved = a.store.load(account, character).await.unwrap();
                assert_eq!(a.store.presentation(account).await.unwrap().credits, 0);
                assert_eq!(saved.items[slot], 0xc42);
                assert_eq!(saved.revision, before.revision + 1);
                assert_eq!(
                    (saved.level, saved.xp_boost.active),
                    (before.level, before.xp_boost.active)
                );
                if case == "normal" {
                    let plan = a.world.call(move |w| w.buy(id, merchant)).await.unwrap();
                    persist(&a, plan).await.unwrap();
                    let declined = a.store.load(account, character).await.unwrap();
                    assert_eq!(
                        (declined.items, declined.revision),
                        (saved.items, saved.revision)
                    );
                    let plan = a
                        .world
                        .call(move |w| {
                            w.use_item(
                                id,
                                Slot {
                                    object: id,
                                    slot,
                                    kind: 0xc42,
                                },
                                w.players[&id].pos,
                            )
                        })
                        .await
                        .unwrap()
                        .unwrap();
                    persist(&a, plan).await.unwrap();
                    let used = a.store.load(account, character).await.unwrap();
                    assert!(used.xp_boost.active);
                    assert_eq!((used.items[slot], used.revision), (-1, saved.revision + 1));
                }
            }
        }
        let rejected = matches!(case, "full" | "stale");
        if rejected {
            let saved = a.store.load(account, character).await.unwrap();
            assert_eq!(saved.items, before.items);
            assert_eq!(saved.revision, before.revision + i32::from(case == "stale"));
            assert_eq!(a.store.presentation(account).await.unwrap().credits, 100);
        }
        a.world
            .call(move |w| {
                let p = &w.players[&id];
                assert_eq!(
                    w.entities[&merchant]
                        .extra_stats
                        .iter()
                        .find(|(k, _)| *k == 42)
                        .unwrap()
                        .1,
                    stock - i32::from(!rejected)
                );
                if case == "stale" {
                    assert!(p.pending && w.entities[&merchant].reserved);
                    assert_eq!(p.character.items, before.items);
                } else {
                    assert!(!p.pending && !w.entities[&merchant].reserved);
                }
                Ok(())
            })
            .await
            .unwrap();
        if case != "stale" {
            depart(&a, id).await.unwrap();
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn closed_vault_buy_packet_commits_paid_storage_before_native_success() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let account = a
        .store
        .register(
            "vault-packet-buyer".into(),
            "vault-packet@example.test".into(),
            "test-password".into(),
        )
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute("UPDATE accounts SET credits=1000 WHERE id=?1", [account])
        .unwrap();
    let ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let world = a
        .world
        .directory(move |d| d.instance("vault", Some(account)))
        .await
        .unwrap();
    a.world = a.world.in_world(world);
    let vault = a.store.vault(account, 0).await.unwrap();
    let presentation = a.store.presentation(account).await.unwrap();
    let (tx, mut rx) = mpsc::channel(128);
    let (lost, _) = tokio::sync::watch::channel(false);
    let output = crate::world::Output {
        packets: tx.clone(),
        lost: lost.clone(),
    };
    let (id, seller) = a
        .world
        .call(move |w| {
            w.load_vaults(vec![vault])?;
            assert_eq!(w.entities.values().filter(|e| e.kind == 0x504).count(), 1);
            assert_eq!(
                w.entities.values().filter(|e| e.kind == 0x505).count(),
                19,
                "nineteen native closed chests must be offered"
            );
            let seller = w.entities.values().find(|e| e.kind == 0x505).unwrap();
            let (seller, pos) = (seller.id, seller.pos);
            let id = w.enter_at(ch, Some(output), Some(pos), presentation)?;
            w.players.get_mut(&id).unwrap().known.insert(seller);
            Ok((id, seller))
        })
        .await
        .unwrap();
    let mut owner = a.claim(account).unwrap();
    owner.registered = true;
    let mut session = Session {
        account: Some(owner),
        player: Some(id),
        ..Default::default()
    };
    for (target, success) in [(seller, true), (seller, false), (-1, false)] {
        let mut body = Writer::new();
        body.i32(target);
        handle(
            &mut a,
            Packet {
                id: 50,
                body: body.0,
            },
            &mut session,
            &tx,
            &lost,
            std::time::Instant::now(),
        )
        .await
        .unwrap();
        let packet = std::iter::from_fn(|| rx.try_recv().ok())
            .find(|p| p.id == 27)
            .unwrap();
        assert_eq!(
            Reader::new(&packet.body).i32().unwrap(),
            i32::from(!success)
        );
        assert_eq!(a.store.presentation(account).await.unwrap().credits, 500);
        assert_eq!(a.store.load(account, character).await.unwrap().revision, 1);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM vaults WHERE account=?1",
                [account],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            2
        );
    }
    assert!(!*lost.borrow());
    session.account.take().unwrap().release();
    std::fs::remove_dir_all(dir).unwrap();
}
