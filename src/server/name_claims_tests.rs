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

async fn registered_name(store: &Store, identity: &str) -> String {
    store
        .characters(format!("{identity}@example.test"), "test-password".into())
        .await
        .unwrap()
        .name
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
    assert_eq!(registered_name(&reopened, "human").await, "kEePeR");
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
    let human = registered(&a, "activehuman").await;
    let cleared_before_publication = Arc::new(AtomicBool::new(false));
    let observed = cleared_before_publication.clone();
    let cctv = a.cctv.clone();
    a.store
        .lifecycle_test_job(move |db| {
            db.update_hook(Some(
                move |action: rusqlite::hooks::Action, _: &str, table: &str, row: i64| {
                    if action == rusqlite::hooks::Action::SQLITE_UPDATE
                        && table == "accounts"
                        && row == i64::from(human)
                    {
                        observed.store(cctv.binding(name).is_none(), Ordering::SeqCst);
                    }
                },
            ));
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(request(&a, "activehuman", name).await, "<Success/>");
    assert!(cleared_before_publication.load(Ordering::SeqCst));
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
    let binding = a.cctv.binding(names[0]).unwrap();
    assert_eq!(binding.1, first.account);
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
    assert_eq!(a.cctv.binding(names[0]), None);
    assert!(
        a.world
            .spectate(binding.0, binding.1)
            .await
            .unwrap()
            .is_none()
    );
    let error = a.world.spectate(id, account).await.err().unwrap();
    assert!(error.to_string().contains("not an autonomous bot"));
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
    assert_eq!(registered_name(&reopened, "heldhuman").await, name);
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
            registered_name(&reopened, "failurehuman").await,
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

#[tokio::test]
async fn name_claims_human_owner_controls_social_and_guild_resolution() {
    use crate::store::GuildAction;
    let (_dir, a) = test_app();
    a.world.clock.set_paused(true);
    let archived = persona(&a, "Keeper").await;
    a.store
        .retire_persona("persona:v1:Keeper".into(), archived.account)
        .await
        .unwrap();
    let current = persona(&a, "Keeper").await;
    assert_eq!(
        a.store.account_named("KEEPER".into()).await.unwrap(),
        archived.account
    );
    let human = registered(&a, "nameowner").await;
    let founder = registered(&a, "namefounder").await;
    assert_eq!(request(&a, "nameowner", "Keeper").await, "<Success/>");
    let target = a.store.account_named("kEePeR".into()).await.unwrap();
    assert_eq!(target, human);
    let lists = a
        .store
        .edit_account_list(founder, 1, true, target)
        .await
        .unwrap();
    assert_eq!(lists[1], std::collections::BTreeSet::from([human]));
    a.store
        .create_prepared_guild(vec![founder, human, current.account], "Name Owners".into())
        .await
        .unwrap();
    assert!(
        a.store
            .change_guild(
                human,
                GuildAction::Rank {
                    name: "KEEPER".into(),
                    rank: 10
                }
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot make")
    );
    let changed = a
        .store
        .change_guild(
            founder,
            GuildAction::Rank {
                name: "KEEPER".into(),
                rank: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(changed.1, human);
    assert_eq!(
        a.store
            .presentation(human)
            .await
            .unwrap()
            .guild
            .unwrap()
            .rank,
        10
    );
    assert_eq!(
        a.store
            .presentation(current.account)
            .await
            .unwrap()
            .guild
            .unwrap()
            .rank,
        0
    );
    assert!(
        a.store
            .change_guild(current.account, GuildAction::Remove("Keeper".into()))
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot remove")
    );
    let outsider = registered(&a, "nameoutsider").await;
    a.store
        .create_prepared_guild(vec![outsider], "Other Owners".into())
        .await
        .unwrap();
    assert!(
        a.store
            .change_guild(outsider, GuildAction::Remove("Keeper".into()))
            .await
            .unwrap_err()
            .to_string()
            .contains("cannot remove")
    );
    assert_eq!(
        a.store
            .change_guild(founder, GuildAction::Remove("keeper".into()))
            .await
            .unwrap()
            .1,
        human
    );
    assert!(a.store.presentation(human).await.unwrap().guild.is_none());
    assert!(
        a.store
            .presentation(current.account)
            .await
            .unwrap()
            .guild
            .is_some()
    );
}

#[tokio::test]
async fn name_claims_ambiguous_human_owners_are_errors_not_arbitrary_social_or_guild_targets() {
    use crate::store::GuildAction;
    let (_dir, a) = test_app();
    let first = registered(&a, "ambiguousone").await;
    let second = registered(&a, "ambiguoustwo").await;
    let founder = registered(&a, "ambiguousfounder").await;
    a.store
        .choose_name(first, "Duplicate".into())
        .await
        .unwrap();
    a.store.rename(second, "dUpLiCaTe".into()).await.unwrap();
    assert!(
        a.store
            .account_named("DUPLICATE".into())
            .await
            .unwrap_err()
            .to_string()
            .contains("Ambiguous human name ownership")
    );
    a.store
        .create_prepared_guild(vec![founder, first, second], "Ambiguous Owners".into())
        .await
        .unwrap();
    for action in [
        GuildAction::Rank {
            name: "Duplicate".into(),
            rank: 10,
        },
        GuildAction::Remove("Duplicate".into()),
    ] {
        assert!(
            a.store
                .change_guild(founder, action)
                .await
                .unwrap_err()
                .to_string()
                .contains("Ambiguous human name ownership")
        );
    }
    for id in [first, second] {
        assert_eq!(
            a.store.presentation(id).await.unwrap().guild.unwrap().rank,
            0
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_claims_wait_for_an_accepted_autonomous_transfer_before_releasing_the_name() {
    let (dir, a) = test_app();
    a.world.clock.set_paused(true);
    let name = crate::bots::profiles()[0];
    let bot = persona(&a, name).await;
    let human = registered(&a, "transferhuman").await;
    let (source, position) = a
        .world
        .directory(|d| {
            let source = d.instance(
                "snake-pit",
                None,
                Some(crate::world::DungeonReturn::Realm(1)),
            )?;
            let w = d.worlds.get_mut(&source).unwrap();
            let (x, y) = w.map.spawns[0];
            let position = Pos {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            w.spawn_entity(0x71d, position, vec![])?;
            Ok((source, position))
        })
        .await
        .unwrap();
    let owner = a.claim(bot.account).unwrap();
    crate::bots::start_test(&a, bot.clone(), source, position, name, owner)
        .await
        .unwrap();
    let (player, account) = a.cctv.binding(name).unwrap();
    let (entered, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let character = bot.id;
    a.store
        .lifecycle_test_job(move |db| {
            let mut entered = Some(entered);
            db.update_hook(Some(
                move |action: rusqlite::hooks::Action, _: &str, table: &str, row: i64| {
                    if action == rusqlite::hooks::Action::SQLITE_UPDATE
                        && table == "characters"
                        && row == i64::from(character)
                        && let Some(entered) = entered.take()
                    {
                        entered.send(()).unwrap();
                        wait.recv().unwrap();
                    }
                },
            ));
            Ok(())
        })
        .await
        .unwrap();
    let release = Release(Some(release));
    a.world.clock.set_paused(false);
    tokio::time::timeout(std::time::Duration::from_secs(10), ready)
        .await
        .unwrap()
        .unwrap();
    a.world.clock.set_paused(true);
    a.world
        .directory(move |d| {
            assert!(d.worlds[&source].players[&player].departing);
            assert!(d.worlds.values().map(|w| w.reserved_slots()).sum::<usize>() > 0);
            Ok(())
        })
        .await
        .unwrap();
    let app = a.clone();
    let claim = tokio::spawn(async move { request(&app, "transferhuman", name).await });
    let db = rusqlite::Connection::open(dir.join("game.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT name FROM accounts WHERE id=?1", [human], |r| r
            .get::<_, String>(
            0
        ))
        .unwrap(),
        format!("Guest{human}")
    );
    assert!(!claim.is_finished());
    drop(release);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), claim)
            .await
            .unwrap()
            .unwrap(),
        "<Success/>"
    );
    assert_eq!(a.cctv.binding(name), None);
    a.world
        .directory(move |d| {
            assert!(
                !d.worlds
                    .values()
                    .any(|w| w.players.values().any(|p| p.character.account == account))
            );
            assert_eq!(
                d.worlds.values().map(|w| w.reserved_slots()).sum::<usize>(),
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    assert!(a.store.load(bot.account, bot.id).await.unwrap().alive);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account=?1",
            [account],
            |r| r.get::<_, i32>(0)
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn name_claims_unpublished_persona_admission_rejects_without_fencing_the_human() {
    let (_dir, a) = test_app();
    let bot = persona(&a, "Admitting").await;
    let owner = a.claim(bot.account).unwrap();
    let human = registered(&a, "admissionretry").await;
    let reply = request(&a, "admissionretry", "Admitting").await;
    assert!(reply.contains("Persona admission is busy"), "{reply}");
    assert!(
        lifecycle::state(&a, human).is_none(),
        "a rejected name request cannot fence its human owner"
    );
    assert_eq!(
        registered_name(&a.store, "admissionretry").await,
        format!("Guest{human}")
    );
    drop(owner);
    assert_eq!(
        request(&a, "admissionretry", "Admitting").await,
        "<Success/>"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_claims_native_unpublished_admission_rejects_without_poisoning_the_human() {
    use lifecycle_finalize_tests::*;
    let (dir, a) = test_app();
    a.world.clock.set_paused(true);
    let bot = persona(&a, "Admitting").await;
    let owner = a.claim(bot.account).unwrap();
    let human = registered(&a, "nativeadmission").await;
    let character = a
        .store
        .create(human, a.content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let (port, server) = socket_server(&a).await;
    let mut socket = NativeSocket::connect(port, &a.origin).await;
    socket
        .send(socket_hello_password(
            &dir,
            "nativeadmission@example.test",
            "test-password",
            -2,
            None,
        ))
        .await;
    socket.receive(wire::MAP_INFO).await;
    socket
        .send(Packet {
            id: wire::LOAD,
            body: character.id.to_be_bytes().to_vec(),
        })
        .await;
    let id = Reader::new(&socket.receive(wire::CREATE_SUCCESS).await.body)
        .i32()
        .unwrap();
    let mut body = Writer::new();
    body.utf("Admitting");
    socket
        .send(Packet {
            id: 33,
            body: body.0,
        })
        .await;
    let packet = socket.receive_one_of(&[0, 20]).await;
    let mut reader = Reader::new(&packet.body);
    let status = if packet.id == 0 {
        reader.i32().unwrap()
    } else {
        i32::from(reader.u8().unwrap())
    };
    let reason = reader.utf().unwrap();
    reader.done().unwrap();
    assert_eq!(packet.id, 20, "status={status}, reason={reason}");
    assert_eq!(status, 0);
    assert_eq!(
        reason,
        "Persona admission is busy; retry name choice shortly"
    );
    assert!(!lifecycle::state(&a, human).unwrap().2);
    a.world
        .for_player(id)
        .call(move |w| {
            let p = &w.players[&id];
            assert!(
                p.character.alive && !p.pending && !p.departing && p.persistence_failure.is_none()
            );
            Ok(())
        })
        .await
        .unwrap();
    drop(owner);
    assert_eq!(
        native_name(&mut socket, "Admitting").await,
        (1, String::new())
    );
    assert_eq!(
        a.store.account_named("ADMITTING".into()).await.unwrap(),
        human
    );
    assert_eq!(a.store.presentation(human).await.unwrap().credits, 100);
    assert!(a.kick(human).await.unwrap());
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
