use super::*;
use crate::{content::WIZARD, store::GuildMembership, wire::Reader};

fn membership(id: i32, rank: i32) -> GuildMembership {
    GuildMembership {
        id,
        name: format!("Guild {id}"),
        rank,
    }
}
fn presentation(guild: Option<GuildMembership>) -> AccountPresentation {
    AccountPresentation {
        guild,
        credits: 100,
        fame: 1000,
        best_fame: BTreeMap::from([(WIZARD, 20)]),
    }
}
fn receive(
    d: &mut Directory,
    world: i32,
    account: i32,
    name: &str,
    guild: Option<GuildMembership>,
) -> (i32, mpsc::Receiver<Packet>) {
    let w = d.worlds.get_mut(&world).unwrap();
    let mut ch = super::super::tests::ch(w);
    ch.id = account;
    ch.account = account;
    ch.name = name.into();
    let (packets, mut rx) = mpsc::channel(128);
    let (lost, _) = watch::channel(false);
    let id = w
        .enter_at(
            ch,
            Some(Output { packets, lost }),
            None,
            presentation(guild),
        )
        .unwrap();
    while rx.try_recv().is_ok() {}
    (id, rx)
}
fn guild(id: i32, level: i32) -> Guild {
    Guild {
        id,
        name: format!("Guild {id}"),
        level,
        fame: 1000000,
        earned: 1000000,
        board: String::new(),
    }
}

#[test]
fn guild_invitation_requires_live_named_starred_visible_peer_and_expires_without_replay() {
    let mut d = super::super::tests::directory();
    let now = Instant::now();
    let (a, mut ar) = receive(&mut d, -2, 1, "Alice", Some(membership(1, 40)));
    let (b, mut br) = receive(&mut d, -2, 2, "Bobby", None);
    let (_, mut cr) = receive(&mut d, -4, 3, "Carol", None);
    let w = d.worlds.get_mut(&-2).unwrap();
    w.updates(a, false).unwrap();
    while ar.try_recv().is_ok() {}
    assert!(d.guild_invite(a, "Carol", now).is_err());
    assert!(cr.try_recv().is_err());
    let origin = d.worlds[&-2].players[&b].pos;
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&b)
        .unwrap()
        .pos = Pos { x: 0.5, y: 0.5 };
    assert!(d.guild_invite(a, "Bobby", now).is_err());
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .players
        .get_mut(&b)
        .unwrap()
        .pos = origin;
    d.guild_invite(a, "bObBy", now).unwrap();
    let packet = br.try_recv().unwrap();
    assert_eq!(packet.id, wire::INVITED_TO_GUILD);
    let mut r = Reader::new(&packet.body);
    assert_eq!(r.utf().unwrap(), "Alice");
    assert_eq!(r.utf().unwrap(), "Guild 1");
    r.done().unwrap();
    assert_eq!(d.guild_invitation(b, "gUiLd 1", now).unwrap(), (1, 1));
    assert!(d.guild_invitation(b, "Guild 2", now).is_err());
    assert!(
        d.guild_invitation(b, "Guild 1", now + Duration::from_secs(60))
            .is_err()
    );
    assert!(d.guild_invitation(a, "Guild 1", now).is_err());
    d.sync_guild(2, presentation(Some(membership(1, 0))), false)
        .unwrap();
    assert!(d.guild_invitation(b, "Guild 1", now).is_err());
    assert!(
        d.guild_invite(b, "Carol", now + Duration::from_secs(5))
            .is_err()
    );
    assert!(ar.try_recv().is_err());
    assert!(cr.try_recv().is_err());
    d.sync_guild(2, presentation(None), false).unwrap();
    drop(br);
    assert!(
        d.guild_invite(a, "Bobby", now + Duration::from_secs(10))
            .is_err()
    );
    assert!(
        d.guild_invitation(b, "Guild 1", now + Duration::from_secs(10))
            .is_err()
    );
}

