use super::*;

async fn transfer(
    d: &mut Directory,
    store: &crate::store::Store,
    player: i32,
    destination: i32,
) -> i32 {
    let source = d.location(player).unwrap();
    let ch = d.worlds[&source].players[&player].character.clone();
    let (time, key, _) = d.issue(ch.account, ch.id, destination).unwrap();
    let mut reservation = d.reserve_transfer(key.clone()).unwrap();
    let snapshot = d
        .begin_source_departure(player, Some(&key))
        .unwrap()
        .unwrap();
    d.accept_source_write(player, &key).unwrap();
    store.progress(snapshot.clone()).await.unwrap();
    d.finish_source_departure(player, snapshot, Some(key.clone()))
        .unwrap();
    reservation.disarm();
    let (_, arrival) = d
        .consume(ch.account, destination, time, key.clone())
        .unwrap();
    d.admit_transfer(arrival, ch.account, ch.id, destination)
        .unwrap();
    let p = d
        .worlds
        .get_mut(&destination)
        .unwrap()
        .enter_admitted(
            store.load(ch.account, ch.id).await.unwrap(),
            None,
            None,
            store.presentation(ch.account).await.unwrap(),
            false,
            Some(key.clone()),
            Controller::Qa,
        )
        .unwrap();
    let entered = d.worlds[&destination].players[&p].entered_at;
    d.clock.set_paused(false);
    std::thread::sleep(std::time::Duration::from_millis(2));
    d.clock.set_paused(true);
    d.complete_dungeon_entry(&key, p).unwrap();
    if source != destination
        && let Some(proof) = d.worlds[&destination].players[&p].dungeon_entered
    {
        assert_eq!(
            proof, entered,
            "entry proof must use actual accepted entry time, not delayed completion"
        );
    }
    p
}

async fn entered(amulet: bool) -> (crate::store::Store, Directory, i32, i32, std::path::PathBuf) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let path = std::env::temp_dir().join(format!("ah-7uck-{}", rand::random::<u64>()));
    std::fs::create_dir(&path).unwrap();
    let store = crate::store::Store::open(&path.join("game.db"), &content).unwrap();
    let account = store.list("synthetic-ah-7uck".into()).await.unwrap().id;
    let mut ch = store
        .create(account, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.level = 20;
    ch.exp = 18050;
    store.progress(ch.clone()).await.unwrap();
    if amulet {
        ch.items[3] = 0xb3e;
        ch.revision = store
            .exchange(
                vec![crate::store::InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                vec![],
            )
            .await
            .unwrap()[0];
    }
    let mut d = Directory::prepared(std::path::Path::new("data"), content, 16, 1376, 2).unwrap();
    d.clock.set_paused(true);
    let (mut source, id) = super::movement_tests::fixture(0x18, false);
    source.name = "Nexus".into();
    source.clock = d.clock.clone();
    source.players.get_mut(&id).unwrap().character = ch;
    let (mut dungeon, _) = super::movement_tests::fixture(0x18, false);
    dungeon.players.clear();
    dungeon.name = "Pirate Cave".into();
    dungeon.safe = false;
    dungeon.clock = d.clock.clone();
    d.worlds.insert(-2, source);
    d.worlds.insert(-3, dungeon);
    let player = transfer(&mut d, &store, id, -3).await;
    (store, d, player, account, path.join("game.db"))
}

#[tokio::test]
async fn ah_7uck_real_dungeon_entry_fatal_event_settles_once_at_permanent_death() {
    let (store, mut d, player, account, path) = entered(false).await;
    let world = d.worlds.get_mut(&-3).unwrap();
    world
        .area_damage(0xdf0, world.players[&player].pos, 2., 100)
        .unwrap();
    let fatal = world.pending_deaths().pop().unwrap().1;
    d.clock.set_paused(false);
    std::thread::sleep(std::time::Duration::from_millis(1750));
    assert_eq!(store.lifecycle_pending_jobs(), 0);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE accounts SET credits=2147483647 WHERE id=?1",
            [account],
        )
        .unwrap();
    assert!(
        store
            .death(fatal.clone(), "induced wallet overflow".into())
            .await
            .is_err()
    );
    assert!(store.load(account, fatal.id).await.unwrap().alive);
    assert_eq!(store.presentation(account).await.unwrap().credits, i32::MAX);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("UPDATE accounts SET credits=100 WHERE id=?1", [account])
        .unwrap();
    store
        .death(fatal.clone(), "synthetic immediate dungeon death".into())
        .await
        .unwrap();
    assert_eq!(
        store.presentation(account).await.unwrap().credits,
        125,
        "missing sixth Medium award after real entry and permanent death"
    );
    let receipt = || {
        rusqlite::Connection::open(&path).unwrap().query_row("SELECT awarded_at,notified_at FROM gold_rewards WHERE account=?1 AND reward='achievement_ah_7uck'",[account],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Option<i64>>(1)?))).unwrap()
    };
    let first = receipt();
    assert!(first.0 > 0 && first.1.is_none());
    assert!(store.death(fatal, "replay".into()).await.is_err());
    assert_eq!(receipt(), first);
    assert_eq!(store.presentation(account).await.unwrap().credits, 125);
}

