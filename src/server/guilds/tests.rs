use super::*;
use crate::store::{Character, GuildAction};

async fn account(a: &App, guid: &str, name: &str, fame: i32) -> Character {
    let account = a.store.list(guid.into()).await.unwrap();
    a.store.rename(account.id, name.into()).await.unwrap();
    let mut ch = a
        .store
        .create(account.id, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    ch.fame = fame;
    ch.alive = false;
    ch.hp = 0;
    a.store
        .death(ch, "guild native contract fixture".into())
        .await
        .unwrap();
    a.store
        .create(account.id, a.content.get(WIZARD).unwrap())
        .await
        .unwrap()
}
async fn enter(a: &App, ch: Character) -> (i32, mpsc::Receiver<Packet>) {
    let presentation = a.store.presentation(ch.account).await.unwrap();
    let (packets, mut rx) = mpsc::channel(256);
    let (lost, _) = tokio::sync::watch::channel(false);
    let id = a
        .world
        .in_world(-2)
        .call(move |w| {
            let pos = w
                .entities
                .values()
                .find(|e| e.kind == 0x1df)
                .context("Guild Register absent")?
                .pos;
            w.enter_at(
                ch,
                Some(crate::world::Output { packets, lost }),
                Some(Pos {
                    x: pos.x + 1.,
                    y: pos.y,
                }),
                presentation,
            )
        })
        .await
        .unwrap();
    while rx.try_recv().is_ok() {}
    (id, rx)
}
async fn send(a: &App, id: i32, kind: u8, body: Writer) -> Vec<Packet> {
    let (tx, mut rx) = mpsc::channel(16);
    native(a, id, kind, &mut Reader::new(&body.0), &tx, Instant::now())
        .await
        .unwrap();
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}
fn guild_result(packets: Vec<Packet>, success: bool) {
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].id, wire::GUILD_RESULT);
    let mut r = Reader::new(&packets[0].body);
    assert_eq!(r.u8().unwrap(), u8::from(success));
    let error = r.utf().unwrap();
    assert_eq!(error.is_empty(), success);
    r.done().unwrap();
}
fn text(value: &str) -> Writer {
    let mut w = Writer::new();
    w.utf(value);
    w
}

#[test]
fn guild_commands_match_native_help_and_preserve_spaced_names() {
    assert_eq!(
        chat_command("  /InViTe Bobby  "),
        Some((wire::GUILD_INVITE, "Bobby"))
    );
    assert_eq!(
        chat_command("/join Pixel Knights"),
        Some((wire::JOIN_GUILD, "Pixel Knights"))
    );
    assert_eq!(chat_command("/invite"), Some((wire::GUILD_INVITE, "")));
    for text in ["/guild hello", "/invitex Bobby", "hello", ""] {
        assert!(chat_command(text).is_none());
    }
}

