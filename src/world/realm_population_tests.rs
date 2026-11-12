use super::*;

fn directory(limit: usize) -> Directory {
    let data = std::path::Path::new("data");
    Directory::test_with_seed(data, Arc::new(Content::load(data).unwrap()), limit, 1344).unwrap()
}

#[test]
fn pending_allocation_serializes_repeated_demand_and_reports_worker_failure() {
    let mut d = directory(1);
    d.issue(1, 1, -3).unwrap();
    d.issue(2, 2, 1).unwrap();
    let id = d.next_world;
    d.next_world += 1;
    let world = d.make("realm-shore", "Realm", false, None).unwrap();
    let (sender, receiver) = oneshot::channel();
    d.realm_population.preparing = Some((id, receiver));
    for _ in 0..10 {
        d.coordinator_step();
    }
    assert_eq!(d.current_realm_count(), 2);
    assert_eq!(d.next_world, id + 1);
    sender.send(Ok(world)).ok().unwrap();
    d.coordinator_step();
    assert_eq!(d.current_realm_count(), 3);
    assert!(d.realm_population.preparing.is_none());
    let (sender, receiver) = oneshot::channel();
    d.realm_population.preparing = Some((d.next_world, receiver));
    drop(sender);
    assert_eq!(
        d.update_realm_population(d.clock.now())
            .unwrap_err()
            .to_string(),
        "Realm allocation worker stopped without a result"
    );
    assert_eq!(d.current_realm_count(), 3);
}

#[test]
fn initial_entry_holds_block_retirement_handoff_to_presence_and_release_on_error() {
    let mut d = directory(2);
    let (_, key, _) = d.issue(1, 1, 1).unwrap();
    let t = d.clock.now();
    d.realm_population_step(t);
    let first = d.hold_initial_entry(-3).unwrap().unwrap();
    let second = d.hold_initial_entry(-3).unwrap().unwrap();
    assert_eq!(d.realm_dependencies(-3), 2);
    drop(first);
    assert_eq!(d.realm_dependencies(-3), 1);
    d.realm_population_step(t + IDLE_GRACE);
    assert!(d.worlds.contains_key(&-3));
    let mut ch = Character::fresh(d.content.get(0x310).unwrap()).unwrap();
    ch.account = 2;
    ch.id = 2;
    let player = d.worlds.get_mut(&-3).unwrap().enter(ch, None).unwrap();
    drop(second);
    assert_eq!(d.realm_dependencies(-3), 0);
    d.realm_population_step(t + IDLE_GRACE * 2);
    assert!(
        d.worlds.contains_key(&-3),
        "successful entry hands off to real player presence"
    );
    d.worlds.get_mut(&-3).unwrap().leave(player).unwrap();
    let failed: Result<()> = (|| {
        let _hold = d.hold_initial_entry(-3)?.unwrap();
        bail!("simulated pre-entry Store error")
    })();
    assert!(failed.is_err());
    assert_eq!(d.realm_dependencies(-3), 0);
    d.cancel_admission(1, key).unwrap();
    d.realm_population_step(t + IDLE_GRACE * 2);
    d.realm_population_step(t + IDLE_GRACE * 3);
    assert!(!d.worlds.contains_key(&-3));
    assert!(d.hold_initial_entry(-2).unwrap().is_none());
    assert!(d.hold_initial_entry(i32::MAX).is_err());
}

#[tokio::test]
async fn initial_selection_and_hold_are_atomic_with_coordinator_retirement() {
    let mut d = directory(1);
    d.clock.set_paused(true);
    let (_, key, _) = d.issue(1, 1, 1).unwrap();
    let t = d.clock.now();
    d.realm_population_step(t);
    let h = Handle::spawn(d);
    let hold = h
        .directory(move |d| {
            let id = d.initial(-3)?;
            assert_eq!(id, -3);
            let hold = d.hold_initial_entry(id)?.unwrap();
            d.cancel_admission(1, key)?;
            Ok(hold)
        })
        .await
        .unwrap();
    h.directory(move |d| {
        d.realm_population_step(t + IDLE_GRACE * 2);
        assert_eq!(
            d.endgame.realms[&-3]
                .initial_entries
                .load(Ordering::Acquire),
            1
        );
        assert!(d.worlds.contains_key(&-3));
        Ok(())
    })
    .await
    .unwrap();
    let waiting = std::future::pending::<()>();
    let mut cancelled = Box::pin(async move {
        let _hold = hold;
        waiting.await;
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(1), &mut cancelled)
            .await
            .is_err()
    );
    drop(cancelled);
    h.directory(move |d| {
        assert_eq!(
            d.endgame.realms[&-3]
                .initial_entries
                .load(Ordering::Acquire),
            0
        );
        d.realm_population_step(t + IDLE_GRACE * 2);
        d.realm_population_step(t + IDLE_GRACE * 3);
        assert!(!d.worlds.contains_key(&-3));
        Ok(())
    })
    .await
    .unwrap();
}

