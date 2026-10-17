use super::{tests, *};
use crate::store::{InventoryWrite, Purchase, Store};

fn fixture() -> (Directory, i32, i32) {
    let mut d = tests::directory();
    let w = d.worlds.get_mut(&-2).unwrap();
    let mut ch = tests::ch(w);
    ch.items[5] = 0x705;
    ch.items[6] = 0x705;
    let a = w.enter(ch.clone(), None).unwrap();
    ch.id = 2;
    ch.account = 2;
    let b = w.enter(ch, None).unwrap();
    w.players.get_mut(&b).unwrap().pos = w.players[&a].pos;
    (d, a, b)
}
fn key(w: &mut World, id: i32) -> Result<InventoryPlan> {
    w.native_use_item(
        id,
        0,
        Slot {
            object: id,
            slot: 5,
            kind: 0x705,
        },
        Pos { x: 9999., y: 9999. },
    )?
    .context("key did not reserve a portal")
}

#[test]
fn pirate_key_publishes_once_after_commit_and_both_players_share_the_instance_and_exit() {
    let (mut d, a, b) = fixture();
    let w = d.worlds.get_mut(&-2).unwrap();
    assert_eq!(
        w.content.objects[&0x705].create_object.as_deref(),
        Some("Pirate Cave Portal")
    );
    let pos = w.players[&a].pos;
    let plan = key(w, a).unwrap();
    let opened = plan.unlock.unwrap().opened;
    assert_eq!(w.entities[&opened].pos.distance(pos), 0.);
    assert!(!w.entities[&opened].published && w.entities[&opened].reserved);
    assert_eq!(w.players[&a].character.items[5], 0x705);
    assert!(key(w, a).is_err());
    assert!(w.finish_inventory(plan.clone(), Ok(1)).is_err());
    let plan = d.stage_unlock(plan).unwrap();
    assert!(d.stage_unlock(plan.clone()).is_err());
    assert!(d.destination(b, Some(opened)).is_err());
    let cave = d.portals[&opened].destination;
    d.tick += 500;
    d.step();
    assert!(d.worlds[&-2].entities.contains_key(&opened));
    let replay = plan.clone();
    d.finish_inventory(plan, Ok(1)).unwrap();
    assert!(d.finish_inventory(replay, Ok(1)).is_err());
    let w = d.worlds.get_mut(&-2).unwrap();
    assert_eq!(&w.players[&a].character.items[5..7], &[-1, 0x705]);
    assert!(key(w, a).is_err());
    assert!(w.entities[&opened].published && !w.entities[&opened].reserved);
    assert!(w.entities[&opened].extra_stats.contains(&(37, 1)));
    assert!(!w.bag_expiry.contains_key(&opened));
    assert_eq!(d.portals[&opened].expires, d.tick + 400);
    let mut arrivals = Vec::new();
    for id in [a, b] {
        let (account, character, destination) = d.destination(id, Some(opened)).unwrap();
        assert_eq!(destination, cave);
        let (time, grant, _) = d.issue(account, character, destination).unwrap();
        let ch = d.worlds.get_mut(&-2).unwrap().leave(id).unwrap();
        assert_eq!(d.consume(account, cave, time, grant).unwrap(), character);
        arrivals.push(d.worlds.get_mut(&cave).unwrap().enter(ch, None).unwrap());
    }
    assert_eq!(d.worlds[&cave].players.len(), 2);
    d.tick = d.portals[&opened].expires - 1;
    d.step();
    assert!(!d.portals.contains_key(&opened));
    assert!(!d.worlds[&-2].entities.contains_key(&opened));
    let exit = d.worlds[&cave]
        .entities
        .values()
        .find(|e| e.kind == 0x703)
        .unwrap()
        .id;
    for id in arrivals {
        let pos = d.worlds[&cave].entities[&exit].pos;
        d.worlds
            .get_mut(&cave)
            .unwrap()
            .players
            .get_mut(&id)
            .unwrap()
            .pos = pos;
        assert_eq!(d.destination(id, Some(exit)).unwrap().2, -2);
    }
    assert!(d.worlds.contains_key(&cave));
}

