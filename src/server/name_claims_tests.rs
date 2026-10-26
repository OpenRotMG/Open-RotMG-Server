use super::*;
use crate::store::Character;

async fn registered(a: &App, identity: &str) -> i32 {
    a.store
        .register(
            identity.into(),
            format!("{identity}@example.test"),
            "test-password".into(),
        )
        .await
        .unwrap()
}

async fn request(a: &App, identity: &str, name: &str) -> String {
    let response = account_request(
        State(a.clone()),
        ConnectInfo("127.0.0.1:12345".parse().unwrap()),
        HeaderMap::new(),
        axum::extract::Path("setName".into()),
        Form(HashMap::from([
            ("guid".into(), format!("{identity}@example.test")),
            ("password".into(), "test-password".into()),
            ("name".into(), name.into()),
        ])),
    )
    .await;
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn persona(a: &App, name: &str) -> Character {
    a.store
        .persona_character(
            format!("persona:v1:{name}"),
            vec![Character::fresh(a.content.get(WIZARD).unwrap()).unwrap()],
            vec![[0xa1f; 8]],
            0,
            None,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn name_claims_http_inactive_and_archived_personas_preserve_identity_and_block_readmission() {
    let (dir, a) = test_app();
    a.world.clock.set_paused(true);
    let first = persona(&a, "Keeper").await;
    a.store
        .retire_persona("persona:v1:Keeper".into(), first.account)
        .await
        .unwrap();
    let second = persona(&a, "Keeper").await;
    let human = registered(&a, "human").await;
    assert_eq!(request(&a, "human", "kEePeR").await, "<Success/>");
    assert_eq!(a.store.presentation(human).await.unwrap().credits, 100);
    for old in [&first, &second] {
        assert_eq!(
            serde_json::to_value(a.store.load(old.account, old.id).await.unwrap()).unwrap(),
            serde_json::to_value(old).unwrap()
        );
        assert_eq!(
            a.store.vaults(old.account).await.unwrap()[0].items,
            vec![0xa1f; 8]
        );
    }
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    assert_eq!(reopened.list("human".into()).await.unwrap().name, "kEePeR");
    assert!(
        reopened
            .persona_character(
                "persona:v1:Keeper".into(),
                vec![Character::fresh(a.content.get(WIZARD).unwrap()).unwrap()],
                vec![[0xa1f; 8]],
                0,
                None
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("another account")
    );
    registered(&a, "competitor").await;
    assert!(
        request(&a, "competitor", "KEEPER")
            .await
            .contains("name already taken")
    );
}

#[tokio::test]
async fn name_claims_http_active_persona_stops_and_saves_before_human_publication() {
    let (_dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let bot = persona(&a, name).await;
    let owner = a.claim(bot.account).unwrap();
    crate::bots::start_test(&a, bot.clone(), -2, Pos { x: 77.5, y: 72.5 }, name, owner)
        .await
        .unwrap();
    registered(&a, "activehuman").await;
    assert_eq!(request(&a, "activehuman", name).await, "<Success/>");
    assert!(!a.owners.lock().unwrap().contains_key(&bot.account));
    a.world
        .directory(move |d| {
            assert!(!d.worlds.values().any(|w| {
                w.players
                    .values()
                    .any(|p| p.character.account == bot.account)
            }));
            Ok(())
        })
        .await
        .unwrap();
    assert!(a.store.load(bot.account, bot.id).await.unwrap().alive);
    assert!(a.store.bot_mind(bot.account).await.unwrap().is_some());
    a.shutdown.send_replace(true);
}

async fn running(a: &App, name: &str) -> (Character, i32) {
    let bot = persona(a, name).await;
    let owner = a.claim(bot.account).unwrap();
    crate::bots::start_test(a, bot.clone(), -2, Pos { x: 77.5, y: 72.5 }, name, owner)
        .await
        .unwrap();
    let account = bot.account;
    let id = a
        .world
        .directory(move |d| {
            Ok(*d.worlds[&-2]
                .players
                .iter()
                .find(|(_, p)| p.character.account == account)
                .unwrap()
                .0)
        })
        .await
        .unwrap();
    (bot, id)
}

async fn native_name(
    socket: &mut lifecycle_finalize_tests::NativeSocket,
    name: &str,
) -> (u8, String) {
    let mut body = Writer::new();
    body.utf(name);
    socket
        .send(Packet {
            id: 33,
            body: body.0,
        })
        .await;
    let packet = socket.receive(20).await;
    let mut reader = Reader::new(&packet.body);
    let result = (reader.u8().unwrap(), reader.utf().unwrap());
    reader.done().unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_claims_native_free_paid_and_rejections_preserve_wallet_and_active_personas() {
    use lifecycle_finalize_tests::*;
    let (dir, a) = test_app();
    a.world.clock.set_paused(true);
    let names = crate::bots::profiles();
    let (first, _) = running(&a, names[0]).await;
    let (protected, _) = running(&a, names[1]).await;
    let paid = persona(&a, "Paidbot").await;
    let account = registered(&a, "nativehuman").await;
    let ch = a
        .store
        .create(account, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    rusqlite::Connection::open(dir.join("game.db"))
        .unwrap()
        .execute("UPDATE accounts SET credits=1600 WHERE id=?1", [account])
        .unwrap();
    let ordinary = registered(&a, "ordinary").await;
    a.store
        .choose_name(ordinary, "Takenname".into())
        .await
        .unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket
        .send(socket_hello_password(
            &dir,
            "nativehuman@example.test",
            "test-password",
            -2,
            None,
        ))
        .await;
    socket.receive(wire::MAP_INFO).await;
    socket
        .send(Packet {
            id: wire::LOAD,
            body: ch.id.to_be_bytes().to_vec(),
        })
        .await;
    let id = Reader::new(&socket.receive(wire::CREATE_SUCCESS).await.body)
        .i32()
        .unwrap();
    assert_eq!(native_name(&mut socket, names[0]).await, (1, String::new()));
    assert_eq!(a.store.presentation(account).await.unwrap().credits, 1600);
    assert!(!a.owners.lock().unwrap().contains_key(&first.account));
    assert_eq!(
        native_name(&mut socket, "pAiDbOt").await,
        (1, String::new())
    );
    assert_eq!(a.store.presentation(account).await.unwrap().credits, 600);
    a.world
        .for_player(id)
        .call(move |w| {
            assert_eq!(w.players[&id].character.name, "pAiDbOt");
            assert_eq!(w.players[&id].presentation.credits, 600);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        native_name(&mut socket, names[1]).await,
        (0, "Not enough Gold.".into())
    );
    assert_eq!(
        native_name(&mut socket, "no1").await,
        (0, "names must be 3–10 letters".into())
    );
    assert_eq!(
        native_name(&mut socket, "TAKENNAME").await,
        (0, "name already taken".into())
    );
    assert_eq!(a.store.presentation(account).await.unwrap().credits, 600);
    assert!(a.owners.lock().unwrap().contains_key(&protected.account));
    assert_eq!(
        a.store.load(paid.account, paid.id).await.unwrap().name,
        "Paidbot"
    );
    assert!(a.kick(account).await.unwrap());
    let guest = a.store.list("nameguest".into()).await.unwrap().id;
    let guest_ch = a
        .store
        .create(guest, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut guest_socket = NativeSocket::connect(port, &a.origin).await;
    socket_load(&mut guest_socket, &dir, "nameguest", guest_ch.id).await;
    assert_eq!(
        native_name(&mut guest_socket, names[1]).await,
        (0, "register before choosing a name".into())
    );
    assert!(a.owners.lock().unwrap().contains_key(&protected.account));
    assert!(a.kick(guest).await.unwrap());
    a.stop_persona(protected.account).await.unwrap();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    let saved = reopened.load(account, ch.id).await.unwrap();
    assert_eq!(saved.name, "pAiDbOt");
    assert_eq!(reopened.presentation(account).await.unwrap().credits, 600);
}

#[tokio::test]
async fn name_claims_http_authentication_validation_and_paid_authority_have_no_bot_side_effects() {
    let (_dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let (bot, _) = running(&a, name).await;
    let human = registered(&a, "invalidhuman").await;
    assert!(
        request(&a, "unknown", name)
            .await
            .contains("invalid email or password")
    );
    assert!(
        request(&a, "invalidhuman", "no1")
            .await
            .contains("names must be")
    );
    assert!(a.owners.lock().unwrap().contains_key(&bot.account));
    a.store
        .choose_name(human, "Firsthuman".into())
        .await
        .unwrap();
    assert!(
        request(&a, "invalidhuman", name)
            .await
            .contains("Paid name changes must be made in game.")
    );
    assert!(a.owners.lock().unwrap().contains_key(&bot.account));
    assert_eq!(a.store.presentation(human).await.unwrap().credits, 100);
    a.stop_persona(bot.account).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_claims_competing_humans_commit_exactly_one_and_stale_bot_admission_is_refused() {
    let (_dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let bot = persona(&a, name).await;
    registered(&a, "racerone").await;
    registered(&a, "racertwo").await;
    let upper = name.to_ascii_uppercase();
    let lower = name.to_ascii_lowercase();
    let (one, two) = tokio::join!(
        request(&a, "racerone", &upper),
        request(&a, "racertwo", &lower)
    );
    assert_eq!(
        [&one, &two]
            .into_iter()
            .filter(|r| *r == "<Success/>")
            .count(),
        1
    );
    assert!(
        [&one, &two]
            .into_iter()
            .any(|r| r.contains("name already taken"))
    );
    let owner = a.claim(bot.account).unwrap();
    let error = crate::bots::start_test(&a, bot.clone(), -2, Pos { x: 77.5, y: 72.5 }, name, owner)
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<crate::store::Rejected>().unwrap().0,
        "persona name is already in use"
    );
    assert!(!a.owners.lock().unwrap().contains_key(&bot.account));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_claims_admission_race_never_publishes_an_ambiguous_name() {
    let (_dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let bot = persona(&a, name).await;
    registered(&a, "admissionhuman").await;
    let owner = a.claim(bot.account).unwrap();
    let (reply, admission) = tokio::join!(
        request(&a, "admissionhuman", name),
        crate::bots::start_test(&a, bot.clone(), -2, Pos { x: 77.5, y: 72.5 }, name, owner)
    );
    match admission {
        Ok(()) => {
            if reply != "<Success/>" {
                assert!(reply.contains("Persona admission is busy"), "{reply}");
                assert_eq!(request(&a, "admissionhuman", name).await, "<Success/>");
            }
        }
        Err(error) => {
            assert_eq!(reply, "<Success/>");
            assert_eq!(
                error.downcast_ref::<crate::store::Rejected>().unwrap().0,
                "persona name is already in use"
            );
        }
    }
    assert!(!a.owners.lock().unwrap().contains_key(&bot.account));
    a.world
        .directory(move |d| {
            assert!(!d.worlds.values().any(|w| {
                w.players
                    .values()
                    .any(|p| p.character.account == bot.account)
            }));
            Ok(())
        })
        .await
        .unwrap();
}

struct Release(Option<std::sync::mpsc::Sender<()>>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_claims_wait_for_accepted_inventory_progress_and_departure_save() {
    let (dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let (bot, id) = running(&a, name).await;
    let human = registered(&a, "heldhuman").await;
    let (entered_inventory, inventory_ready) = tokio::sync::oneshot::channel();
    let (inventory_release, inventory_wait) = std::sync::mpsc::channel();
    let (entered_save, save_ready) = tokio::sync::oneshot::channel();
    let (save_release, save_wait) = std::sync::mpsc::channel();
    let character = bot.id;
    a.store
        .lifecycle_test_job(move |db| {
            let mut gates = std::collections::VecDeque::from([
                (entered_inventory, inventory_wait),
                (entered_save, save_wait),
            ]);
            db.update_hook(Some(
                move |action: rusqlite::hooks::Action, _: &str, table: &str, row: i64| {
                    if action == rusqlite::hooks::Action::SQLITE_UPDATE
                        && table == "characters"
                        && row == i64::from(character)
                        && let Some((entered, release)) = gates.pop_front()
                    {
                        entered.send(()).unwrap();
                        release.recv().unwrap();
                    }
                },
            ));
            Ok(())
        })
        .await
        .unwrap();
    let inventory_release = Release(Some(inventory_release));
    let save_release = Release(Some(save_release));
    a.world
        .for_player(id)
        .call(move |w| {
            let p = w.players.get_mut(&id).unwrap();
            p.character.exp = 49;
            p.queue_experience(1, None, false)?;
            Ok(())
        })
        .await
        .unwrap();
    let app = a.clone();
    let kind = bot.items[0];
    let inventory = tokio::spawn(async move {
        lifecycle::player(
            &app,
            id,
            "name_claim_accepted_inventory",
            move |a| async move {
                let plan = a
                    .world
                    .for_player(id)
                    .call(move |w| {
                        w.drop_item(
                            id,
                            Slot {
                                object: id,
                                slot: 0,
                                kind,
                            },
                        )
                    })
                    .await?;
                persist(&a, plan).await
            },
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), inventory_ready)
        .await
        .unwrap()
        .unwrap();
    let app = a.clone();
    let claim = tokio::spawn(async move { request(&app, "heldhuman", name).await });
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    let human_name = || {
        db.query_row("SELECT name FROM accounts WHERE id=?1", [human], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
    };
    assert_eq!(human_name(), format!("Guest{human}"));
    assert!(!claim.is_finished());
    drop(inventory_release);
    inventory.await.unwrap().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), save_ready)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(human_name(), format!("Guest{human}"));
    assert!(!claim.is_finished());
    assert!(a.owners.lock().unwrap().contains_key(&bot.account));
    a.world
        .for_player(id)
        .call(move |w| {
            assert!(w.players[&id].departing);
            Ok(())
        })
        .await
        .unwrap();
    drop(save_release);
    assert_eq!(claim.await.unwrap(), "<Success/>");
    let saved = a.store.load(bot.account, bot.id).await.unwrap();
    assert_eq!(saved.items[0], -1);
    assert_eq!(saved.revision, bot.revision + 1);
    assert_eq!(saved.level, 2);
    assert_eq!(
        a.store.vaults(bot.account).await.unwrap()[0].items,
        vec![0xa1f; 8]
    );
    assert!(!a.owners.lock().unwrap().contains_key(&bot.account));
    let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
    assert_eq!(
        reopened.load(bot.account, bot.id).await.unwrap().items[0],
        -1
    );
    assert_eq!(reopened.list("heldhuman".into()).await.unwrap().name, name);
}

#[tokio::test]
async fn name_claims_save_and_claim_failures_are_visible_without_human_publication_or_charge() {
    for failing_table in ["bot_minds", "characters", "accounts"] {
        let (dir, a) = test_app();
        a.world.clock.set_paused(true);
        let name = crate::bots::profiles()[0];
        let (bot, _) = running(&a, name).await;
        let human = registered(&a, "failurehuman").await;
        let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
        let (column, id) = if failing_table == "accounts" {
            ("id", human)
        } else if failing_table == "characters" {
            ("id", bot.id)
        } else {
            ("account", bot.account)
        };
        db.execute_batch(&format!("CREATE TRIGGER fail_name_claim BEFORE UPDATE ON {failing_table} WHEN OLD.{column}={id} BEGIN SELECT RAISE(ABORT,'injected {failing_table} claim failure'); END;")).unwrap();
        let reply = request(&a, "failurehuman", name).await;
        assert!(reply.starts_with("<Error>"), "{reply}");
        assert!(
            reply.contains(&format!("injected {failing_table} claim failure")),
            "{reply}"
        );
        let (saved_name, credits): (String, i32) = db
            .query_row(
                "SELECT name,credits FROM accounts WHERE id=?1",
                [human],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(saved_name, format!("Guest{human}"));
        assert_eq!(credits, 100);
        if failing_table != "accounts" {
            assert!(
                lifecycle::state(&a, bot.account).unwrap().2,
                "failed save must retain the account fence"
            );
        }
        let reopened = Store::open(&dir.join("game.db"), &a.content).unwrap();
        assert_eq!(
            reopened.list("failurehuman".into()).await.unwrap().name,
            format!("Guest{human}")
        );
        a.shutdown.send_replace(true);
    }
}

#[tokio::test]
async fn name_claims_pending_permanent_death_settles_without_replacement_or_inventory_transfer() {
    let (dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let (bot, id) = running(&a, name).await;
    let human = registered(&a, "deathhuman").await;
    let human_ch = a
        .store
        .create(human, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    a.world
        .for_player(id)
        .call(move |w| {
            let p = w.players.get_mut(&id).unwrap();
            p.character.hp = 0;
            p.death_pending = true;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(request(&a, "deathhuman", name).await, "<Success/>");
    assert!(
        !a.store
            .source_character_alive(bot.account, bot.id)
            .await
            .unwrap()
    );
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1",
            [bot.account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE character=?1",
            [bot.id],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        a.store.load(human, human_ch.id).await.unwrap().items,
        human_ch.items
    );
    assert_eq!(
        a.store.vaults(bot.account).await.unwrap()[0].items,
        vec![0xa1f; 8]
    );
}
