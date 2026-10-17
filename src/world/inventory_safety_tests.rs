use super::*;

fn swap(w: &mut World, player: i32, from: usize, to: usize) -> InventoryPlan {
    w.swap(
        player,
        Slot {
            object: player,
            slot: from,
            kind: w.players[&player].character.items[from],
        },
        Slot {
            object: player,
            slot: to,
            kind: w.players[&player].character.items[to],
        },
    )
    .unwrap()
}

#[test]
fn replayed_inventory_completion_cannot_overwrite_a_later_commit() {
    let mut w = tests::world();
    let player = w.enter(tests::ch(&w), None).unwrap();
    let first = swap(&mut w, player, 4, 5);
    w.finish_inventory(first.clone(), Ok(1)).unwrap();
    let second = swap(&mut w, player, 5, 4);
    w.finish_inventory(second, Ok(2)).unwrap();
    let before = w.players[&player].character.clone();
    assert!(w.finish_inventory(first, Ok(1)).is_err());
    let p = &w.players[&player];
    assert_eq!(
        (p.character.revision, &p.character.items),
        (before.revision, &before.items)
    );
    assert!(p.pending && p.persistence_failure.is_some());
    assert!(w.begin_departure(player).is_err());
}

#[test]
fn success_is_acknowledged_only_after_inventory_publication() {
    let mut w = tests::world();
    let (tx, mut rx) = mpsc::channel(64);
    let (lost, _) = watch::channel(false);
    let player = w
        .enter(tests::ch(&w), Some(Output { packets: tx, lost }))
        .unwrap();
    while rx.try_recv().is_ok() {}
    let before = w.players[&player].character.items.clone();
    let plan = swap(&mut w, player, 4, 5);
    assert_eq!(w.players[&player].character.items, before);
    assert!(rx.try_recv().is_err());
    let expected = plan.items.clone();
    w.finish_inventory(plan, Ok(1)).unwrap();
    let packet = rx.try_recv().unwrap();
    assert_eq!(packet.id, 4);
    assert_eq!(wire::Reader::new(&packet.body).i32().unwrap(), 0);
    assert_eq!(w.players[&player].character.items, expected);
    assert!(!w.players[&player].pending);
    assert!(rx.try_recv().is_err());
}

#[test]
fn foreign_or_wrong_revision_completion_quarantines_without_success() {
    for fault in ["account", "character", "revision", "pending"] {
        let mut w = tests::world();
        let (tx, mut rx) = mpsc::channel(64);
        let (lost, mut lost_rx) = watch::channel(false);
        let player = w
            .enter(tests::ch(&w), Some(Output { packets: tx, lost }))
            .unwrap();
        let mut plan = swap(&mut w, player, 4, 5);
        let before = w.players[&player].character.clone();
        let revision = match fault {
            "account" => {
                plan.account += 1;
                1
            }
            "character" => {
                plan.character += 1;
                1
            }
            "revision" => 2,
            "pending" => {
                w.players.get_mut(&player).unwrap().pending = false;
                1
            }
            _ => unreachable!(),
        };
        while rx.try_recv().is_ok() {}
        assert!(w.finish_inventory(plan, Ok(revision)).is_err(), "{fault}");
        let p = &w.players[&player];
        assert_eq!(
            (p.character.revision, &p.character.items),
            (before.revision, &before.items)
        );
        assert!(p.pending && p.persistence_failure.is_some(), "{fault}");
        assert!(*lost_rx.borrow_and_update(), "{fault}");
        assert!(
            !std::iter::from_fn(|| rx.try_recv().ok()).any(|packet| packet.id == 4),
            "{fault}"
        );
    }
}