#[test]
fn unsupported_stale_foreign_paused_and_dead_key_uses_do_not_consume_or_publish() {
    let (mut d, a, b) = fixture();
    let w = d.worlds.get_mut(&-2).unwrap();
    let count = w.entities.len();
    for kind in [0x706, 0x701, 0x70a, 0x70b, 0x71f] {
        w.players.get_mut(&a).unwrap().character.items[5] = kind;
        let result = w.use_item(
            a,
            Slot {
                object: a,
                slot: 5,
                kind,
            },
            w.players[&a].pos,
        );
        assert!(result.err().unwrap().to_string().contains("not supported"));
        assert_eq!(w.players[&a].character.items[5], kind);
        assert!(!w.players[&a].pending);
    }
    w.players.get_mut(&a).unwrap().character.items[5] = 0x705;
    for slot in [
        Slot {
            object: b,
            slot: 5,
            kind: 0x705,
        },
        Slot {
            object: a,
            slot: 99,
            kind: 0x705,
        },
        Slot {
            object: a,
            slot: 5,
            kind: 0x706,
        },
    ] {
        assert!(w.use_item(a, slot, w.players[&a].pos).is_err());
    }
    w.players.get_mut(&a).unwrap().paused = true;
    assert!(key(w, a).is_err());
    w.players.get_mut(&a).unwrap().paused = false;
    w.players.get_mut(&a).unwrap().death_pending = true;
    assert!(key(w, a).is_err());
    w.players.get_mut(&a).unwrap().death_pending = false;
    Arc::make_mut(&mut w.content)
        .objects
        .get_mut(&0x705)
        .unwrap()
        .create_object = Some("Spider Den Portal".into());
    assert!(key(w, a).is_err());
    assert_eq!(w.entities.len(), count);
    assert_eq!(w.players[&a].character.items[5], 0x705);
    assert!(!w.players[&a].pending);
}

#[test]
fn key_instance_limit_releases_reservation_without_consuming_or_quarantining() {
    let (mut d, a, _) = fixture();
    while d.worlds.len() < 12 {
        d.instance("pirate-cave", None).unwrap();
    }
    let plan = key(d.worlds.get_mut(&-2).unwrap(), a).unwrap();
    let opened = plan.unlock.unwrap().opened;
    assert!(d.stage_unlock(plan).is_err());
    let w = &d.worlds[&-2];
    assert_eq!(w.players[&a].character.items[5], 0x705);
    assert!(!w.players[&a].pending && w.players[&a].persistence_failure.is_none());
    assert!(!w.entities.contains_key(&opened) && !d.portals.contains_key(&opened));
    assert_eq!(d.worlds.len(), 12);
}

#[tokio::test]
async fn bought_key_consumption_is_durable_and_failed_save_removes_the_unpublished_instance() {
    let mut d = tests::directory();
    let path = std::env::temp_dir().join(format!("pirate-key-{}.db", rand::random::<u64>()));
    let content = d.content.clone();
    let store = Store::open(&path, &content).unwrap();
    for fail in [false, true] {
        let account = store.list(format!("key-{fail}")).await.unwrap().id;
        store
            .register(
                format!("key-{fail}"),
                format!("key-{fail}@example.test"),
                "test-password".into(),
            )
            .await
            .unwrap();
        let ch = store
            .create(account, content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        let mut items = ch.items.clone();
        items[5] = 0x705;
        store
            .purchase(
                InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items,
                },
                Purchase {
                    item: 0x705,
                    slot: 5,
                    price: 25,
                    currency: 0,
                },
            )
            .await
            .unwrap()
            .unwrap();
        let ch = store.load(account, ch.id).await.unwrap();
        let character = ch.id;
        let w = d.worlds.get_mut(&-2).unwrap();
        let id = w.enter(ch, None).unwrap();
        let plan = key(w, id).unwrap();
        let opened = plan.unlock.unwrap().opened;
        let plan = d.stage_unlock(plan).unwrap();
        let cave = d.portals[&opened].destination;
        if fail {
            rusqlite::Connection::open(&path).unwrap().execute_batch("CREATE TRIGGER fail_key BEFORE UPDATE OF items ON characters BEGIN SELECT RAISE(ABORT,'injected key save failure'); END;").unwrap();
        }
        let result = store
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
            .map(|r| r[0]);
        assert_eq!(d.finish_inventory(plan, result).is_err(), fail);
        let reopened = Store::open(&path, &content).unwrap();
        assert_eq!(
            reopened.load(account, character).await.unwrap().items[5],
            if fail { 0x705 } else { -1 }
        );
        assert_eq!(reopened.presentation(account).await.unwrap().credits, 75);
        if fail {
            assert!(!d.worlds.contains_key(&cave) && !d.portals.contains_key(&opened));
            assert!(!d.worlds[&-2].entities.contains_key(&opened));
            assert!(d.worlds[&-2].players[&id].persistence_failure.is_some());
        } else {
            assert!(d.worlds[&-2].entities[&opened].published);
        }
    }
    std::fs::remove_file(path).unwrap();
}