#[tokio::test]
async fn guild_native_creation_invite_accept_rank_and_removal_use_committed_runtime_state() {
    let (dir, a) = super::super::account_actions::tests::fixture();
    // Transaction-only clients do not ACK ticks; visibility must not depend on wall-time scheduling.
    a.world.clock.set_paused(true);
    let alice = account(&a, "guild-native-alice", "Alice", 1000).await;
    let aa = alice.account;
    let bob = account(&a, "guild-native-bobby", "Bobby", 20).await;
    let ba = bob.account;
    let carol = account(&a, "guild-native-carol", "Carol", 20).await;
    let (alice, _ar) = enter(&a, alice).await;
    let (bob, mut br) = enter(&a, bob).await;
    let (carol, mut cr) = enter(&a, carol).await;
    a.world
        .in_world(-2)
        .call(move |w| {
            for id in [alice, bob, carol] {
                w.updates(id, false)?;
            }
            Ok(())
        })
        .await
        .unwrap();
    let malformed = text("Alpha");
    let (tx, _rx) = mpsc::channel(16);
    let mut bytes = malformed.0;
    bytes.push(0);
    assert!(
        native(
            &a,
            alice,
            wire::CREATE_GUILD,
            &mut Reader::new(&bytes),
            &tx,
            Instant::now()
        )
        .await
        .is_err()
    );
    assert_eq!(a.store.presentation(aa).await.unwrap().fame, 1000);
    guild_result(
        send(&a, alice, wire::CREATE_GUILD, text("Bad1")).await,
        false,
    );
    guild_result(
        send(&a, alice, wire::CREATE_GUILD, text("Alpha")).await,
        true,
    );
    assert_eq!(a.store.presentation(aa).await.unwrap().fame, 0);
    assert_eq!(
        a.store.presentation(aa).await.unwrap().guild.unwrap().rank,
        40
    );
    let (guild_shop, register) = a
        .world
        .for_player(alice)
        .call(move |w| {
            let register = w.entities.values().find(|e| e.kind == 0x1df).unwrap().id;
            let shop = w.spawn_entity(0x736, w.players[&alice].pos, vec![])?;
            Ok((shop, register))
        })
        .await
        .unwrap();
    let mut guild_buy = Writer::new();
    guild_buy.i32(guild_shop);
    assert!(merchant(&a, alice, &guild_buy.0).await.unwrap());
    let mut other_buy = Writer::new();
    other_buy.i32(register);
    assert!(!merchant(&a, alice, &other_buy.0).await.unwrap());
    other_buy.0.push(0);
    assert!(merchant(&a, alice, &other_buy.0).await.is_err());
    let rejected = send(&a, alice, 50, guild_buy).await;
    assert_eq!(rejected[0].id, 27);
    let mut r = Reader::new(&rejected[0].body);
    assert_ne!(r.i32().unwrap(), 0);
    assert!(r.utf().unwrap().contains("Guild Hall"));
    r.done().unwrap();
    assert_eq!(a.store.guild(aa).await.unwrap().fame, 0);
    send(&a, alice, wire::GUILD_INVITE, text("Bobby")).await;
    let packets: Vec<_> = std::iter::from_fn(|| br.try_recv().ok()).collect();
    let invite = packets
        .iter()
        .find(|p| p.id == wire::INVITED_TO_GUILD)
        .unwrap();
    let mut r = Reader::new(&invite.body);
    assert_eq!(r.utf().unwrap(), "Alice");
    assert_eq!(r.utf().unwrap(), "Alpha");
    r.done().unwrap();
    let wrong = send(&a, bob, wire::JOIN_GUILD, text("Beta")).await;
    assert_eq!(wrong[0].id, wire::TEXT);
    assert!(a.store.presentation(ba).await.unwrap().guild.is_none());
    send(&a, bob, wire::JOIN_GUILD, text("Alpha")).await;
    assert_eq!(
        a.store.presentation(ba).await.unwrap().guild.unwrap().rank,
        0
    );
    let mut rank = text("Alice");
    rank.i32(0);
    guild_result(send(&a, bob, wire::CHANGE_GUILD_RANK, rank).await, false);
    while cr.try_recv().is_ok() {}
    let denied = send(&a, bob, wire::GUILD_INVITE, text("Carol")).await;
    assert_eq!(denied[0].id, wire::TEXT);
    assert!(!std::iter::from_fn(|| cr.try_recv().ok()).any(|p| p.id == wire::INVITED_TO_GUILD));
    let mut rank = text("Bobby");
    rank.i32(20);
    guild_result(send(&a, alice, wire::CHANGE_GUILD_RANK, rank).await, true);
    send(&a, bob, wire::GUILD_INVITE, text("Carol")).await;
    assert!(std::iter::from_fn(|| cr.try_recv().ok()).any(|p| p.id == wire::INVITED_TO_GUILD));
    guild_result(
        send(&a, alice, wire::GUILD_REMOVE, text("Bobby")).await,
        true,
    );
    assert!(a.store.presentation(ba).await.unwrap().guild.is_none());
    let denied = send(&a, bob, wire::GUILD_INVITE, text("Carol")).await;
    assert_eq!(denied[0].id, wire::TEXT);
    assert!(!std::iter::from_fn(|| cr.try_recv().ok()).any(|p| p.id == wire::INVITED_TO_GUILD));
    assert!(
        a.world
            .for_player(carol)
            .call(move |w| Ok(w.players.contains_key(&carol)))
            .await
            .unwrap()
    );
    send(&a, alice, wire::GUILD_INVITE, text("Bobby")).await;
    send(&a, bob, wire::JOIN_GUILD, text("Alpha")).await;
    assert!(a.store.presentation(ba).await.unwrap().guild.is_some());
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    db.execute("DROP TABLE guild_members", []).unwrap();
    let mut rank = text("Bobby");
    rank.i32(10);
    let (tx, mut rx) = mpsc::channel(16);
    let failure = native(
        &a,
        alice,
        wire::CHANGE_GUILD_RANK,
        &mut Reader::new(&rank.0),
        &tx,
        Instant::now(),
    )
    .await
    .unwrap_err();
    assert!(failure.to_string().contains("requires review"));
    assert!(rx.try_recv().is_err());
    for player in [alice, bob] {
        assert!(
            a.world
                .directory(move |d| d.guild_actor(player))
                .await
                .is_err()
        );
        assert!(
            a.world
                .for_player(player)
                .call(move |w| Ok(w.players[&player].character.items[0] == 0xa97))
                .await
                .unwrap()
        );
    }
    assert!(
        a.world
            .directory(move |d| d.guild_actor(carol))
            .await
            .is_ok()
    );
    drop(db);
    drop(a);
    std::fs::remove_dir_all(dir).unwrap();
}

