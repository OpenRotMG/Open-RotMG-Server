use super::*;
use crate::store::InventoryWrite;

#[tokio::test]
async fn xp_booster_commits_use_progress_reconnect_and_death_without_bonus_fame() {
    let (dir, mut a) = account_actions::tests::fixture();
    a.world.clock.set_paused(true);
    a.world = a.world.in_world(-2);
    let account = a.store.list("xp-booster-owner".into()).await.unwrap().id;
    let mut ch = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.gain_experience(&a.content, 1995, &mut rand::thread_rng())
        .unwrap();
    a.store.progress(ch.clone()).await.unwrap();
    ch.items[4..7].copy_from_slice(&[0xc42, 0xc42, 0xc43]);
    ch.revision = a
        .store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            Vec::new(),
        )
        .await
        .unwrap()[0];
    let character = ch.id;
    let _owner = a.claim(account).unwrap();
    let bind = lifecycle::binding(&a, account).unwrap();
    let id = a
        .world
        .call(move |w| {
            let id = w.enter(ch, None)?;
            bind(id)?;
            Ok(id)
        })
        .await
        .unwrap();
    let plan = a
        .world
        .call(move |w| {
            let slot = |slot, kind| Slot {
                object: id,
                slot,
                kind,
            };
            assert!(w.use_item(id, slot(6, 0xc43), w.players[&id].pos).is_err());
            let plan = w.use_item(id, slot(4, 0xc42), w.players[&id].pos)?.unwrap();
            assert!(!w.players[&id].character.xp_boost.active);
            assert_eq!(w.players[&id].character.items[4], 0xc42);
            Ok(plan)
        })
        .await
        .unwrap();
    let saved = a.store.load(account, character).await.unwrap();
    assert!(!saved.xp_boost.active);
    assert_eq!(saved.items[4], 0xc42);
    persist(&a, plan).await.unwrap();
    let saved = a.store.load(account, character).await.unwrap();
    assert!(saved.xp_boost.active);
    assert_eq!(saved.items[4], -1);
    assert_eq!(
        (saved.exp, saved.xp_boost.bonus_exp, saved.fame),
        (1995, 0, 1)
    );
    let snapshot = a
        .world
        .call(move |w| {
            let p = &w.players[&id];
            assert!(p.character.xp_boost.active);
            assert!(
                w.use_item(
                    id,
                    Slot {
                        object: id,
                        slot: 5,
                        kind: 0xc42
                    },
                    p.pos
                )
                .is_err()
            );
            assert_eq!(w.players[&id].character.items[5], 0xc42);
            w.players.get_mut(&id).unwrap().experience_queued = 3;
            let (_, snapshot) = w.pending_progress().pop().unwrap();
            assert_eq!(w.players[&id].character.exp, 1995);
            Ok(snapshot)
        })
        .await
        .unwrap();
    assert_eq!(
        (snapshot.exp, snapshot.xp_boost.bonus_exp, snapshot.fame),
        (2001, 3, 1)
    );
    a.store.progress(snapshot.clone()).await.unwrap();
    a.world
        .call(move |w| w.finish_progress(id, snapshot))
        .await
        .unwrap();
    depart(&a, id).await.unwrap();
    let restarted = Store::open(&dir.join("game.db"), &a.content).unwrap();
    let loaded = restarted.load(account, character).await.unwrap();
    assert!(loaded.xp_boost.active);
    assert_eq!(
        (loaded.exp, loaded.xp_boost.bonus_exp, loaded.fame),
        (2001, 3, 1)
    );
    let bind = lifecycle::binding(&a, account).unwrap();
    let (id, fatal) = a
        .world
        .call(move |w| {
            let id = w.enter(loaded, None)?;
            bind(id)?;
            let p = w.players.get_mut(&id).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            let (_, fatal, _) = w.pending_deaths().pop().unwrap();
            Ok((id, fatal))
        })
        .await
        .unwrap();
    persist_death(&a, id, fatal, "XP booster test".into())
        .await
        .unwrap();
    assert_eq!(a.store.presentation(account).await.unwrap().fame, 1);
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT snapshot FROM deaths WHERE character=?1",
            [character],
            |r| r.get(0),
        )
        .unwrap();
    let dead: crate::store::Character = serde_json::from_str(&raw).unwrap();
    assert!(!dead.xp_boost.active);
    assert_eq!((dead.exp, dead.xp_boost.bonus_exp, dead.fame), (2001, 3, 1));
    let next = a
        .store
        .create(account, a.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    assert!(!next.xp_boost.active);
    assert_eq!(next.xp_boost.bonus_exp, 0);
    std::fs::remove_dir_all(dir).unwrap();
}