#[test]
fn entrance_lease_requires_every_closed_hidden_condition_and_valid_metadata() {
    let mut d = directory(2);
    let portal = d.endgame.realms[&-3].portal.unwrap();
    assert!(d.realm_entrance_reserves_position(portal).unwrap());
    d.worlds.get_mut(&-3).unwrap().admission_open = false;
    d.sync_realm_portal();
    assert!(
        d.realm_entrance_reserves_position(portal).unwrap(),
        "hidden open cycle still reserves"
    );
    let cycle = &mut d.endgame.realms.get_mut(&-3).unwrap().cycle;
    for root in cycle.remaining.clone() {
        assert!(cycle.defeated(root, 0));
    }
    assert!(!d.realm_entrance_reserves_position(portal).unwrap());
    assert_ne!(
        d.unused_realm_name(Some(-3)).unwrap(),
        d.endgame.realms[&-3].name
    );
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .entities
        .get_mut(&portal)
        .unwrap()
        .published = true;
    assert!(
        d.realm_entrance_reserves_position(portal).unwrap(),
        "published closed entrance still reserves"
    );
    d.worlds
        .get_mut(&-2)
        .unwrap()
        .entities
        .get_mut(&portal)
        .unwrap()
        .published = false;
    d.endgame.realms.get_mut(&-3).unwrap().enabled = false;
    assert!(
        d.realm_entrance_reserves_position(portal).unwrap(),
        "unpublished successor still reserves"
    );
    d.endgame.realms.get_mut(&-3).unwrap().enabled = true;
    d.worlds.get_mut(&-3).unwrap().admission_open = true;
    assert!(
        d.realm_entrance_reserves_position(portal).unwrap(),
        "world admission must actually be closed"
    );
    d.worlds.get_mut(&-3).unwrap().admission_open = false;
    let new = d.install_realm_portal(0).unwrap();
    assert!(
        d.realm_entrance_reserves_position(new).unwrap(),
        "unbound hidden new reservation still reserves"
    );
    d.worlds.remove(&-3);
    assert!(
        d.realm_entrance_reserves_position(portal)
            .unwrap_err()
            .to_string()
            .contains("bound Realm world missing")
    );
    d.worlds.get_mut(&-2).unwrap().remove_entity(portal);
    assert!(
        d.realm_entrance_reserves_position(portal)
            .unwrap_err()
            .to_string()
            .contains("entrance entity missing")
    );
}

#[test]
fn closed_hidden_predecessors_release_space_without_deleting_owned_entities() {
    let data = std::path::Path::new("data");
    let content = Arc::new(Content::load(data).unwrap());
    let mut d = Directory::prepared(data, content, 16, 1344, 8).unwrap();
    let origins: Vec<_> = d.endgame.realms.keys().copied().collect();
    let mut holds = Vec::new();
    let mut bindings = Vec::new();
    for (i, origin) in origins.iter().enumerate() {
        let mut ch = Character::fresh(d.content.get(0x310).unwrap()).unwrap();
        ch.account = i as i32 + 1;
        ch.id = i as i32 + 1;
        d.worlds.get_mut(origin).unwrap().enter(ch, None).unwrap();
        d.issue(i as i32 + 1, i as i32 + 1, -2).unwrap();
        holds.push(d.hold_initial_entry(*origin).unwrap().unwrap());
        let record = d.endgame.realms.get_mut(origin).unwrap();
        for root in record.cycle.remaining.clone() {
            assert!(record.cycle.defeated(root, 0));
        }
        d.worlds.get_mut(origin).unwrap().admission_open = false;
        bindings.push((*origin, record.portal.unwrap()));
    }
    d.sync_realm_portal();
    let mut fresh = Vec::new();
    for slot in 0..8 {
        let id = d
            .install_realm_portal(slot)
            .expect("eight closed hidden predecessors must not exhaust successor spatial leases");
        let pos = d.worlds[&-2].entities[&id].pos;
        assert!(
            fresh
                .iter()
                .all(|id| d.worlds[&-2].entities[id].pos.distance(pos) >= 4.)
        );
        fresh.push(id);
    }
    for (origin, portal) in bindings {
        assert_eq!(d.endgame.realms[&origin].portal, Some(portal));
        assert_eq!(d.worlds[&-2].entities[&portal].kind, 0x712);
        assert!(!d.worlds[&-2].entities[&portal].published);
        assert_eq!(d.worlds[&origin].players.len(), 1);
        assert_eq!(
            d.endgame.realms[&origin]
                .initial_entries
                .load(Ordering::Acquire),
            1
        );
        assert_eq!(
            d.grants
                .values()
                .filter(|g| g.origin == Some(origin))
                .count(),
            1
        );
    }
    assert_eq!(holds.len(), 8);
}

