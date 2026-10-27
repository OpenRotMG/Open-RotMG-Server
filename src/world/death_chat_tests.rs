use super::*;

async fn fixture() -> (
    World,
    crate::store::Store,
    [i32; 2],
    Vec<mpsc::Receiver<Packet>>,
) {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let mut world = World::new(content, map).unwrap();
    let mut ids = Vec::new();
    let mut inboxes = Vec::new();
    let (lost, _) = watch::channel(None);
    for role in ["victim", "witness"] {
        let account = store.list(format!("death-chat-{role}")).await.unwrap();
        let mut ch = store
            .create(
                account.id,
                world.content.get(crate::content::WIZARD).unwrap(),
            )
            .await
            .unwrap();
        if role == "victim" {
            ch.name = "Victim".into();
        }
        let (packets, mut inbox) = mpsc::channel(64);
        let output = Some(Output {
            packets,
            lost: lost.clone(),
        });
        ids.push(world.enter(ch, output).unwrap());
        while inbox.try_recv().is_ok() {}
        inboxes.push(inbox);
    }
    let origin = world.players[&ids[0]].pos;
    let witness = world.players.get_mut(&ids[1]).unwrap();
    witness.pos = Pos {
        x: origin.x,
        y: origin.y - 60.,
    };
    (world, store, [ids[0], ids[1]], inboxes)
}

fn messages(inbox: &mut mpsc::Receiver<Packet>) -> Vec<String> {
    std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|p| {
            if p.id != wire::TEXT {
                return None;
            }
            let mut r = wire::Reader::new(&p.body);
            assert_eq!(r.utf().unwrap(), "");
            assert_eq!(r.i32().unwrap(), -1);
            assert_eq!(r.i32().unwrap(), -1);
            assert_eq!(r.u8().unwrap(), 0);
            assert_eq!(r.utf().unwrap(), "");
            Some(r.utf().unwrap())
        })
        .collect()
}

#[tokio::test]
async fn gravestones_follow_all_eleven_tiers_after_committed_death() {
    let tiers = [
        (1, 0, 0x723, Some(30_000)),
        (19, 0, 0x724, Some(60_000)),
        (20, 0, 0x725, Some(300_000)),
        (20, 1, 0x726, None),
        (20, 2, 0x727, None),
        (20, 3, 0x728, None),
        (20, 4, 0x729, None),
        (20, 5, 0x72a, None),
        (20, 6, 0x72b, None),
        (20, 7, 0x734, None),
        (20, 8, 0x735, None),
    ];
    for (level, maxed, kind, lifetime) in tiers {
        let (mut world, store, [victim, witness], _) = fixture().await;
        let caps = world.content.classes[&crate::content::WIZARD].caps;
        let p = world.players.get_mut(&victim).unwrap();
        p.character.level = level;
        p.character.base = std::array::from_fn(|i| caps[i] - i32::from(i >= maxed));
        p.character.hp = 0;
        p.death_pending = true;
        p.killer = "Medusa".into();
        let position = p.pos;
        assert!(world.finish_death(victim).is_err());
        let (_, ch, killer) = world.pending_deaths().pop().unwrap();
        assert!(!ch.items.iter().all(|i| *i == -1));
        assert!(!world.entities.values().any(|e| e.kind == kind));
        world
            .committed(victim, store.death(ch.clone(), killer).await)
            .unwrap();
        let born = world.now;
        world.finish_death(victim).unwrap();
        let graves: Vec<_> = world.entities.values().filter(|e| e.kind == kind).collect();
        assert_eq!(graves.len(), 1, "level {level}, maxed {maxed}");
        let grave = graves[0];
        let grave_id = grave.id;
        assert_eq!((grave.pos.x, grave.pos.y), (position.x, position.y));
        assert_eq!(grave.name, "Victim");
        assert!(grave.items.is_empty());
        assert!(grave.accessible(world.players[&witness].character.account));
        assert_eq!(*grave.expires, lifetime.map(|ms| born + ms));
        assert!(
            world.players[&victim]
                .character
                .items
                .iter()
                .all(|i| *i == -1)
        );
        assert!(store.load(ch.account, ch.id).await.is_err());
        assert!(world.finish_death(victim).is_err());
        let end = born + lifetime.unwrap_or(3_600_000);
        world.now = end - 100;
        world.step();
        assert!(world.entities.contains_key(&grave_id));
        world.step();
        assert_eq!(world.entities.contains_key(&grave_id), lifetime.is_none());
    }
}

#[tokio::test]
async fn death_chat_is_world_wide_once_and_only_after_successful_persistence() {
    for fail in [false, true] {
        let (mut world, store, [victim, witness], inboxes) = fixture().await;
        let [mut victim_inbox, mut inbox]: [_; 2] = inboxes.try_into().unwrap();
        assert!(world.finish_death(victim).is_err());
        assert!(messages(&mut inbox).is_empty());
        let p = world.players.get_mut(&victim).unwrap();
        p.character.hp = 0;
        p.death_pending = true;
        p.killer = "Medusa".into();
        let (_, ch, killer) = world.pending_deaths().pop().unwrap();
        assert!(messages(&mut inbox).is_empty());
        assert!(store.fame(ch.account, ch.id).await.is_err());
        if fail {
            assert!(
                world
                    .committed::<()>(
                        victim,
                        Err(anyhow::anyhow!("injected death commit failure"))
                    )
                    .is_err()
            );
            assert!(world.finish_death(victim).is_err());
            assert!(messages(&mut inbox).is_empty());
            continue;
        }
        world
            .committed(victim, store.death(ch.clone(), killer).await)
            .unwrap();
        assert!(store.fame(ch.account, ch.id).await.is_ok());
        world.finish_death(victim).unwrap();
        assert_eq!(victim_inbox.try_recv().unwrap().id, wire::TEXT);
        assert_eq!(victim_inbox.try_recv().unwrap().id, 41);
        assert!(victim_inbox.try_recv().is_err());
        assert_eq!(
            messages(&mut inbox),
            ["Victim died at level 1, killed by Medusa"]
        );
        assert!(!world.players[&victim].character.alive);
        assert!(world.players[&witness].character.alive);
        assert!(world.finish_death(victim).is_err());
        assert!(messages(&mut inbox).is_empty());
    }
}