#[test]
fn guild_chat_crosses_worlds_with_native_recipient_stars_ignore_and_membership_isolation() {
    let mut d = super::super::tests::directory();
    let now = Instant::now();
    let (a, mut ar) = receive(&mut d, -2, 1, "Alice", Some(membership(1, 40)));
    let (b, mut br) = receive(&mut d, -4, 2, "Bobby", Some(membership(1, 0)));
    let (_, mut cr) = receive(&mut d, -2, 3, "Carol", Some(membership(2, 40)));
    d.chat(a, "/g Meet in our hall".into(), now).unwrap();
    for packet in [ar.try_recv().unwrap(), br.try_recv().unwrap()] {
        assert_eq!(packet.id, wire::TEXT);
        let mut r = Reader::new(&packet.body);
        assert_eq!(r.utf().unwrap(), "Alice");
        assert_eq!(r.i32().unwrap(), -1);
        assert_eq!(r.i32().unwrap(), 1);
        assert_eq!(r.u8().unwrap(), 0);
        assert_eq!(r.utf().unwrap(), "*Guild*");
        assert_eq!(r.utf().unwrap(), "Meet in our hall");
        assert_eq!(r.utf().unwrap(), "");
        r.done().unwrap();
    }
    assert!(cr.try_recv().is_err());
    d.worlds
        .get_mut(&-4)
        .unwrap()
        .players
        .get_mut(&b)
        .unwrap()
        .account_lists[1]
        .insert(1);
    d.chat(a, "/GUILD Ignored".into(), now).unwrap();
    assert!(ar.try_recv().is_ok());
    assert!(br.try_recv().is_err());
    d.sync_guild(2, presentation(None), false).unwrap();
    d.chat(a, "/g After removal".into(), now).unwrap();
    assert!(ar.try_recv().is_ok());
    assert!(br.try_recv().is_err());
    assert!(cr.try_recv().is_err());
    assert!(d.chat(a, "/g Throttled".into(), now).is_err());
    assert!(d.chat(b, "/g Former member".into(), now).is_err());
}

#[test]
fn guild_cache_publication_preserves_concurrent_wallet_and_history_updates() {
    let mut d = super::super::tests::directory();
    let (id, _rx) = receive(&mut d, -2, 2, "Bobby", None);
    let p = d.worlds.get_mut(&-2).unwrap().players.get_mut(&id).unwrap();
    p.presentation.credits = 7;
    p.presentation.fame = 25;
    let mut update = presentation(Some(membership(1, 10)));
    update.best_fame.insert(WIZARD, 2000);
    d.sync_guild(2, update.clone(), false).unwrap();
    let p = &d.worlds[&-2].players[&id];
    assert_eq!(
        (
            p.presentation.credits,
            p.presentation.fame,
            p.presentation.best_fame[&WIZARD]
        ),
        (7, 25, 20)
    );
    assert_eq!(p.presentation.guild.as_ref().unwrap().rank, 10);
    update.fame = 0;
    d.sync_guild(2, update, true).unwrap();
    let p = &d.worlds[&-2].players[&id];
    assert_eq!(
        (
            p.presentation.credits,
            p.presentation.fame,
            p.presentation.best_fame[&WIZARD]
        ),
        (7, 0, 20)
    );
}

#[test]
fn guild_stats_are_public_but_wallets_remain_owner_only_and_creation_requires_register() {
    let mut d = super::super::tests::directory();
    let (a, _ar) = receive(&mut d, -2, 1, "Alice", Some(membership(1, 40)));
    let (b, _br) = receive(&mut d, -2, 2, "Bobby", None);
    let w = d.worlds.get_mut(&-2).unwrap();
    for recipient in [a, b] {
        let bytes = w.stats(a, recipient).unwrap();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.i32().unwrap(), a);
        r.f32().unwrap();
        r.f32().unwrap();
        let count = r.i16().unwrap();
        let mut strings = BTreeMap::new();
        let mut numbers = BTreeMap::new();
        for _ in 0..count {
            let key = r.u8().unwrap();
            if matches!(key, 31 | 62) {
                strings.insert(key, r.utf().unwrap());
            } else {
                numbers.insert(key, r.i32().unwrap());
            }
        }
        r.done().unwrap();
        assert_eq!(strings[&62], "Guild 1");
        assert_eq!(numbers[&63], 40);
        assert_eq!(numbers.contains_key(&35), recipient == a);
        assert_eq!(numbers.contains_key(&39), recipient == a);
    }
    assert!(d.guild_creation(a).is_err());
    let w = d.worlds.get_mut(&-2).unwrap();
    let pos = w.entities.values().find(|e| e.kind == 0x1df).unwrap().pos;
    let pos = [
        Pos {
            x: pos.x + 1.,
            y: pos.y,
        },
        Pos {
            x: pos.x - 1.,
            y: pos.y,
        },
    ]
    .into_iter()
    .find(|p| w.walkable(*p))
    .unwrap();
    w.players.get_mut(&a).unwrap().pos = pos;
    w.updates(a, false).unwrap();
    d.guild_creation(a).unwrap();
}