#[test]
fn eight_current_slots_refuse_growth_even_when_closed_without_rerouting() {
    let mut d = directory(1);
    let mut account = 1;
    for expected in 3..=8 {
        for (id, _) in d.available_realms() {
            d.issue(account, account, id).unwrap();
            account += 1;
        }
        d.coordinator_step();
        let pending = d.realm_population.preparing.as_ref().unwrap().0;
        d.realm_population_step(d.clock.now());
        assert!(d.worlds.contains_key(&pending));
        assert_eq!(d.current_realm_count(), expected);
    }
    for (id, _) in d.available_realms() {
        d.issue(account, account, id).unwrap();
    }
    d.coordinator_step();
    assert_eq!(d.current_realm_count(), 8);
    assert!(d.realm_population.preparing.is_none());
    assert!(d.initial(-3).unwrap_err().to_string().contains("capacity"));
    let names: BTreeSet<_> = d.endgame.realms.values().map(|r| r.name).collect();
    let slots: BTreeSet<_> = d.endgame.realms.values().map(|r| r.portal_slot).collect();
    assert_eq!(names.len(), 8);
    assert_eq!(slots.len(), 8);
    for id in d.endgame.realms.keys() {
        d.worlds.get_mut(id).unwrap().admission_open = false;
    }
    d.coordinator_step();
    assert_eq!(
        d.current_realm_count(),
        8,
        "sealed worlds still occupy current slots"
    );
    assert!(d.realm_population.preparing.is_none());
}

#[test]
fn continuous_dependency_free_grace_resets_on_dungeon_return_and_reservations() {
    let mut d = directory(2);
    let (_, other_key, _) = d.issue(1, 1, 1).unwrap();
    let t = d.clock.now();
    d.realm_population_step(t);
    d.realm_population_step(t + IDLE_GRACE - Duration::from_secs(1));
    assert!(d.worlds.contains_key(&-3));
    let dungeon = d
        .instance("pirate-cave", None, Some(DungeonReturn::Realm(-3)))
        .unwrap();
    assert_eq!(d.realm_dependencies(-3), 1);
    d.realm_population_step(t + IDLE_GRACE);
    assert!(d.endgame.realms[&-3].empty_since.is_none());
    d.worlds.remove(&dungeon);
    d.realm_population_step(t + IDLE_GRACE);
    d.realm_population_step(t + IDLE_GRACE * 2 - Duration::from_secs(1));
    assert!(d.worlds.contains_key(&-3));
    let (_, key, _) = d.issue(2, 2, -3).unwrap();
    d.realm_population_step(t + IDLE_GRACE * 2);
    assert!(d.endgame.realms[&-3].empty_since.is_none());
    d.cancel_admission(2, key).unwrap();
    d.realm_population_step(t + IDLE_GRACE * 2);
    let portal = d.endgame.realms[&-3].portal.unwrap();
    d.cancel_admission(1, other_key).unwrap();
    d.realm_population_step(t + IDLE_GRACE * 3);
    assert!(!d.worlds.contains_key(&-3));
    assert!(!d.worlds[&-2].entities.contains_key(&portal));
    assert_eq!(d.current_realm_count(), 1);
    assert_eq!(d.available_realms(), vec![(1, 0)]);
    d.realm_population_step(t + IDLE_GRACE * 4);
    assert!(
        d.worlds.contains_key(&1),
        "the last open Realm must survive"
    );
}