async fn http(
    a: &App,
    operation: &str,
    guid: &str,
    board: Option<&str>,
    origin: Option<&str>,
) -> (StatusCode, String) {
    let mut form = HashMap::from([
        ("guid".into(), guid.into()),
        ("password".into(), String::new()),
        ("num".into(), "50".into()),
        ("offset".into(), "0".into()),
        ("guildId".into(), "1".into()),
    ]);
    if let Some(board) = board {
        form.insert("board".into(), board.into());
    }
    let mut headers = HeaderMap::new();
    if let Some(origin) = origin {
        headers.insert("origin", origin.parse().unwrap());
    }
    let response = request(
        State(a.clone()),
        axum::extract::Path(operation.into()),
        ConnectInfo("127.0.0.1:1234".parse().unwrap()),
        headers,
        Form(form),
    )
    .await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn guild_native_http_chronicle_and_board_auth_permissions_privacy_and_unicode() {
    let (dir, a) = super::super::account_actions::tests::fixture();
    let alice = account(&a, "guild-http-alice", "Alice", 1000).await;
    let bob = account(&a, "guild-http-bobby", "Bobby", 20).await;
    let carol = account(&a, "guild-http-carol", "Carol", 1000).await;
    let (guild, _) = a
        .store
        .change_guild(alice.account, GuildAction::Create("Alpha".into()))
        .await
        .unwrap();
    a.store
        .change_guild(
            bob.account,
            GuildAction::Join {
                guild,
                inviter: alice.account,
            },
        )
        .await
        .unwrap();
    a.store
        .change_guild(carol.account, GuildAction::Create("Beta".into()))
        .await
        .unwrap();
    let board = "Meet at eight!\nBring potions & friends. <not markup>\n日本語 🧙";
    assert_eq!(
        http(&a, "setBoard", "guild-http-alice", Some(board), None).await,
        (StatusCode::OK, board.into())
    );
    assert_eq!(
        http(&a, "getBoard", "guild-http-bobby", None, None).await,
        (StatusCode::OK, board.into())
    );
    assert_eq!(
        http(&a, "setBoard", "guild-http-bobby", Some("overwrite"), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        http(&a, "getBoard", "guild-http-carol", None, None).await,
        (StatusCode::OK, String::new())
    );
    let (_, roster) = http(&a, "listMembers", "guild-http-bobby", None, None).await;
    let xml = roxmltree::Document::parse(&roster).unwrap();
    assert_eq!(xml.root_element().attribute("name"), Some("Alpha"));
    assert!(roster.contains("<Name>Alice</Name><Rank>40</Rank>"));
    assert!(roster.contains("<Name>Bobby</Name><Rank>0</Rank>"));
    assert!(!roster.contains("Carol"));
    let (_, roster) = http(&a, "listMembers", "guild-http-carol", None, None).await;
    assert!(roster.contains("name=\"Beta\""));
    assert!(!roster.contains("Alice"));
    assert_eq!(
        http(
            &a,
            "getBoard",
            "guild-http-alice",
            None,
            Some("https://attacker.invalid")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, error) = http(&a, "getBoard", "missing-guid", None, None).await;
    assert!(error.contains("<Error>"));
    assert!(!error.contains(board));
    assert_eq!(
        a.store.guild_board(alice.account, None).await.unwrap(),
        board
    );
    drop(a);
    std::fs::remove_dir_all(dir).unwrap();
}
