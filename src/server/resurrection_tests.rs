use super::*;
use crate::store::{Character, InventoryWrite};

async fn fixture(
    slot: usize,
) -> (
    PathBuf,
    App,
    i32,
    Character,
    mpsc::Receiver<Packet>,
    tokio::sync::watch::Receiver<bool>,
) {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    let account = a.store.list("amulet-owner".into()).await.unwrap();
    let mut ch = a
        .store
        .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.items[slot] = 0xb3e;
    ch.revision = a
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            Vec::new(),
        )
        .await
        .unwrap()[0];
    let (packets, rx) = mpsc::channel(4096);
    let (lost, loss) = tokio::sync::watch::channel(false);
    a.world = a.world.in_world(-3);
    let (id, fatal) = a
        .world
        .call(move |w| {
            let id = w.enter(ch, Some(crate::world::Output { packets, lost }))?;
            let p = w.players.get_mut(&id).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            let (_, fatal, _) = w.pending_deaths().pop().unwrap();
            Ok((id, fatal))
        })
        .await
        .unwrap();
    (dir, a, id, fatal, rx, loss)
}

#[tokio::test]
async fn equipped_amulet_is_durably_consumed_before_one_owned_nexus_return() {
    let (dir, mut a, id, fatal, mut rx, _) = fixture(3).await;
    let expected = fatal.clone();
    persist_death(&a, id, fatal, "Pirate".into()).await.unwrap();
    let restored = a.store.load(expected.account, expected.id).await.unwrap();
    assert!(restored.alive);
    assert_eq!(restored.items[3], -1);
    assert_eq!(restored.revision, expected.revision + 1);
    let stats = restored.effective(&a.content).unwrap();
    assert_eq!((restored.hp, restored.mp), (stats[0], stats[1]));
    assert_eq!(
        (restored.exp, restored.fame, restored.base),
        (expected.exp, expected.fame, expected.base)
    );
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE character=?1",
            [expected.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        a.store.presentation(expected.account).await.unwrap().fame,
        0
    );
    let (pos, output, before) = a
        .world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.departing && p.character.alive && !p.death_pending);
            assert!(
                !w.entities
                    .values()
                    .any(|e| matches!(e.kind, 0x723..=0x72b | 0x734 | 0x735))
            );
            let snapshot = (
                p.pos,
                p.tx.clone().unwrap(),
                serde_json::to_value(&p.character)?,
            );
            assert!(
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 4,
                        kind: 0xa22
                    },
                    p.pos
                )
                .is_err()
            );
            Ok(snapshot)
        })
        .await
        .unwrap();
    let mut session = Session {
        account: Some(a.claim(expected.account).unwrap()),
        player: Some(id),
        ..Default::default()
    };
    let mut movement = Writer::new();
    movement.i32(0).i32(1000).f32(pos.x).f32(pos.y).i16(0);
    let mut shot = Writer::new();
    shot.i32(1000)
        .u8(0)
        .i16(expected.items[0] as i16)
        .f32(pos.x)
        .f32(pos.y)
        .f32(0.);
    let mut use_item = Writer::new();
    use_item
        .i32(1000)
        .i32(id)
        .u8(4)
        .i16(expected.items[4] as i16)
        .f32(pos.x)
        .f32(pos.y);
    for (kind, body) in [
        (wire::MOVE, movement.0),
        (wire::PLAYER_SHOOT, shot.0),
        (65, use_item.0),
        (wire::UPDATE_ACK, vec![]),
    ] {
        handle(
            &mut a,
            Packet { id: kind, body },
            &mut session,
            &output.packets,
            &output.lost,
            std::time::Instant::now(),
        )
        .await
        .unwrap();
    }
    a.world
        .for_player(id)
        .call(move |w| {
            assert_eq!((w.players[&id].pos.x, w.players[&id].pos.y), (pos.x, pos.y));
            assert_eq!(serde_json::to_value(&w.players[&id].character)?, before);
            Ok(())
        })
        .await
        .unwrap();
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).all(|p| p.id != 41 && p.id != 0));
    let transfer = automatic_transfer(&a, id).await.unwrap().unwrap();
    assert_eq!(transfer.destination, -2);
    assert_eq!(transfer.packet().id, 39);
    session.player = None;
    session.account = None;
    session.reconnecting = true;
    for kind in [65, 30, 35, wire::PLAYER_SHOOT, wire::HELLO] {
        handle(
            &mut a,
            Packet {
                id: kind,
                body: vec![],
            },
            &mut session,
            &output.packets,
            &output.lost,
            std::time::Instant::now(),
        )
        .await
        .unwrap();
    }
    assert!(automatic_transfer(&a, id).await.unwrap().is_none());
    a.world
        .directory(move |d| {
            assert!(
                d.consume(
                    expected.account + 1,
                    -2,
                    transfer.key_time,
                    transfer.key.clone()
                )
                .is_err()
            );
            assert_eq!(
                d.consume(
                    expected.account,
                    -2,
                    transfer.key_time,
                    transfer.key.clone()
                )?,
                expected.id
            );
            assert!(
                d.consume(expected.account, -2, transfer.key_time, transfer.key)
                    .is_err()
            );
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        a.store
            .resurrect(expected.clone(), 3, a.content.clone())
            .await
            .is_err()
    );
    assert_eq!(
        a.store
            .load(expected.account, expected.id)
            .await
            .unwrap()
            .revision,
        restored.revision
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn resurrection_failure_quarantines_without_death_or_live_inventory_changes() {
    let (dir, a, id, fatal, _rx, loss) = fixture(3).await;
    let account = fatal.account;
    let character = fatal.id;
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_resurrection BEFORE UPDATE ON characters BEGIN SELECT RAISE(ABORT,'injected resurrection write failure'); END;").unwrap();
    assert!(persist_death(&a, id, fatal, "Pirate".into()).await.is_err());
    assert!(*loss.borrow());
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.death_pending && p.character.alive && p.death_started);
            assert_eq!(p.character.items[3], 0xb3e);
            assert_eq!(p.character.hp, 0);
            assert!(w.pending_deaths().is_empty());
            Ok(())
        })
        .await
        .unwrap();
    let saved = a.store.load(account, character).await.unwrap();
    assert!(saved.alive);
    assert_eq!(saved.items[3], 0xb3e);
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        0
    );
    assert!(automatic_transfer(&a, id).await.unwrap().is_none());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn disconnect_during_resurrection_keeps_the_saved_character_without_issuing_a_grant() {
    let (dir, a, id, fatal, _rx, _) = fixture(3).await;
    let account = fatal.account;
    let character = fatal.id;
    a.world
        .for_player(id)
        .call(move |w| {
            assert!(w.begin_departure(id)?.is_none());
            Ok(())
        })
        .await
        .unwrap();
    persist_death(&a, id, fatal, "Pirate".into()).await.unwrap();
    assert!(automatic_transfer(&a, id).await.unwrap().is_none());
    depart(&a, id).await.unwrap();
    let saved = a.store.load(account, character).await.unwrap();
    assert!(saved.alive && saved.hp > 0);
    assert_eq!(saved.items[3], -1);
    let presentation = a.store.presentation(account).await.unwrap();
    assert_eq!(presentation.best_fame[&saved.class], saved.fame);
    assert_eq!(presentation.fame, 0);
    assert!(a.store.fame(account, character).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn backpack_amulet_cannot_avoid_death_and_committed_death_cannot_be_resurrected() {
    let (dir, a, id, fatal, _rx, _) = fixture(4).await;
    let account = fatal.account;
    let character = fatal.id;
    persist_death(&a, id, fatal, "Pirate".into()).await.unwrap();
    assert!(a.store.load(account, character).await.is_err());
    assert!(automatic_transfer(&a, id).await.unwrap().is_none());
    std::fs::remove_dir_all(dir).unwrap();
    let (dir, a, _id, mut fatal, _rx, _) = fixture(4).await;
    a.store
        .death(fatal.clone(), "normal committed death".into())
        .await
        .unwrap();
    fatal.items[3] = 0xb3e;
    assert!(
        a.store
            .resurrect(fatal.clone(), 3, a.content.clone())
            .await
            .is_err()
    );
    assert!(a.store.load(fatal.account, fatal.id).await.is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