#[test]
fn predecessor_reference_and_creation_grace_prevent_premature_retirement() {
    let mut d = directory(2);
    let (_, other_key, _) = d.issue(1, 1, 1).unwrap();
    let t = d.clock.now();
    d.endgame.realms.get_mut(&-3).unwrap().created_at = t + IDLE_GRACE;
    d.endgame.realms.get_mut(&1).unwrap().replacement = Some(-3);
    assert_eq!(d.current_realm_count(), 1);
    assert_eq!(d.realm_dependencies(-3), 1);
    d.realm_population_step(t + IDLE_GRACE);
    assert!(d.endgame.realms[&-3].empty_since.is_none());
    d.endgame.realms.get_mut(&1).unwrap().replacement = None;
    d.realm_population_step(t + IDLE_GRACE);
    d.realm_population_step(t + IDLE_GRACE * 2 - Duration::from_secs(1));
    assert!(d.worlds.contains_key(&-3));
    d.cancel_admission(1, other_key).unwrap();
    d.realm_population_step(t + IDLE_GRACE * 2);
    assert!(!d.worlds.contains_key(&-3));
    assert!(d.worlds.contains_key(&1));
}

async fn population_app() -> crate::server::App {
    let (_, mut app) = crate::server::test_app();
    let d = directory(72);
    d.clock.set_paused(true);
    app.max_players = 72;
    app.world = Handle::spawn(d);
    app
}