#[tokio::test]
async fn failed_pickup_publication_keeps_the_source_reserved() {
    use crate::store::{InventoryWrite, Store};
    let mut w = tests::world();
    let store = Store::open(std::path::Path::new(":memory:"), &w.content).unwrap();
    let account = store
        .list("publication-source-reservation".into())
        .await
        .unwrap();
    let ch = store
        .create(account.id, w.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let player = w.enter(ch.clone(), None).unwrap();
    let mut other = tests::ch(&w);
    other.id = 2;
    other.account = 2;
    let other = w.enter(other, None).unwrap();
    let bag = w
        .entities
        .values()
        .find(|e| e.kind == 0x806)
        .unwrap()
        .clone();
    w.players.get_mut(&player).unwrap().pos = bag.pos;
    let plan = w
        .swap(
            player,
            Slot {
                object: bag.id,
                slot: 0,
                kind: 0xa4f,
            },
            Slot {
                object: player,
                slot: 3,
                kind: -1,
            },
        )
        .unwrap();
    let revisions = store
        .exchange(
            vec![InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision,
                items: plan.items.clone(),
            }],
            None,
        )
        .await
        .unwrap();
    w.players.get_mut(&player).unwrap().character.base[0] = -1;
    assert!(w.finish_inventory(plan, Ok(revisions[0])).is_err());
    assert!(w.players[&player].persistence_failure.is_some());
    assert!(w.entities[&bag.id].reserved);
    assert_eq!(store.load(ch.account, ch.id).await.unwrap().items[3], 0xa4f);
    w.players.get_mut(&other).unwrap().pos = bag.pos;
    assert!(
        w.swap(
            other,
            Slot {
                object: bag.id,
                slot: 0,
                kind: 0xa4f
            },
            Slot {
                object: other,
                slot: 3,
                kind: -1
            },
        )
        .is_err()
    );
}

#[tokio::test]
async fn committed_vault_transfer_recovers_after_publication_failure() {
    use crate::store::{InventoryWrite, Store};
    let path = std::env::temp_dir().join(format!(
        "rotmg-vault-publication-{:016x}.db",
        rand::random::<u64>()
    ));
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let store = Store::open(&path, &content).unwrap();
    let account = store.list("vault-publication".into()).await.unwrap();
    let ch = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let map = Arc::new(Map::load(std::path::Path::new("data/vault.wmap"), &content).unwrap());
    let mut w = World::with_ids(content.clone(), map, Arc::new(AtomicI32::new(1))).unwrap();
    w.name = "Vault".into();
    w.owner = Some(account.id);
    w.safe = true;
    w.load_vault(store.vault(account.id).await.unwrap())
        .unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let (lost, _) = watch::channel(false);
    let player = w
        .enter(ch.clone(), Some(Output { packets: tx, lost }))
        .unwrap();
    let chest = w.vault.as_ref().unwrap().0;
    w.players.get_mut(&player).unwrap().pos = w.entities[&chest].pos;
    let plan = w
        .swap(
            player,
            Slot {
                object: player,
                slot: 4,
                kind: 0xa22,
            },
            Slot {
                object: chest,
                slot: 0,
                kind: -1,
            },
        )
        .unwrap();
    while rx.try_recv().is_ok() {}
    let revisions = store
        .exchange(
            vec![InventoryWrite {
                id: plan.character,
                account: plan.account,
                revision: plan.revision,
                items: plan.items.clone(),
            }],
            plan.vault.clone(),
        )
        .await
        .unwrap();
    assert!(rx.try_recv().is_err());
    w.entities.remove(&chest);
    assert!(w.finish_inventory(plan, Ok(revisions[0])).is_err());
    assert!(w.players[&player].persistence_failure.is_some());
    assert!(w.players[&player].pending);
    assert!(w.begin_departure(player).is_err());
    w.players.get_mut(&player).unwrap().death_pending = true;
    assert!(w.pending_deaths().is_empty());
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|packet| packet.id == 4));
    drop(store);
    let reopened = Store::open(&path, &content).unwrap();
    let saved = reopened.load(account.id, ch.id).await.unwrap();
    assert_eq!((saved.revision, saved.items[4]), (1, -1));
    let vault = reopened.vault(account.id).await.unwrap();
    assert_eq!((vault.revision, vault.items[0]), (1, 0xa22));
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