#[tokio::test]
async fn ah_7uck_clock_boundaries_unknown_entry_level_and_autonomous_are_exact() {
    let (mut w, id) = super::movement_tests::fixture(0x18, false);
    w.name = "Pirate Cave".into();
    w.safe = false;
    w.clock.set_paused(true);
    let now = w.clock.now();
    for (entry, level, autonomous, damage, expected) in [
        (Some(0), 20, false, 200, true),
        (Some(1700), 20, false, 200, true),
        (Some(1701), 20, false, 200, false),
        (None, 20, false, 200, false),
        (Some(0), 19, false, 200, false),
        (Some(0), 20, true, 200, false),
        (Some(0), 20, false, 99, false),
    ] {
        let p = w.players.get_mut(&id).unwrap();
        p.character.hp = 100;
        p.character.level = level;
        p.character.achievements.clear();
        p.death_pending = false;
        p.controller = if autonomous {
            Controller::Autonomous
        } else {
            Controller::Qa
        };
        p.dungeon_entered = entry.map(|ms| now - std::time::Duration::from_millis(ms));
        w.area_damage(0xdf0, Pos { x: 8.5, y: 8.5 }, 2., damage)
            .unwrap();
        assert_eq!(
            w.players[&id]
                .character
                .achievements
                .contains(&crate::achievements::Achievement::Ah7uck),
            expected
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(w.clock.now(), now, "gameClock excludes paused wall time");
}

#[tokio::test]
async fn ah_7uck_entry_proof_covers_dungeons_not_safe_worlds_and_reconnect_never_resets() {
    let (store, mut d, mut player, _, _) = entered(false).await;
    let old = d.clock.now() - std::time::Duration::from_millis(1701);
    d.worlds
        .get_mut(&-3)
        .unwrap()
        .players
        .get_mut(&player)
        .unwrap()
        .dungeon_entered = Some(old);
    player = transfer(&mut d, &store, player, -3).await;
    assert_eq!(d.worlds[&-3].players[&player].dungeon_entered, Some(old));
    let key = d
        .grants
        .iter()
        .find(|(_, g)| g.origin == Some(-3) && g.destination == -3)
        .unwrap()
        .0
        .clone();
    d.complete_dungeon_entry(&key, player).unwrap();
    assert_eq!(d.worlds[&-3].players[&player].dungeon_entered, Some(old));
    assert!(d.complete_dungeon_entry(&key, player + 9999).is_err());
    player = transfer(&mut d, &store, player, -2).await;
    let names = crate::dungeon::DUNGEONS
        .iter()
        .map(|(_, _, name, _)| *name)
        .chain([
            "Oryx's Castle",
            "Oryx's Chamber",
            "Wine Cellar",
            "Realm",
            "Nexus",
            "Vault",
            "Development tutorial",
        ]);
    for name in names {
        d.worlds.get_mut(&-3).unwrap().name = name.into();
        player = transfer(&mut d, &store, player, -3).await;
        let dungeon = !matches!(name, "Realm" | "Nexus" | "Vault" | "Development tutorial");
        assert_eq!(
            d.worlds[&-3].players[&player].dungeon_entered.is_some(),
            dungeon,
            "{name}"
        );
        player = transfer(&mut d, &store, player, -2).await;
        assert!(d.worlds[&-2].players[&player].dungeon_entered.is_none());
    }
}

#[tokio::test]
async fn ah_7uck_volatile_fatal_observation_cannot_flush_or_pay_an_amulet_resurrection() {
    let (store, mut d, player, account, _) = entered(true).await;
    let world = d.worlds.get_mut(&-3).unwrap();
    world
        .area_damage(0xdf0, world.players[&player].pos, 2., 400)
        .unwrap();
    assert!(world.players[&player].death_pending);
    let mut forbidden = world.players[&player].character.clone();
    forbidden.hp = 1;
    assert!(
        store.progress(forbidden).await.is_err(),
        "unpaid fatal observation cannot enter reusable progress"
    );
    world
        .players
        .get_mut(&player)
        .unwrap()
        .queue_experience(1, None, false)
        .unwrap();
    let progress = world.pending_progress_where(|_, _| true).pop().unwrap().1;
    assert!(
        !progress
            .achievements
            .contains(&crate::achievements::Achievement::Ah7uck)
    );
    store.progress(progress.clone()).await.unwrap();
    world.finish_progress(player, progress).unwrap();
    assert!(
        world.players[&player]
            .character
            .achievements
            .contains(&crate::achievements::Achievement::Ah7uck)
    );
    let fatal = world.pending_deaths().pop().unwrap().1;
    let content = world.content.clone();
    let restored = store.resurrect(fatal.clone(), 3, content).await.unwrap();
    assert!(
        !restored
            .achievements
            .contains(&crate::achievements::Achievement::Ah7uck)
    );
    assert!(
        store
            .death(fatal, "stale failed fatal attempt".into())
            .await
            .is_err()
    );
    d.finish_resurrection(player, Ok(restored.clone())).unwrap();
    assert!(
        !store
            .load(account, restored.id)
            .await
            .unwrap()
            .achievements
            .contains(&crate::achievements::Achievement::Ah7uck)
    );
    assert_eq!(store.presentation(account).await.unwrap().credits, 100);
}
