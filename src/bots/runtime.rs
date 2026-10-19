use super::*;
use crate::{
    server::{
        self, App, Owner,
        lifecycle::{self, CloseReason},
    },
    store::Character,
    world::Output,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

async fn enter(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Option<Pos>,
) -> Result<(i32, mpsc::Receiver<Packet>, mpsc::Sender<Packet>, Brain)> {
    let world = if world == -3 && position.is_none() {
        app.world.directory(|d| d.initial(-3)).await?
    } else {
        world
    };
    let vault = if app
        .world
        .in_world(world)
        .call(|w| Ok(w.name == "Vault"))
        .await?
    {
        Some(app.store.vaults(character.account).await?)
    } else {
        None
    };
    tracing::info!(name = profile.name, saved = %serde_json::to_string(&character)?,
        "autonomous saved character loaded");
    // Concurrent novas and hit receipts must fit between bounded perception updates.
    let (packets, output) = mpsc::channel(OUTPUT_CAPACITY);
    let sender = packets.clone();
    let (lost, _) = watch::channel(false);
    let presentation = app.store.presentation(character.account).await?;
    let bind = server::lifecycle::binding(app, character.account)?;
    let id = app
        .world
        .in_world(world)
        .call(move |w| {
            if let Some(vault) = vault {
                w.load_vaults(vault)?;
            }
            let id = w.enter_at(
                character,
                Some(Output { packets, lost }),
                position,
                presentation,
            )?;
            bind(id)?;
            w.players
                .get_mut(&id)
                .context("new autonomous player missing")?
                .controller = crate::world::Controller::Autonomous;
            Ok(id)
        })
        .await?;
    let view = app.world.directory(move |d| d.player_view(id)).await?;
    tracing::info!(
        player = id,
        name = profile.name,
        class = view.character.class,
        level = view.character.level,
        character = view.session.character,
        account = view.session.account,
        experience = view.character.experience,
        hp = view.character.hp,
        mp = view.character.mp,
        items = ?view.character.items,
        revision = view.character.inventory_revision,
        "autonomous character entered"
    );
    Ok((id, output, sender, Brain::new(profile, &view)))
}

type Entry = (i32, mpsc::Receiver<Packet>, mpsc::Sender<Packet>, Brain);

enum Reentry {
    Collective {
        destination: i32,
        time: i32,
        key: Vec<u8>,
        character: i32,
    },
    Portal {
        destination: i32,
        time: i32,
        key: Vec<u8>,
        session: crate::world::view::Session,
        selected: Option<i32>,
    },
    Replacement {
        previous: i32,
    },
}

async fn reenter(
    app: &App,
    profile: Profile,
    account: i32,
    owner: &mut Option<Owner>,
    request: Reentry,
) -> Result<Entry> {
    owner
        .as_ref()
        .context("autonomous reentry lost account custody")?
        .admit_controller(app)?;
    let entry = lifecycle::drain::entry(app)?;
    let mut owned = owner
        .take()
        .context("autonomous reentry lost account custody")?;
    owned.arm_cleanup(app, tokio::runtime::Handle::current());
    let (owned, result) = lifecycle::account(
        app,
        vec![account],
        "autonomous_reentry",
        move |app| async move {
            let _entry = entry;
            let result = async {
                server::lifecycle::stage(server::lifecycle::Stage::Reservation);
                let (character, destination) = match request {
                    Reentry::Collective {
                        destination,
                        time,
                        key,
                        character,
                    } => {
                        let consumed = app
                            .world
                            .directory(move |d| d.consume(account, destination, time, key))
                            .await?;
                        ensure!(
                            consumed == character,
                            "collective transfer character binding changed"
                        );
                        server::lifecycle::stage(server::lifecycle::Stage::Store);
                        (app.store.load(account, consumed).await?, destination)
                    }
                    Reentry::Portal {
                        destination,
                        time,
                        key,
                        session,
                        selected,
                    } => {
                        let consumed = app
                            .world
                            .directory(move |d| d.consume(account, destination, time, key))
                            .await?;
                        ensure!(
                            consumed == session.character,
                            "transfer character binding changed"
                        );
                        server::lifecycle::stage(server::lifecycle::Stage::Store);
                        match selected {
                            Some(selected) => (
                                continue_selected(&app.store, account, selected).await?,
                                destination,
                            ),
                            None => {
                                continue_character(
                                    &app.store,
                                    &app.content,
                                    profile,
                                    &session,
                                    destination,
                                )
                                .await?
                            }
                        }
                    }
                    Reentry::Replacement { previous } => {
                        server::lifecycle::stage(server::lifecycle::Stage::Store);
                        let character = app
                            .store
                            .shore_character(
                                profile.identity.into(),
                                profile.name.into(),
                                &app.content,
                                profile.healer,
                            )
                            .await?;
                        ensure!(
                            character.account == account && character.id != previous,
                            "replacement did not preserve account and replace the dead character"
                        );
                        (character, -3)
                    }
                };
                server::lifecycle::stage(server::lifecycle::Stage::Publication);
                enter(&app, profile, character, destination, None).await
            }
            .await;
            if let Err(error) = &result {
                lifecycle::observe(&app, account, "autonomous_reentry", error);
            }
            Ok((owned, result))
        },
    )
    .await?;
    *owner = Some(owned);
    result
}

#[cfg(test)]
mod finalize_tests;
#[cfg(test)]
mod reentry_tests;

async fn continue_character(
    store: &crate::store::Store,
    content: &Content,
    profile: Profile,
    session: &crate::world::view::Session,
    destination: i32,
) -> Result<(Character, i32)> {
    // Departure can commit a death after the transfer was requested.
    let character = store
        .shore_character(
            profile.identity.into(),
            profile.name.into(),
            content,
            profile.healer,
        )
        .await?;
    ensure!(
        character.account == session.account,
        "departed autonomous account binding changed"
    );
    let destination = if character.id == session.character {
        destination
    } else {
        -3
    };
    Ok((character, destination))
}

async fn continue_selected(
    store: &crate::store::Store,
    account: i32,
    character: i32,
) -> Result<Character> {
    store
        .load(account, character)
        .await
        .map_err(|error| {
            if matches!(
                error.downcast_ref::<rusqlite::Error>(),
                Some(rusqlite::Error::QueryReturnedNoRows)
            ) {
                lifecycle::stage(lifecycle::Stage::Rejected);
                error.context(CloseReason::Controller)
            } else {
                error
            }
        })
        .context("selected autonomous character is missing or dead; no replacement was selected")
}
#[cfg(test)]
#[tokio::test]
async fn selected_character_death_never_creates_a_replacement() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let account = store.list("selected-lifetime-test".into()).await.unwrap();
    let mut character = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let selected = character.id;
    character.hp = 0;
    character.alive = false;
    store.death(character, "unit-test".into()).await.unwrap();
    assert!(
        continue_selected(&store, account.id, selected)
            .await
            .is_err()
    );
    assert!(
        store
            .list("selected-lifetime-test".into())
            .await
            .unwrap()
            .characters
            .is_empty()
    );
}
#[cfg(test)]
#[tokio::test]
async fn death_committed_during_departure_rebuilds_in_realm_without_resurrection() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let profile = PROFILES[0];
    let mut wizard = store
        .shore_character(profile.identity.into(), profile.name.into(), &content, true)
        .await
        .unwrap();
    wizard.level = 5;
    wizard.exp = crate::store::level_start(5).unwrap();
    store.progress(wizard.clone()).await.unwrap();
    let session = crate::world::view::Session {
        world: -3,
        player: 1,
        account: wizard.account,
        character: wizard.id,
    };
    let (living, destination) = continue_character(&store, &content, profile, &session, -2)
        .await
        .unwrap();
    assert_eq!(
        (living.id, living.class, destination),
        (wizard.id, crate::content::WIZARD, -2)
    );
    wizard.hp = 0;
    wizard.alive = false;
    store
        .death(wizard.clone(), "prepared departure race fixture".into())
        .await
        .unwrap();
    assert!(
        store
            .load(session.account, session.character)
            .await
            .is_err()
    );
    let (replacement, destination) = continue_character(&store, &content, profile, &session, -2)
        .await
        .unwrap();
    assert_eq!(destination, -3);
    assert_eq!(replacement.account, session.account);
    assert_ne!(replacement.id, session.character);
    assert_eq!(
        (replacement.class, replacement.level, replacement.exp),
        (crate::content::PRIEST, 1, 0)
    );
    assert_eq!(
        replacement.items,
        content.get(crate::content::PRIEST).unwrap().equipment
    );
    let replacement_id = replacement.id;
    let (same, _) = continue_character(&store, &content, profile, &session, -2)
        .await
        .unwrap();
    assert_eq!(same.id, replacement_id);
    assert!(
        store
            .load(session.account, session.character)
            .await
            .is_err()
    );
}

