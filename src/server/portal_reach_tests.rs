use super::lifecycle_finalize_tests::{NativeSocket, socket_hello, socket_load, socket_server};
use super::*;

fn assert_refusal(reply: Packet, reason: &str) {
    if reply.id == 0 {
        let mut failure = Reader::new(&reply.body);
        failure.i32().unwrap();
        eprintln!(
            "Expected recoverable {reason:?}; received FAILURE {:?}",
            failure.utf().unwrap()
        );
    }
    assert_eq!(
        reply.id,
        wire::TEXT,
        "refusal must not send FAILURE or RECONNECT"
    );
    let mut text = Reader::new(&reply.body);
    text.utf().unwrap();
    text.i32().unwrap();
    text.i32().unwrap();
    text.u8().unwrap();
    text.utf().unwrap();
    assert_eq!(text.utf().unwrap(), reason);
}

#[tokio::test]
async fn portal_out_of_reach_refuses_and_closed_realm_preserves_session_then_allows_entry() {
    let (dir, app) = account_actions::tests::fixture();
    app.world.clock.set_paused(true);
    let guid = "portal-reach-owner";
    let account = app.store.list(guid.into()).await.unwrap().id;
    let character = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let character_id = character.id;
    let (port, server) = socket_server(&app).await;
    let mut socket = NativeSocket::connect(port, &app.origin).await;
    let player = socket_load(&mut socket, &dir, guid, character_id).await;
    let (portal, pos, destination) = app
        .world
        .directory(move |d| {
            let world = d.worlds.get_mut(&-2).unwrap();
            let portal = world.entities.values().find(|e| e.kind == 0x712).unwrap();
            let (id, pos) = (portal.id, portal.pos);
            world.players.get_mut(&player).unwrap().pos = pos;
            let destination = d.destination(player, Some(id))?.2;
            let world = d.worlds.get_mut(&-2).unwrap();
            world.players.get_mut(&player).unwrap().pos = Pos {
                x: pos.x,
                y: pos.y + 2.001,
            };
            world.step();
            world.step();
            Ok((id, pos, destination))
        })
        .await
        .unwrap();
    let tick = Reader::new(&socket.receive(wire::NEW_TICK).await.body)
        .i32()
        .unwrap();
    let use_portal = || Packet {
        id: 3,
        body: portal.to_be_bytes().to_vec(),
    };
    socket.send(use_portal()).await;
    assert_refusal(
        socket.receive_one_of(&[wire::TEXT, 0, 39]).await,
        "portal out of reach",
    );
    let retained = move |d: &mut crate::world::Directory| {
        assert_eq!(d.location(player)?, -2);
        let p = &d.worlds[&-2].players[&player];
        assert!(p.character.alive && !p.departing && !p.death_pending);
        assert_eq!(
            (p.character.account, p.character.id),
            (account, character_id)
        );
        assert!(!d.character_transferring(character_id));
        assert!(d.worlds.values().all(|w| w.occupancy() == w.players.len()));
        Ok(())
    };
    app.world.directory(retained).await.unwrap();
    assert_eq!(
        app.store.load(account, character_id).await.unwrap(),
        character
    );
    assert!(app.owners.lock().unwrap().contains_key(&account));
    app.world
        .directory(move |d| {
            d.worlds
                .get_mut(&destination)
                .unwrap()
                .set_admission_open_for_test(false);
            Ok(())
        })
        .await
        .unwrap();
    // MOVE and USEPORTAL share a frame just as buffered native writes can; entry must see the preceding MOVE.
    let mut movement = Writer::new();
    movement
        .i32(tick)
        .i32(100)
        .f32(pos.x)
        .f32(pos.y + 2.)
        .i16(0);
    socket
        .send_frame(vec![
            Packet {
                id: wire::MOVE,
                body: movement.0,
            },
            use_portal(),
        ])
        .await;
    assert_refusal(
        socket.receive_one_of(&[wire::TEXT, 0, 39]).await,
        "Realm is closed",
    );
    app.world.directory(retained).await.unwrap();
    assert_eq!(
        app.store.load(account, character_id).await.unwrap(),
        character
    );
    app.world
        .directory(move |d| {
            let moved = d.worlds[&-2].players[&player].pos;
            assert_eq!((moved.x, moved.y), (pos.x, pos.y + 2.));
            d.worlds
                .get_mut(&destination)
                .unwrap()
                .set_admission_open_for_test(true);
            Ok(())
        })
        .await
        .unwrap();
    socket.send(use_portal()).await;
    let reply = socket.receive_one_of(&[0, 39]).await;
    assert_eq!(
        reply.id, 39,
        "unchanged two-tile boundary must allow normal transfer"
    );
    let mut reconnect = Reader::new(&reply.body);
    reconnect.utf().unwrap();
    reconnect.utf().unwrap();
    reconnect.i32().unwrap();
    assert_eq!(reconnect.i32().unwrap(), destination);
    let time = reconnect.i32().unwrap();
    let n = reconnect.i16().unwrap() as usize;
    let key = reconnect.take(n).unwrap().to_vec();
    reconnect.done().unwrap();
    let mut replacement = NativeSocket::connect(port, &app.origin).await;
    replacement
        .send(socket_hello(&dir, guid, destination, Some((time, key))))
        .await;
    replacement.receive(wire::MAP_INFO).await;
    replacement
        .send(Packet {
            id: wire::LOAD,
            body: character_id.to_be_bytes().to_vec(),
        })
        .await;
    let created = replacement.receive(wire::CREATE_SUCCESS).await;
    let arrived = Reader::new(&created.body).i32().unwrap();
    assert_ne!(arrived, player);
    app.world
        .directory(move |d| {
            assert_eq!(d.location(arrived)?, destination);
            let p = &d.worlds[&destination].players[&arrived];
            assert_eq!(
                (p.character.account, p.character.id),
                (account, character_id)
            );
            assert!(p.character.alive && !p.death_pending);
            Ok(())
        })
        .await
        .unwrap();
    let saved = app.store.load(account, character_id).await.unwrap();
    assert!(saved.alive);
    assert_eq!(saved.tombs, character.tombs);
    assert_eq!(saved.items, character.items);
    assert_eq!(saved.revision, character.revision);
    assert_eq!(saved.exp, character.exp);
    assert_eq!(saved.base, character.base);
    assert_eq!(
        rusqlite::Connection::open(dir.join("game.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM deaths WHERE character=?1",
                [character_id],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
        0
    );
    drop(replacement);
    drop(socket);
    server.abort();
    drop(app);
}

#[tokio::test]
async fn portal_missing_remains_a_fatal_protocol_error() {
    let (dir, app) = account_actions::tests::fixture();
    app.world.clock.set_paused(true);
    let guid = "portal-missing-owner";
    let account = app.store.list(guid.into()).await.unwrap().id;
    let character = app
        .store
        .create(account, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (port, server) = socket_server(&app).await;
    let mut socket = NativeSocket::connect(port, &app.origin).await;
    socket_load(&mut socket, &dir, guid, character.id).await;
    socket
        .send(Packet {
            id: 3,
            body: i32::MAX.to_be_bytes().to_vec(),
        })
        .await;
    let reply = socket.receive_one_of(&[0, 39, wire::TEXT]).await;
    assert_eq!(reply.id, 0);
    let mut failure = Reader::new(&reply.body);
    assert_eq!(failure.i32().unwrap(), 0);
    assert_eq!(failure.utf().unwrap(), "portal missing");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while app.diagnostics.previous_drop(account).is_null() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        app.diagnostics.previous_drop(account)["transport"]["reason"],
        "Protocol"
    );
    assert!(app.store.load(account, character.id).await.unwrap().alive);
    drop(socket);
    server.abort();
    drop(app);
}