async fn claimed_actor(
    app: &crate::server::App,
    guid: &str,
    destination: i32,
) -> crate::server::Owner {
    let account = app.store.list(guid.into()).await.unwrap().id;
    let character = app
        .store
        .create(account, app.content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let mut owner = app.claim(account).unwrap();
    owner.arm_cleanup(app, tokio::runtime::Handle::current());
    let bind = crate::server::lifecycle::binding(app, account).unwrap();
    let presentation = app.store.presentation(account).await.unwrap();
    let legendary = app.store.is_legend(account).await.unwrap();
    app.world
        .directory(move |d| {
            let (time, key, _) = d.issue(account, character.id, destination)?;
            let (_, arrival) = d.consume(account, destination, time, key.clone())?;
            d.admit_transfer(arrival, account, character.id, destination)?;
            let world = d.worlds.get_mut(&destination).unwrap();
            assert_eq!(world.max_players, 72);
            let player = world.enter_admitted(
                character,
                None,
                None,
                presentation,
                legendary,
                Some(key.clone()),
                Controller::Human,
            )?;
            bind(player)?;
            d.complete_dungeon_entry(&key, player)?;
            Ok(())
        })
        .await
        .unwrap();
    owner
}

async fn close_actors(app: &crate::server::App, actors: Vec<crate::server::Owner>) {
    for owner in actors {
        owner
            .close(app, crate::server::lifecycle::CloseReason::Transport)
            .unwrap()
            .unwrap()
            .wait()
            .await
            .unwrap();
    }
    assert!(app.owners.lock().unwrap().is_empty());
}

async fn finish_population_build(app: &crate::server::App) {
    app.world
        .directory(|d| {
            if let Some((id, receiver)) = d.realm_population.preparing.take() {
                // Await the real independent builder without polling or changing its publication path.
                let result = std::thread::spawn(move || receiver.blocking_recv())
                    .join()
                    .unwrap()
                    .unwrap();
                let (sender, receiver) = oneshot::channel();
                sender.send(result).ok().unwrap();
                d.realm_population.preparing = Some((id, receiver));
            }
            d.coordinator_step();
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn ordinary_two_claimed_actors_in_distinct_72_capacity_realms_do_not_allocate() {
    let app = population_app().await;
    let first = claimed_actor(&app, "population-first", -3).await;
    let second = claimed_actor(&app, "population-second", 1).await;
    assert_eq!(app.owners.lock().unwrap().len(), 2);
    let (next, started) = app
        .world
        .directory(|d| {
            assert_eq!(d.max_players, 72);
            assert_eq!(d.current_realm_count(), 2);
            assert_eq!(d.available_realms(), vec![(-3, 1), (1, 1)]);
            let next = d.next_world;
            d.coordinator_step();
            Ok((next, d.realm_population.preparing.is_some()))
        })
        .await
        .unwrap();
    let requests = (0..8).map(|_| {
        app.world.directory(|d| {
            d.coordinator_step();
            Ok(())
        })
    });
    for result in futures_util::future::join_all(requests).await {
        result.unwrap();
    }
    finish_population_build(&app).await;
    let (count, allocated, available) = app
        .world
        .directory(move |d| {
            Ok((
                d.current_realm_count(),
                d.next_world - next,
                d.available_realms(),
            ))
        })
        .await
        .unwrap();
    close_actors(&app, vec![first, second]).await;
    assert!(
        !started,
        "two lightly occupied Realms already have capacity"
    );
    assert_eq!(count, 2);
    assert_eq!(
        allocated, 0,
        "queued demand must not create unnecessary Realms"
    );
    assert_eq!(available, vec![(-3, 1), (1, 1)]);
}

#[tokio::test]
async fn same_realm_claimants_keep_last_empty_spare_without_idle_retire_regrow_churn() {
    let app = population_app().await;
    let first = claimed_actor(&app, "same-realm-first", -3).await;
    let second = claimed_actor(&app, "same-realm-second", -3).await;
    let (t, hold) = app
        .world
        .directory(|d| {
            d.coordinator_step();
            let t = d.clock.now();
            let next = d.next_world;
            for grace in 0..=4 {
                d.realm_population_step(t + IDLE_GRACE * grace);
                assert_eq!(d.current_realm_count(), 2);
                assert_eq!(d.available_realms(), vec![(-3, 2), (1, 0)]);
                assert!(d.realm_population.preparing.is_none());
                assert_eq!(d.next_world, next);
            }
            Ok((t, d.hold_initial_entry(1)?.unwrap()))
        })
        .await
        .unwrap();
    close_actors(&app, vec![first, second]).await;
    app.world
        .directory(move |d| {
            d.realm_population_step(t + IDLE_GRACE * 4);
            d.realm_population_step(t + IDLE_GRACE * 5 - Duration::from_secs(1));
            assert_eq!(d.current_realm_count(), 2);
            assert!(d.worlds[&-3].players.is_empty());
            d.realm_population_step(t + IDLE_GRACE * 5);
            assert!(!d.worlds.contains_key(&-3));
            assert_eq!(d.available_realms(), vec![(1, 0)]);
            assert_eq!(d.realm_dependencies(1), 1);
            drop(hold);
            d.realm_population_step(t + IDLE_GRACE * 6);
            d.realm_population_step(t + IDLE_GRACE * 7);
            assert_eq!(d.current_realm_count(), 1);
            assert!(d.realm_population.preparing.is_none());
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn coupled_global_72_claims_and_realm_72_hard_admission_remain_unchanged() {
    let app = population_app().await;
    let mut actors = Vec::new();
    for number in 0..72 {
        actors.push(claimed_actor(&app, &format!("hard-capacity-{number}"), -3).await);
    }
    assert_eq!(app.owners.lock().unwrap().len(), 72);
    let extra = app.store.list("hard-capacity-73".into()).await.unwrap().id;
    assert!(app.try_claim(extra).unwrap().is_none());
    app.world
        .directory(move |d| {
            assert_eq!(d.worlds[&-3].max_players, 72);
            assert_eq!(d.worlds[&-3].occupancy(), 72);
            assert_eq!(d.available_realms(), vec![(1, 0)]);
            assert!(d.issue(extra, 1, -3).is_err());
            d.coordinator_step();
            assert_eq!(d.current_realm_count(), 2);
            assert!(d.realm_population.preparing.is_none());
            Ok(())
        })
        .await
        .unwrap();
    close_actors(&app, actors).await;
    assert!(app.try_claim(extra).unwrap().is_some());
}

#[test]
fn unexpired_admissions_do_not_request_growth_with_free_capacity() {
    let mut d = directory(72);
    d.issue(1, 1, -3).unwrap();
    d.issue(2, 2, 1).unwrap();
    assert!(d.worlds[&-3].players.is_empty());
    assert!(d.worlds[&1].players.is_empty());
    assert_eq!(d.available_realms(), vec![(-3, 1), (1, 1)]);
    d.coordinator_step();
    d.coordinator_step();
    assert_eq!(d.current_realm_count(), 2);
    assert!(d.realm_population.preparing.is_none());
    assert_eq!(d.available_realms(), vec![(-3, 1), (1, 1)]);
}

#[test]
fn forty_scattered_admissions_do_not_multiply_realms() {
    let mut d = directory(72);
    let next = d.next_world;
    for account in 1..=40 {
        d.issue(account, account, if account % 2 == 0 { -3 } else { 1 })
            .unwrap();
        d.coordinator_step();
        assert_eq!(d.current_realm_count(), 2);
        assert!(d.realm_population.preparing.is_none());
        assert_eq!(d.next_world, next);
    }
    assert_eq!(d.available_realms(), vec![(-3, 20), (1, 20)]);
}

#[test]
fn eight_current_slots_keep_free_hard_admission_without_an_empty_spare() {
    let mut d = directory(72);
    for id in 2..=7 {
        let world = d.make("realm-shore", "Realm", false, None).unwrap();
        let mut record = endgame::RealmRecord::new(
            crate::endgame::RealmCycle::new(world.hero_identities()).unwrap(),
            d.tick * 50,
            d.clock.now(),
        );
        record.portal_slot = id as usize;
        record.name = id as usize;
        record.portal = Some(d.install_realm_portal(id as usize).unwrap());
        d.worlds.insert(id, world);
        d.endgame.realms.insert(id, record);
    }
    let mut account = 1;
    for (id, occupancy) in d.available_realms() {
        if occupancy == 0 {
            d.issue(account, account, id).unwrap();
            account += 1;
        }
    }
    let next = d.next_world;
    for _ in 0..3 {
        d.coordinator_step();
        assert_eq!(d.realm_population.bound_reported, None);
        assert_eq!(d.available_realms().len(), 8);
        assert!(d.initial(-3).is_ok());
        assert!(d.realm_population.preparing.is_none());
        assert_eq!(d.next_world, next);
    }
    d.issue(account, account, -3).unwrap();
    for id in d.endgame.realms.keys() {
        d.worlds.get_mut(id).unwrap().admission_open = false;
    }
    d.coordinator_step();
    assert_eq!(d.realm_population.bound_reported, Some("closed"));
    assert!(d.initial(-3).is_err());
    assert_eq!(d.current_realm_count(), 8);
    assert!(d.realm_population.preparing.is_none());
}

#[test]
fn closed_or_disabled_realms_still_request_growth_without_population() {
    for disabled in [false, true] {
        let mut d = directory(72);
        for (id, record) in &mut d.endgame.realms {
            record.enabled = !disabled;
            d.worlds.get_mut(id).unwrap().admission_open = disabled;
        }
        assert!(d.available_realms().is_empty());
        d.coordinator_step();
        let id = d.realm_population.preparing.as_ref().unwrap().0;
        d.coordinator_step();
        assert_eq!(d.current_realm_count(), 3);
        assert_eq!(d.available_realms(), vec![(id, 0)]);
    }
}

#[test]
fn token_closed_realms_keep_closure_identity_and_allocate_an_admissible_spare() {
    let mut d = directory(72);
    d.clock.set_paused(true);
    let t = d.clock.now();
    let other_roster = d.endgame.realms[&1].cycle.remaining.clone();
    let mut players = Vec::new();
    for (number, realm) in [-3, 1].into_iter().enumerate() {
        let mut ch = Character::fresh(d.content.get(crate::content::WIZARD).unwrap()).unwrap();
        ch.id = number as i32 + 1;
        ch.account = ch.id;
        let player = d.worlds.get_mut(&realm).unwrap().enter(ch, None).unwrap();
        let (_, key, _) = d
            .issue(100 + number as i32, 100 + number as i32, realm)
            .unwrap();
        d.force_realm_closure(player).unwrap();
        assert!(!d.grants.contains_key(&key));
        assert!(d.worlds[&realm].admissions.is_empty());
        assert!(!d.worlds[&realm].admission_open);
        assert!(d.endgame.realms[&realm].cycle.remaining.is_empty());
        assert!(
            d.force_realm_closure(player)
                .unwrap_err()
                .to_string()
                .contains("already closing")
        );
        if realm == -3 {
            assert_eq!(d.endgame.realms[&1].cycle.remaining, other_roster);
            assert!(d.worlds[&1].admission_open);
        }
        players.push((realm, player));
    }
    for (realm, player) in players {
        d.worlds.get_mut(&realm).unwrap().leave(player).unwrap();
    }
    let next = d.next_world;
    d.realm_population_step(t + IDLE_GRACE * 2);
    d.realm_population_step(t + IDLE_GRACE * 2);
    assert_eq!(d.current_realm_count(), 3);
    assert_eq!(d.available_realms(), vec![(next, 0)]);
    assert!(d.initial(-3).is_ok());
    for realm in [-3, 1] {
        assert!(
            d.worlds.contains_key(&realm),
            "token closure is not idle retirement"
        );
        assert!(matches!(
            d.endgame.realms[&realm].cycle.stage,
            crate::endgame::RealmStage::Closing { .. }
        ));
        assert!(d.endgame.realms[&realm].replacement.is_none());
    }
}