pub async fn start(app: &App) -> Result<()> {
    for profile in PROFILES {
        let character = app
            .store
            .shore_character(
                profile.identity.into(),
                profile.name.into(),
                &app.content,
                profile.healer,
            )
            .await?;
        let owner = app.claim(character.account)?;
        launch(app, profile, character, -3, None, None, owner).await?;
    }
    Ok(())
}
pub(super) async fn start_selected(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Pos,
    owner: server::Owner,
) -> Result<()> {
    let selected = character.id;
    launch(
        app,
        profile,
        character,
        world,
        Some(position),
        Some(selected),
        owner,
    )
    .await
}
async fn launch(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Option<Pos>,
    selected: Option<i32>,
    mut owner: server::Owner,
) -> Result<()> {
    owner.admit_controller(app)?;
    let mut drain = lifecycle::drain::stopping(app);
    let entry = lifecycle::drain::entry(app)?;
    let account = character.account;
    let mut completed = [character.pirate_caves, character.dungeons[0]];
    owner.arm_cleanup(app, tokio::runtime::Handle::current());
    let (owner, entry, result) = lifecycle::account(
        app,
        vec![account],
        "autonomous_entry",
        move |app| async move {
            let result = async {
                let lists = app.store.account_lists(account).await?;
                lifecycle::stage(lifecycle::Stage::Publication);
                Ok((
                    enter(&app, profile, character, world, position).await?,
                    lists,
                ))
            }
            .await;
            if let Err(error) = &result {
                lifecycle::observe(&app, account, "autonomous_entry", error);
            }
            Ok((owner, entry, result))
        },
    )
    .await?;
    let ((mut id, mut output, mut sender, mut brain), lists) = result?;
    app.society
        .lock()
        .expect("adventure registry poisoned")
        .register(account, lists);
    let app = app.clone();
    tokio::spawn(async move {
        let _entry = entry;
        let mut owner = Some(owner);
        let mut shutdown = app.shutdown.subscribe();

        let mut next_bank = app.world.clock.now();
        let mut pending: Option<dialogue::Pending> = None;
        let mut last_dialogue: Option<std::time::Instant> = None;
        let mut phase = adventures::Phase::Seeking;
        let result: Result<()> = async {
                loop {
                    if *shutdown.borrow() || drain.has_changed()? || !lifecycle::drain::open(&app) { break; }
                    tokio::select! {
                        _=tokio::time::sleep(Duration::from_millis(50))=>{},
                        _=shutdown.changed()=>break,
                        _=drain.changed()=>break,
                    }
                    if app.world.clock.paused() { continue; }
                    let _entry = lifecycle::drain::entry(&app)?;
                    if let Some(transfer) = server::automatic_transfer(&app, id).await? {
                        dialogue::cancel(&app, &mut pending).await;
                        dialogue::end_agreement(&app, &mut brain, "world changed").await;
                        ensure!(transfer.player == id && transfer.output.packets.same_channel(&sender), "collective transfer does not belong to autonomous output");
                        while output.try_recv().is_ok() {}
                        let destination = transfer.destination;
                        let character_id = brain.session.2;
                        (id, output, sender, brain) = reenter(&app, profile, account, &mut owner, Reentry::Collective { destination, time: transfer.key_time, key: transfer.key, character: character_id }).await?;
                        tracing::info!(player=id,account,character=character_id,destination,"autonomous collective transfer completed");
                        continue;
                    }
                    let view = app.world.directory(move |d| d.player_view(id)).await?;
                    ensure!(view.session.account == account, "autonomous account binding changed");
                    if let Some(selected) = selected { ensure!(view.session.character == selected, "selected autonomous character changed"); }
                    if view.map.name=="Nexus" && brain.agreement.as_ref().is_none_or(|a|a.waiting.is_none()) && brain.group_lost_since.is_some_and(|since|view.time_ms.saturating_sub(since)>=30000) {
                        app.society.lock().expect("adventure registry poisoned").leave(account);
                        brain.group_lost_since=None;
                        tracing::info!(account,"autonomous rendezvous timed out; continuing independently");
                    }
                    {
                        let mut society=app.society.lock().expect("adventure registry poisoned");
                        brain.group=society.group(account,view.elapsed_ms);
                        brain.offered=society.outgoing(account,view.elapsed_ms);
                    }
                    let observed_phase=brain.group.as_ref().map_or(adventures::Phase::Seeking,|g|g.phase(&view));
                    if observed_phase!=phase {tracing::info!(account,?observed_phase,"autonomous adventure phase changed");phase=observed_phase;}
                    let observed=[view.character.pirate_caves,view.character.snake_pits];
                    let objective=brain.group.as_ref().map(|g|usize::from(g.purpose==adventures::Purpose::Speed));
                    if view.lifecycle.alive && !view.lifecycle.death_pending && objective.is_some_and(|n|observed[n]>completed[n]) {
                        let peers = app.society.lock().expect("adventure registry poisoned").completed(account, &view, completed);
                        for peer in peers {
                            if app.society.lock().expect("adventure registry poisoned").remember(account, peer) {
                                remember_clear_peer(&app, id, account, peer).await?;
                                tracing::info!(account,peer,"autonomous shared clear remembered in account lock list");
                            }
                        }
                    }
                    completed = observed;
                    if view.lifecycle.death_pending {
                        while output.try_recv().is_ok() {}
                        continue;
                    }
                    if !view.lifecycle.alive {
                        app.society.lock().expect("adventure registry poisoned").leave(account);
                        dialogue::cancel(&app, &mut pending).await;
                        dialogue::end_agreement(&app, &mut brain, "character died").await;
                        lifecycle::finalize(&app, id, CloseReason::Controller)?.wait().await?;
                        if selected.is_some() { return Ok(()); }
                        tokio::select! {
                            _=tokio::time::sleep(Duration::from_secs(3))=>{},
                            _=shutdown.changed()=>return Ok(()),
                        }
                        (id, output, sender, brain) = reenter(&app, profile, account, &mut owner, Reentry::Replacement { previous: view.session.character }).await?;
                        continue;
                    }
                    ensure!(matches!(view.lifecycle.output, OutputState::Live)
                        && !view.lifecycle.departing && !view.lifecycle.quarantined,
                        lifecycle::controller("controller is not live"));
                    if let Some(agreement) = &brain.agreement {
                        let reason = if view.time_ms >= brain.invitation_until {
                            Some("agreement expired")
                        } else if agreement.waiting.is_none() && !view.objects.iter().any(|p| p.name==agreement.partner && p.hp>0 && !p.equipped.is_empty()) {
                            Some("lost sight of partner")
                        } else { None };
                        if let Some(reason) = reason { dialogue::end_agreement(&app, &mut brain, reason).await; }
                    }
                    while let Ok(packet) = output.try_recv() {
                        if packet.id == crate::wire::GOTO {
                            app.world.for_player(id).call(move |w| w.acknowledge_autonomous_goto(id)).await?;
                            continue;
                        }
                        if let Some(service) = &app.dialogue
                            && let Some(request) = brain.dialogue_request(&packet, &view, &app.content)?
                        {
                            if pending.is_some() || last_dialogue.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
                                dialogue::failed(&app, request.speaker, "busy", "busy").await;
                            } else {
                                last_dialogue = Some(std::time::Instant::now());
                                pending = Some(dialogue::Pending::start(service.clone(), request));
                            }
                            continue;
                        }
                        if brain.receive(packet, &view, app.dialogue.is_none())? {
                            let received = app.world.clock.now();
                            if let Err(error) = app.world.directory(move |d| d.chat(id, "omw".into(), received)).await {
                                tracing::warn!(player=id,%error,"invitation acknowledgement rejected");
                            }
                            tracing::info!(player=id, companion=brain.companion, "visible invitation accepted");
                        }
                    }
                    if pending.is_none() && app.dialogue.as_ref().is_none_or(|service|service.automatic_available()) {
                        let plan=app.society.lock().expect("adventure registry poisoned").next(&brain,&view,&app.content,app.dialogue.is_some());
                        if let Some(plan)=plan {
                            let request=brain.adventure_request(plan,&view,&app.content)?;
                            if let Some(service)=&app.dialogue {pending=Some(dialogue::Pending::start(service.clone(),request));}
                            else {
                                let intent:dialogue::Intent=serde_json::from_value(request.payload["allowed_intents"][0].clone())?;
                                let reply=adventures::policy_reply(&request,intent)?;
                                if let Err(error)=dialogue::publish(&app,&mut brain,&request,&reply,&view).await {
                                    tracing::warn!(account,%error,"autonomous policy invitation rejected");
                                }
                            }
                        }
                    }
                    if pending.as_ref().is_some_and(|p| p.task.is_finished()) {
                        let mut completed = pending.take().expect("finished dialogue missing");
                        let result = match (&mut completed.task).await {
                            Ok(result) => result,
                            Err(error) => {
                                tracing::error!(request=%completed.id,%error,"bot dialogue worker failed");
                                Err(dialogue::Failure("worker_failed"))
                            }
                        };
                        match result {
                            Ok(reply) => {
                                let validation = dialogue::publish(&app,&mut brain,&completed.request,&reply,&view).await;
                                match validation {
                                    Ok(()) => {
                                        tracing::info!(request=%completed.id,player=id,speaker=completed.request.speaker,intent=?reply.intent,"bot dialogue intention applied");
                                    }
                                    Err(error) => {
                                        tracing::error!(request=%completed.id,player=id,%error,"bot dialogue intention rejected");
                                        dialogue::failed(&app, completed.request.speaker, &completed.id, "rejected").await;
                                    }
                                }
                            }
                            Err(error) => dialogue::failed(&app, completed.request.speaker, &completed.id, error.0).await,
                        }
                    }
                    brain.proposing=pending.as_ref().is_some_and(|p|matches!(p.request.plan,Some(adventures::Plan::Invite{..})));
                    if view.lifecycle.busy || view.tick < brain.next_decision { continue; }
                    // World clocks start at different times; banking delay belongs to this account session.
                    let now = app.world.clock.now();
                    brain.next_bank = view.time_ms + u64::try_from(next_bank.saturating_duration_since(now).as_millis())?;
                    let deciding = crate::performance::Measure::start();
                    let decision = brain.decide(&view, &app.content)?;
                    let timings={let mut p=brain.performance.lock().expect("bot timings poisoned");p.work("decision",deciding);p.report()};
                    if let Some(timings)=timings {
                        tracing::info!(name=profile.name,world=%view.map.name,world_id=view.session.world,player=view.session.player,known_tiles=view.tiles.len(),
                            cover_tiles=brain.cover.len(),visited_tiles=brain.visits.len(),
                            blocked_routes=brain.blocked.len(),tracks=brain.tracks.len(),contacts=brain.contacts.len(),
                            navigation_rebuilds=brain.navigation_rebuilds,navigation_reuses=brain.navigation_reuses,
                            navigation=%serde_json::json!(brain.navigation.as_ref().map(|g|g.metrics())),timings=%timings,"bot performance");
                    }
                    next_bank = now.checked_add(Duration::from_millis(brain.next_bank.saturating_sub(view.time_ms))).context("autonomous bank cooldown overflow")?;
                    if let Some(text) = decision.chat {
                        let received = app.world.clock.now();
                        match app.world.directory(move |d| d.chat(id, text.into(), received)).await {
                            Ok(()) => tracing::info!(player=id,text,"autonomous public call delivered"),
                            Err(error) => tracing::warn!(player=id,text,%error,"autonomous public call rejected"),
                        }
                    }
                    if decision.goal != brain.goal {
                        tracing::info!(player=id,name=profile.name,tick=view.tick,hp=view.character.hp,
                            experience=view.character.experience,x=view.position.x,y=view.position.y,
                            goal=?decision.goal,"autonomous decision");
                    }
                    brain.goal = decision.goal;
                    brain.destination = decision.movement;
                    let mut actions = vec![Action::Stop];
                    if decision.journey.is_none() {
                        if let Some(pos) = decision.movement { actions.push(Action::Move { x:pos.x, y:pos.y }); }
                        if let Some(angle) = decision.aim { actions.push(Action::Shoot { angle }); }
                        if let Some(action) = decision.item { actions.push(action); }
                    }
                    let diagnostic = format!("{actions:?}");
                    match server::inventory(&app, id, move |w| {
                        let mut plan = None;
                        for action in actions {
                            if let Some(item) = w.autonomous_action(id, action)? { plan = Some(item); }
                        }
                        Ok(plan)
                    }).await? {
                        Ok(()) => {},
                        Err(error) => {
                            if let Some(pos) = brain.destination { brain.blocked.insert(tile(pos), view.tick + 100); }
                            tracing::warn!(player=id,%error,actions=diagnostic,"autonomous commands rejected; route excluded and will reassess");
                            continue;
                        }
                    }
                    if let Some(journey) = decision.journey {
                        match journey {
                            Journey::Regroup(target) => {
                                let received = app.world.clock.now();
                                if let Err(error) = app.world.directory(move |d| d.teleport(id, target, received)).await {
                                    tracing::warn!(player=id,target,%error,"autonomous regroup rejected");
                                }
                            }
                            Journey::Portal(_) | Journey::Nexus => {
                                let portal = match journey { Journey::Portal(portal) => Some(portal), Journey::Nexus => None, Journey::Regroup(_) => unreachable!() };
                                let portal_kind = portal.and_then(|target| view.objects.iter().find(|o| o.id == target).map(|o| o.kind));
                                let request = app.world.directory(move |d| {
                                    let (account, character, destination) = d.destination(id, portal)?;
                                    let (time, key, _) = d.issue(account, character, destination)?;
                                    Ok((destination, time, key))
                                }).await;
                                let (destination, time, key) = match request {
                                    Ok(request) => request,
                                    Err(error) => {
                                        brain.portal = None;
                                        tracing::warn!(player=id,?portal,%error,"autonomous transfer rejected before departure");
                                        continue;
                                    }
                                };
                                dialogue::cancel(&app, &mut pending).await;
                                dialogue::end_agreement(&app, &mut brain, "world changed").await;
                                lifecycle::finalize(&app, id, CloseReason::Controller)?.wait().await?;
                                let previous = view.session.character;
                                (id, output, sender, brain) = reenter(&app, profile, account, &mut owner, Reentry::Portal { destination, time, key, session: view.session, selected }).await?;
                                let current_character = brain.session.2;
                                let destination = if current_character != previous { -3 } else { destination };
                                if current_character != previous {
                                    tracing::info!(account,character=previous,replacement=current_character,"autonomous transfer interrupted by permanent death");
                                }
                                tracing::info!(player=id,account,character=current_character,destination,?portal,portal_kind,"autonomous durable transfer completed");
                            }
                        }
                    }
                }
                Ok(())
            }.await;
        // A replacement may register as soon as finalization releases this generation.
        app.society
            .lock()
            .expect("adventure registry poisoned")
            .unregister(account);
        let closing = owner.take().map(|owner| {
            close_controller(
                &app,
                owner,
                result.as_ref().err(),
                if *shutdown.borrow() {
                    CloseReason::Shutdown
                } else {
                    CloseReason::Controller
                },
            )
        });
        if let Some(closing) = closing {
            let saved = match closing {
                Ok(Some(completion)) => completion.wait().await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            if let Err(error) = saved {
                tracing::error!(player=id,account,%error,"autonomous finalization failed; account remains fenced");
            }
        }
        dialogue::cancel(&app, &mut pending).await;
        dialogue::end_agreement(&app, &mut brain, "controller stopped").await;
        if let Err(error) = result {
            tracing::error!(player=id,name=profile.name,%error,"autonomous controller stopped");
        }
    });
    Ok(())
}

async fn remember_clear_peer(app: &App, id: i32, account: i32, peer: i32) -> Result<()> {
    lifecycle::account(
        app,
        vec![account],
        "autonomous_remembrance",
        move |app| async move {
            lifecycle::stage(lifecycle::Stage::Store);
            let lists = app.store.edit_account_list(account, 0, true, peer).await?;
            lifecycle::stage(lifecycle::Stage::Publication);
            app.world
                .for_player(id)
                .call(move |w| w.sync_account_lists(id, Some(lists)))
                .await
        },
    )
    .await
}

fn close_controller(
    app: &App,
    owner: Owner,
    error: Option<&anyhow::Error>,
    reason: CloseReason,
) -> Result<Option<lifecycle::Finalized>> {
    let reason = match error {
        None => reason,
        Some(error) if lifecycle::rule_rejection(error) => CloseReason::Controller,
        Some(error) => error
            .downcast_ref::<CloseReason>()
            .copied()
            .unwrap_or(CloseReason::Internal),
    };
    if reason == CloseReason::Internal
        && let Some(error) = error
    {
        owner.failed(format!(
            "unclassified autonomous controller failure: {error:#}"
        ));
    }
    owner.close(app, reason)
}
