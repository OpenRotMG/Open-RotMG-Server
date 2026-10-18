use super::*;
use crate::store::InventoryWrite;

#[tokio::test]
async fn equipment_death_waits_for_final_inventory_and_boost_excluded_queued_progress() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let account = a.store.list("equipment-queued".into()).await.unwrap();
    let mut ch = a
        .store
        .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(&a.content, 17999, &mut rand::thread_rng())
        .unwrap();
    ch.xp_boost.active = true;
    ch.items = vec![0xaa2, -1, -1, -1, 0xabf, 0xc01, -1, -1, -1, -1, -1, -1];
    ch.revision = a
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account: ch.account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            vec![],
        )
        .await
        .unwrap()[0];
    a.store.progress(ch.clone()).await.unwrap();
    let character = ch.id;
    let (packets, mut rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    let id = a
        .world
        .call(move |w| w.enter(ch, Some(crate::world::Output { packets, lost })))
        .await
        .unwrap();
    let plan = a
        .world
        .call(move |w| {
            let plan = w.swap(
                id,
                Slot {
                    object: id,
                    slot: 4,
                    kind: 0xabf,
                },
                Slot {
                    object: id,
                    slot: 3,
                    kind: -1,
                },
            )?;
            let p = w.players.get_mut(&id).unwrap();
            p.experience_queued = 501;
            p.character.hp = 0;
            p.death_pending = true;
            assert!(w.pending_progress().is_empty());
            assert!(w.pending_deaths().is_empty());
            Ok(plan)
        })
        .await
        .unwrap();
    assert_eq!(
        a.store.load(account.id, character).await.unwrap().items[3],
        -1
    );
    persist(&a, plan).await.unwrap();
    let snapshot = a
        .world
        .call(move |w| {
            assert!(w.pending_deaths().is_empty());
            assert_eq!(w.players[&id].character.items[3], 0xabf);
            Ok(w.pending_progress().pop().unwrap().1)
        })
        .await
        .unwrap();
    assert_eq!(
        (snapshot.exp, snapshot.xp_boost.bonus_exp, snapshot.fame),
        (19001, 501, 18)
    );
    a.store.progress(snapshot.clone()).await.unwrap();
    let fatal = a
        .world
        .call(move |w| {
            w.finish_progress(id, snapshot)?;
            assert_eq!(w.players[&id].character.hp, 0);
            Ok(w.pending_deaths().pop().unwrap().1)
        })
        .await
        .unwrap();
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|packet| packet.id == 41));
    persist_death(&a, id, fatal, "Queued equipment fixture".into())
        .await
        .unwrap();
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|packet| packet.id == 41));
    assert_eq!(a.store.presentation(account.id).await.unwrap().fame, 19);
    let xml = a.store.fame(account.id, character).await.unwrap();
    assert!(xml.contains("<BaseFame>18</BaseFame>"));
    assert!(xml.contains("<TotalFame>19</TotalFame>"));
    assert!(xml.contains("<Bonus id=\"Well Equipped\" desc=\"Bonus for equipment\">1</Bonus>"));
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM deaths WHERE character=?1",
            [character],
            |r| r.get(0),
        )
        .unwrap();
    let archived: crate::store::Character = serde_json::from_str(&raw).unwrap();
    assert_eq!(archived.items[3], 0xabf);
    assert_eq!(archived.items[4], -1);
    assert_eq!(archived.fame, 18);
    assert!(!archived.xp_boost.active);
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    assert_eq!(reopened.fame(account.id, character).await.unwrap(), xml);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn equipment_death_storage_failure_never_publishes_death_or_reward() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-3);
    let account = a
        .store
        .list("equipment-death-failure".into())
        .await
        .unwrap();
    let ch = a
        .store
        .create(account.id, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let character = ch.id;
    let (packets, mut rx) = mpsc::channel(4096);
    let (lost, _) = tokio::sync::watch::channel(false);
    let (id, fatal) = a
        .world
        .call(move |w| {
            let id = w.enter(ch, Some(crate::world::Output { packets, lost }))?;
            let p = w.players.get_mut(&id).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            Ok((id, w.pending_deaths().pop().unwrap().1))
        })
        .await
        .unwrap();
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_death BEFORE INSERT ON deaths BEGIN SELECT RAISE(ABORT,'forced death storage failure'); END").unwrap();
    assert!(
        persist_death(&a, id, fatal, "rollback enemy".into())
            .await
            .is_err()
    );
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|p| p.id == 41));
    assert!(a.store.load(account.id, character).await.unwrap().alive);
    assert_eq!(a.store.presentation(account.id).await.unwrap().fame, 0);
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i32>(0))
            .unwrap(),
        0
    );
    a.world
        .call(move |w| {
            assert!(w.begin_departure(id).is_err());
            assert!(w.pending_deaths().is_empty());
            Ok(())
        })
        .await
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