#[test]
fn guild_halls_are_private_shareable_grant_safe_and_upgrade_only_after_empty_idle() {
    let mut d = super::super::tests::directory();
    let first = d.guild_instance(guild(1, 1)).unwrap();
    let other = d.guild_instance(guild(2, 1)).unwrap();
    assert_ne!(first, other);
    let (a, _ar) = receive(&mut d, first, 1, "Alice", Some(membership(1, 40)));
    assert_eq!(d.guild_instance(guild(1, 2)).unwrap(), first);
    assert_eq!(d.worlds[&first].guild, Some((1, 1)));
    let w = d.worlds.get_mut(&first).unwrap();
    let mut ch = super::super::tests::ch(w);
    ch.account = 2;
    ch.id = 2;
    assert!(
        w.enter_at(ch.clone(), None, None, presentation(None))
            .is_err()
    );
    assert!(
        w.enter_at(ch, None, None, presentation(Some(membership(2, 40))))
            .is_err()
    );
    let merchant = w.entities.values().find(|e| e.kind == 0x736).unwrap().id;
    assert!(d.guild_upgrade_level(a, merchant).is_err());
    d.sync_guild(1, presentation(None), false).unwrap();
    let transfer = d.forced_transfer(a).unwrap().unwrap();
    assert_eq!(transfer.destination, -2);
    assert!(d.worlds[&first].players[&a].departing);
    d.worlds.get_mut(&first).unwrap().players.remove(&a);
    d.idle.insert(first, 0);
    d.tick = 6000;
    let (time, key, _) = d.issue(2, 2, first).unwrap();
    d.step();
    assert!(d.worlds.contains_key(&first));
    assert!(d.consume(999, first, time, key).is_err());
    d.tick += 2401;
    d.step();
    assert!(!d.worlds.contains_key(&first));
    let upgraded = d.guild_instance(guild(1, 2)).unwrap();
    assert_ne!(upgraded, first);
    assert_eq!(d.worlds[&upgraded].map.width, 53);
}

#[test]
fn all_four_guild_layouts_resolve_selected_assets_and_reachable_native_facilities() {
    let c = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    for (level, width, height) in [(1, 32, 32), (2, 53, 47), (3, 45, 42), (4, 57, 60)] {
        let map = Map::load(
            &std::path::Path::new("data").join(format!("guild-hall-{level}.wmap")),
            &c,
        )
        .unwrap();
        assert_eq!((map.width, map.height), (width, height));
        let w = World::new(c.clone(), Arc::new(map)).unwrap();
        let start = (w.map.spawns[0].0 as i32, w.map.spawns[0].1 as i32);
        let mut reached = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some((x, y)) = queue.pop_front() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let next = (x + dx, y + dy);
                if w.walkable(Pos {
                    x: next.0 as f32 + 0.5,
                    y: next.1 as f32 + 0.5,
                }) && reached.insert(next)
                {
                    queue.push_back(next);
                }
            }
        }
        assert!(w.entities.values().any(|e| e.kind == 0x1e5));
        assert!(w.entities.values().any(|e| e.kind == 0x1e7));
        assert!(w.entities.values().any(|e| e.kind == 0x720));
        assert_eq!(
            w.entities
                .values()
                .filter(|e| (0x736..=0x738).contains(&e.kind))
                .count(),
            usize::from(level < 4)
        );
        for e in w
            .entities
            .values()
            .filter(|e| matches!(e.kind, 0x1e5 | 0x1e7 | 0x720 | 0x712 | 0x736..=0x738))
        {
            assert!(
                reached.iter().any(|(x, y)| Pos {
                    x: *x as f32 + 0.5,
                    y: *y as f32 + 0.5
                }
                .distance(e.pos)
                    <= 1.5),
                "level {level} unreachable facility {}",
                e.kind
            );
        }
    }
}
