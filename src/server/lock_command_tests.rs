use super::lifecycle_finalize_tests::{NativeSocket, socket_load, socket_server};
use super::*;
use std::collections::BTreeSet;

type Lists = Vec<(i32, Vec<i32>)>;

fn account_list(packet: Packet) -> (i32, Vec<i32>) {
    let mut r = Reader::new(&packet.body);
    let id = r.i32().unwrap();
    let accounts = (0..r.i16().unwrap()).map(|_| r.i32().unwrap()).collect();
    r.done().unwrap();
    (id, accounts)
}

async fn chat(socket: &mut NativeSocket, text: &str, expected: &str) -> Lists {
    let mut body = Writer::new();
    body.utf(text);
    socket
        .send(Packet {
            id: wire::PLAYER_TEXT,
            body: body.0,
        })
        .await;
    let mut lists = Vec::new();
    loop {
        let reply = socket
            .receive_one_of(&[wire::ACCOUNT_LIST, wire::TEXT])
            .await;
        if reply.id == wire::ACCOUNT_LIST {
            lists.push(account_list(reply));
            continue;
        }
        let mut r = Reader::new(&reply.body);
        r.utf().unwrap();
        r.i32().unwrap();
        r.i32().unwrap();
        r.u8().unwrap();
        r.utf().unwrap();
        assert_eq!(r.utf().unwrap(), expected, "{text}");
        r.utf().unwrap();
        r.done().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
        return lists;
    }
}

#[tokio::test]
async fn lock_command_persists_offline_account_and_publishes_only_to_owner() {
    let (dir, app) = account_actions::tests::fixture();
    let owner = app
        .store
        .list("lock-command-owner".into())
        .await
        .unwrap()
        .id;
    let peer = app.store.list("lock-command-peer".into()).await.unwrap().id;
    app.store.rename(owner, "Lockowner".into()).await.unwrap();
    app.store.rename(peer, "Lockpeer".into()).await.unwrap();
    let character = app
        .store
        .create(owner, app.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    app.store
        .edit_account_list(owner, 1, true, peer)
        .await
        .unwrap();
    let (port, server) = socket_server(&app).await;
    let mut socket = NativeSocket::connect(port, &app.origin).await;
    socket_load(&mut socket, &dir, "lock-command-owner", character.id).await;
    for expected in [(0, vec![]), (1, vec![peer])] {
        assert_eq!(
            account_list(socket.receive(wire::ACCOUNT_LIST).await),
            expected
        );
    }
    let expected = vec![(0, vec![peer]), (1, vec![peer])];
    for text in [" /LoCk lockPEER ", "/lock Lockpeer"] {
        assert_eq!(chat(&mut socket, text, "Player locked.").await, expected);
    }
    for (text, error) in [
        ("/lock", "Usage: /lock <player>"),
        ("/lock Lockpeer extra", "Usage: /lock <player>"),
        ("/lock Lockowner", "invalid account list edit"),
        ("/lock Missing", "Player not found."),
    ] {
        assert!(chat(&mut socket, text, error).await.is_empty());
        assert_eq!(
            app.store.account_lists(owner).await.unwrap(),
            [BTreeSet::from([peer]), BTreeSet::from([peer])]
        );
    }
    assert_eq!(
        app.store.account_lists(peer).await.unwrap(),
        crate::store::AccountLists::default()
    );
    assert_eq!(
        chat(&mut socket, "/unignore Lockpeer", "Player unignored.").await,
        vec![(0, vec![peer]), (1, vec![])]
    );
    assert_eq!(
        app.store.account_lists(owner).await.unwrap(),
        [BTreeSet::from([peer]), BTreeSet::new()]
    );
    assert_eq!(
        chat(&mut socket, "/ignore Lockpeer", "Player ignored.").await,
        expected
    );
    drop(socket);
    server.abort();
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
