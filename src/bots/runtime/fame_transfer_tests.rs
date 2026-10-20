use super::*;

struct TransferFixture {
    _directory: std::path::PathBuf,
    app: App,
    profile: Profile,
    owner: Option<Owner>,
    entry: Entry,
}
impl TransferFixture {
    async fn new(template: &'static str) -> Self {
        let (directory, app) = server::lifecycle_test_fixture();
        app.world.clock.set_paused(true);
        let profile = PROFILES[0];
        let character = app
            .store
            .shore_character(
                profile.identity.into(),
                profile.name.into(),
                &app.content,
                profile.healer,
            )
            .await
            .unwrap();
        let account = character.account;
        let mut owner = app.claim(account).unwrap();
        owner.arm_cleanup(&app, tokio::runtime::Handle::current());
        let world = app
            .world
            .directory(move |d| d.instance(template, None))
            .await
            .unwrap();
        let entry = lifecycle::account(
            &app,
            vec![account],
            "phase4_autonomous_fixture",
            move |app| async move { enter(&app, profile, character, world, None, None).await },
        )
        .await
        .unwrap();
        Self {
            _directory: directory,
            app,
            profile,
            owner: Some(owner),
            entry,
        }
    }
    async fn issue(
        &self,
    ) -> (
        i32,
        i32,
        Vec<u8>,
        crate::world::TransferReservation,
        crate::world::view::Session,
    ) {
        let id = self.entry.0;
        self.app
            .world
            .directory(move |d| {
                let session = d.player_view(id)?.session;
                let w = d.worlds.get_mut(&session.world).unwrap();
                // A prepared cleared-dungeon portal; no native boss-clear/history claim.
                let kind = if w.name == "Abyss of Demons" {
                    0x71c
                } else {
                    0x704
                };
                let portal = w.spawn_entity(kind, w.players[&id].pos, vec![])?;
                let (account, character, destination) = d.destination(id, Some(portal))?;
                let (time, key, _) = d.issue(account, character, destination)?;
                d.bind_exit(id, Some(portal), &key)?;
                let reservation = d.reserve_transfer(key.clone())?;
                Ok((destination, time, key, reservation, session))
            })
            .await
            .unwrap()
    }
    async fn close(mut self, id: Option<i32>) {
        if let Some(id) = id {
            lifecycle::finalize(&self.app, id, CloseReason::Controller)
                .unwrap()
                .wait()
                .await
                .unwrap();
        }
        if let Some(finalization) = self
            .owner
            .take()
            .unwrap()
            .close(&self.app, CloseReason::Controller)
            .unwrap()
        {
            finalization.wait().await.unwrap();
        }
        assert_eq!(self.app.store.lifecycle_test_jobs(), 0);
        assert!(self.app.owners.lock().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_death_fame_phase4_autonomous_earned_exits() {
    for (template, dungeon) in [
        ("snake-pit", crate::dungeon::HistoricalDungeon::SnakePit),
        (
            "abyss-of-demons",
            crate::dungeon::HistoricalDungeon::AbyssOfDemons,
        ),
        (
            "forbidden-jungle",
            crate::dungeon::HistoricalDungeon::ForbiddenJungle,
        ),
        (
            "manor-of-the-immortals",
            crate::dungeon::HistoricalDungeon::ManorOfTheImmortals,
        ),
    ] {
        let mut f = TransferFixture::new(template).await;
        let (destination, time, key, reservation, session) = f.issue().await;
        let (account, character) = (session.account, session.character);
        lifecycle::finalize_transfer(&f.app, f.entry.0, reservation)
            .unwrap()
            .wait()
            .await
            .unwrap();
        let new = reenter(
            &f.app,
            f.profile,
            account,
            &mut f.owner,
            Reentry::Portal {
                destination,
                time,
                key: key.clone(),
                session,
                selected: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(new.3.session.2, character);
        assert!(
            f.app
                .store
                .load(account, character)
                .await
                .unwrap()
                .lifetime
                .realm_exits
                .contains(&dungeon)
        );
        assert!(
            f.app
                .world
                .directory(move |d| d.consume(account, destination, time, key))
                .await
                .is_err()
        );
        f.close(Some(new.0)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_death_fame_phase4_autonomous_committed_death_not_grant_fallback() {
    for selected in [false, true] {
        let mut f = TransferFixture::new("snake-pit").await;
        let (destination, time, key, reservation, session) = f.issue().await;
        let (account, character, id) = (session.account, session.character, f.entry.0);
        // Prepared terminal HP after valid intent, before canonical source-write acceptance.
        f.app
            .world
            .for_player(id)
            .call(move |w| {
                let p = w.players.get_mut(&id).unwrap();
                p.character.hp = 0;
                p.death_pending = true;
                Ok(())
            })
            .await
            .unwrap();
        lifecycle::finalize_transfer(&f.app, id, reservation)
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert!(
            !f.app
                .store
                .source_character_alive(account, character)
                .await
                .unwrap()
        );
        let db = rusqlite::Connection::open(f._directory.join("game.db")).unwrap();
        let receipt: String = db
            .query_row(
                "SELECT snapshot FROM deaths WHERE character=?",
                [character],
                |r| r.get(0),
            )
            .unwrap();
        let dead: Character = serde_json::from_str(&receipt).unwrap();
        assert!(dead.lifetime.realm_exits.is_empty());
        let result = reenter(
            &f.app,
            f.profile,
            account,
            &mut f.owner,
            Reentry::Portal {
                destination,
                time,
                key,
                session,
                selected: selected.then_some(character),
            },
        )
        .await;
        if selected {
            assert!(result.is_err());
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM characters WHERE account=?",
                    [account],
                    |r| r.get::<_, i32>(0)
                )
                .unwrap(),
                1
            );
            f.close(None).await;
        } else {
            let new = result.unwrap();
            let replacement = new.3.session.2;
            assert_ne!(replacement, character);
            let ch = f.app.store.load(account, replacement).await.unwrap();
            assert_eq!(ch.level, 1);
            assert!(ch.lifetime.realm_exits.is_empty());
            assert_eq!(
                f.app
                    .world
                    .directory(move |d| Ok(d.player_view(new.0)?.map.name.clone()))
                    .await
                    .unwrap(),
                "Realm"
            );
            f.close(Some(new.0)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_death_fame_phase4_autonomous_bad_grant_and_collective_binding() {
    for bad_grant in [false, true] {
        let mut f = TransferFixture::new("snake-pit").await;
        let (destination, time, key, reservation, session) = f.issue().await;
        let (account, character, id) = (session.account, session.character, f.entry.0);
        if bad_grant {
            drop(reservation);
            lifecycle::finalize(&f.app, id, CloseReason::Controller)
                .unwrap()
                .wait()
                .await
                .unwrap();
        } else {
            lifecycle::finalize_transfer(&f.app, id, reservation)
                .unwrap()
                .wait()
                .await
                .unwrap();
        }
        let request = if bad_grant {
            Reentry::Portal {
                destination,
                time,
                key,
                session,
                selected: None,
            }
        } else {
            Reentry::Collective {
                destination,
                time,
                key,
                character: character + 1,
            }
        };
        assert!(
            reenter(&f.app, f.profile, account, &mut f.owner, request)
                .await
                .is_err()
        );
        let db = rusqlite::Connection::open(f._directory.join("game.db")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM characters WHERE account=?",
                [account],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            1
        );
        assert!(
            f.app
                .world
                .directory(move |d| Ok(d
                    .worlds
                    .values()
                    .all(|w| w.players.values().all(|p| p.character.account != account))))
                .await
                .unwrap()
        );
        assert_eq!(
            f.app
                .store
                .load(account, character)
                .await
                .unwrap()
                .lifetime
                .realm_exits
                .is_empty(),
            bad_grant
        );
        let state = lifecycle::state(&f.app, account).unwrap();
        assert!(
            state.1.is_none(),
            "invalid transfer published a ghost actor"
        );
        assert_eq!(
            state.2, !bad_grant,
            "protocol rejection is clean; accepted collective identity mismatch must fence"
        );
        if bad_grant {
            f.close(None).await;
        } else {
            drop(f.owner.take());
            assert!(f.app.claim(account).is_err());
            assert_eq!(f.app.store.lifecycle_test_jobs(), 0);
        }
    }
}
