use super::*;
use crate::{
    server::{self, App},
    store::Character,
    world::Output,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

// The inclusive interval also awaits policy/Store; these elapsed (not CPU) subspans leave clone/log/bookkeeping outside.
fn record_decision_phase_elapsed<T>(
    window: &mut crate::performance::Window,
    phase: &'static str,
    elapsed: Duration,
    outcome: Result<T>,
) -> Result<T> {
    window.record(phase, elapsed);
    outcome
}

async fn enter(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Option<Pos>,
) -> Result<(i32, mpsc::Receiver<Packet>, mpsc::Sender<Packet>, Brain)> {
    enter_admitted(app, profile, character, world, position, None).await
}
async fn enter_admitted(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Option<Pos>,
    admission: Option<Vec<u8>>,
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
        Some(app.store.vault(character.account).await?)
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
    let own_name = character.name.clone();
    let own_guild = presentation.guild.is_some();
    let id = app
        .world
        .in_world(world)
        .call(move |w| {
            if let Some(vault) = vault {
                w.load_vault(vault)?;
            }
            let id = w.enter_admitted(
                character,
                Some(Output { packets, lost }),
                position,
                presentation,
                admission,
            )?;
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
    let mut brain = Brain::new(profile, &view);
    if let Some(mind) = app.store.bot_mind(view.session.account).await? {
        brain.mind = mind;
    }
    brain.mind.market.name = own_name;
    brain.mind.market.guild = own_guild;
    brain.mind.market.observe(&view);
    if let Some(b) = &brain.mind.market.bargain {
        let receipts = app
            .store
            .market_receipts(view.session.account, b.terms.version)
            .await?;
        brain.mind.market.recover(receipts)?;
    }
    brain.mind.future_class = Some(if profile.healer {
        crate::content::PRIEST
    } else {
        crate::content::WIZARD
    });
    let mut generation = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&app.started, &mut generation);
    brain
        .mind
        .bind(&view, std::hash::Hasher::finish(&generation))?;
    brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?;
    Ok((id, output, sender, brain))
}

async fn rebuild_character(
    app: &App,
    profile: Profile,
    dead: &crate::world::view::Session,
) -> Result<(i32, mpsc::Receiver<Packet>, mpsc::Sender<Packet>, Brain)> {
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
        character.account == dead.account && character.id != dead.character,
        "replacement did not preserve account and replace the dead character"
    );
    // A fresh life rebuilds in Nexus; its dead character's Realm slot may already belong to a waiter.
    tracing::info!(
        account = character.account,
        dead_character = dead.character,
        character = character.id,
        destination = -2,
        "autonomous replacement rebuilding arrival selected"
    );
    enter(app, profile, character, -2, None).await
}

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
        -2
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
async fn death_committed_during_departure_rebuilds_in_nexus_without_resurrection() {
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
    assert_eq!(destination, -2);
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
        launch(app, profile, character, -3, None, None).await?;
    }
    Ok(())
}
pub(super) async fn start_selected(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Pos,
) -> Result<()> {
    let selected = character.id;
    launch(
        app,
        profile,
        character,
        world,
        Some(position),
        Some(selected),
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
) -> Result<()> {
    let account = character.account;
    let mut completed = [character.pirate_caves, character.dungeons[0]];
    let owner = app.claim(account)?;
    let lists = app.store.account_lists(account).await?;
    let (mut id, mut output, mut sender, mut brain) =
        enter(app, profile, character, world, position).await?;
    app.society
        .lock()
        .expect("adventure registry poisoned")
        .register(account, lists);
    let app = app.clone();
    tokio::spawn(async move {
        let mut shutdown = app.shutdown.subscribe();
        let mut next_bank = app.world.clock.now();
        let mut pending: Option<dialogue::Pending> = None;
        let mut deferred_market: Option<(Packet, Arc<PlayerView>)> = None;
        let mut admission: Option<Vec<u8>> = None;
        let mut last_dialogue: Option<std::time::Instant> = None;
        let mut phase = adventures::Phase::Seeking;
        let result: Result<()> = async {
                loop {
                    tokio::select! {
                        _=tokio::time::sleep(Duration::from_millis(50))=>{},
                        _=shutdown.changed()=>break,
                    }
                    if app.world.clock.paused() { continue; }
                    if let Some(transfer) = server::automatic_transfer(&app, id).await? {
                        dialogue::cancel(&app, &mut pending).await;
                        dialogue::end_agreement(&app, &mut brain, "world changed").await;
                        ensure!(transfer.player == id && transfer.output.packets.same_channel(&sender), "collective transfer does not belong to autonomous output");
                        while output.try_recv().is_ok() {}
                        deferred_market = None;
                        let destination = transfer.destination;
                        admission = Some(transfer.key.clone());
                        let character_id = app.world.directory(move |d| d.consume(account, destination, transfer.key_time, transfer.key)).await?;
                        ensure!(character_id == brain.session.2, "collective transfer character binding changed");
                        let character = app.store.load(account, character_id).await?;
                        (id, output, sender, brain) = enter_admitted(&app, profile, character, destination, None, admission.clone()).await?;
                        admission = None;
                        tracing::info!(player=id,account,character=character_id,destination,"autonomous collective transfer completed");
                        continue;
                    }
                    let view = Arc::new(app.world.directory(move |d| d.player_view(id)).await?);
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
                                let lists = app.store.edit_account_list(account,0,true,peer).await?;
                                app.world.for_player(id).call(move |w| w.sync_account_lists(id,Some(lists))).await?;
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
                        brain.mind.end(&view, "character_died_steps_invalidated");
                        brain.mind.market.finish("character_died_remaining_balance_unenforced");
                        brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?;
                        server::depart(&app, id).await?;
                        if selected.is_some() { return Ok(()); }
                        tokio::select! {
                            _=tokio::time::sleep(Duration::from_secs(3))=>{},
                            _=shutdown.changed()=>return Ok(()),
                        }
                        (id, output, sender, brain) = rebuild_character(&app, profile, &view.session).await?;
                        deferred_market = None;
                        continue;
                    }
                    ensure!(matches!(view.lifecycle.output, OutputState::Live)
                        && !view.lifecycle.departing && !view.lifecycle.quarantined, "controller is not live");
                    if let Some(agreement) = &brain.agreement {
                        let reason = if view.time_ms >= brain.invitation_until {
                            Some("agreement expired")
                        } else if agreement.waiting.is_none() && !view.objects.iter().any(|p| p.name==agreement.partner && p.hp>0 && !p.equipped.is_empty()) {
                            Some("lost sight of partner")
                        } else { None };
                        if let Some(reason) = reason { dialogue::end_agreement(&app, &mut brain, reason).await; }
                    }
                    while let Some((packet, received_view)) = deferred_market.take().or_else(||output.try_recv().ok().map(|p|(p,view.clone()))) {
                        if packet.id == crate::wire::GOTO {
                            app.world.for_player(id).call(move |w| w.acknowledge_autonomous_goto(id)).await?;
                            continue;
                        }
                        let settled_view = if matches!(packet.id,12|23|67) { Some(app.world.directory(move |d|d.player_view(id)).await?) } else { None };
                        let social_view = settled_view.as_ref().unwrap_or(&received_view);
                        let event_before = brain.mind.events.clone();
                        brain.mind.events.receive(&packet, social_view, profile.name)?;
                        if brain.mind.events != event_before { brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?; }
                        let social_before = brain.mind.social.clone();
                        let socially_handled = social::receive_life(&app, &mut brain, &packet, social_view).await?;
                        if brain.mind.social != social_before { brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?; }
                        if socially_handled { continue; }
                        match social::receive_market(&app,&mut brain,&packet,social_view).await? {
                            social::MarketUptake::Applied => continue,
                            social::MarketUptake::PendingCommit => { deferred_market=Some((packet,received_view)); break; },
                            social::MarketUptake::Unrelated => {},
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
                    if deferred_market.is_some() { continue; }
                    if pending.is_none() && brain.mind.market.outgoing.is_none() && brain.mind.market.bargain.as_ref().is_none_or(|b|b.resolution.is_some()) && app.dialogue.as_ref().is_none_or(|service|service.automatic_available()) {
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
                    if view.lifecycle.busy { continue; }
                    if social::life(&app, &mut brain, &view).await? { continue; }
                    if social::market(&app, &mut brain, &view).await? { continue; }
                    if view.tick < brain.next_decision { continue; }
                    // World clocks start at different times; banking delay belongs to this account session.
                    let now = app.world.clock.now();
                    brain.next_bank = view.time_ms + u64::try_from(next_bank.saturating_duration_since(now).as_millis())?;
                    let deciding = std::time::Instant::now();
                    let prior_mind = brain.mind.clone();
                    let sync_deciding = std::time::Instant::now();
                    let decision = brain.decide(&view, &app.content);
                    let decision = record_decision_phase_elapsed(&mut brain.performance, "decide_sync_elapsed", sync_deciding.elapsed(), decision)?;
                    if events::shared(&view.map.name) {
                        tracing::info!(account,character=view.session.character,world=%view.map.name,tick=view.tick,time_ms=view.time_ms,
                            hp=view.character.hp,max_hp=view.character.stats[0],mp=view.character.mp,recovery=profile.recovery,
                            held_health=view.character.items.iter().skip(4).filter(|k|**k==0xa22).count(),
                            goal=?decision.goal,journey=?decision.journey,movement=?decision.movement,
                            "autonomous event action selected");
                    }
                    let revoking = std::time::Instant::now();
                    let revoked = social::revoke_market(&app,&mut brain).await;
                    if !record_decision_phase_elapsed(&mut brain.performance, "market_revoke_await_elapsed", revoking.elapsed(), revoked)? {
                        brain.mind=prior_mind;
                        continue;
                    }
                    if brain.mind != prior_mind {
                        let saving_mind = std::time::Instant::now();
                        let revision = app.store.save_bot_mind(brain.mind.clone()).await;
                        brain.mind.revision = record_decision_phase_elapsed(&mut brain.performance, "mind_store_await_elapsed", saving_mind.elapsed(), revision)?;
                    }
                    brain.performance.record("decision", deciding.elapsed());
                    if let Some(timings)=brain.performance.report() {
                        tracing::info!(name=profile.name,world=%view.map.name,known_tiles=view.tiles.len(),
                            cover_tiles=brain.cover.len(),visited_tiles=brain.visits.len(),
                            blocked_routes=brain.blocked.len(),tracks=brain.tracks.len(),crowd_contacts=brain.crowd.len(),
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
                        tracing::info!(account,character=view.session.character,world=view.session.world,player=id,name=profile.name,tick=view.tick,hp=view.character.hp,
                            experience=view.character.experience,x=view.position.x,y=view.position.y,
                            cognition=%serde_json::to_string(&brain.mind.cognition)?,aim=?decision.aim,movement=?decision.movement,
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
                    match app.world.for_player(id).call(move |w| {
                        let mut plan = None;
                        for action in actions {
                            if let Some(item) = w.autonomous_action(id, action)? { plan = Some(item); }
                        }
                        Ok(plan)
                    }).await {
                        Ok(Some(plan)) => {
                            server::persist(&app, plan).await?;
                            let after = app.world.directory(move |d| d.player_view(id)).await?;
                            let prior_events = brain.mind.events.clone();
                            brain.mind.events.committed(&after)?;
                            if brain.mind.events != prior_events { brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?; }
                            tracing::info!(account, character=after.session.character, world=after.session.world,
                                before_revision=view.character.inventory_revision, revision=after.character.inventory_revision,
                                items=?after.character.items, actions=diagnostic, "autonomous inventory outcome");
                        },
                        Ok(None) => { brain.mind.events.rejected(); },
                        Err(error) => {
                            brain.mind.events.rejected();
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
                                admission = Some(key.clone());
                                dialogue::cancel(&app, &mut pending).await;
                                dialogue::end_agreement(&app, &mut brain, "world changed").await;
                                server::depart(&app, id).await?;
                                let character_id = app.world.directory(move |d| d.consume(account, destination, time, key)).await?;
                                ensure!(character_id == view.session.character, "transfer character binding changed");
                                let (character, destination) = match selected {
                                    Some(selected) => (continue_selected(&app.store, account, selected).await?, destination),
                                    None => continue_character(&app.store, &app.content, profile, &view.session, destination).await?,
                                };
                                let current_character = character.id;
                                if current_character != character_id {
                                    tracing::info!(account,character=character_id,replacement=current_character,"autonomous transfer interrupted by permanent death");
                                }
                                if current_character != character_id {
                                    let key=admission.take().context("replaced character admission missing")?;
                                    app.world.directory(move |d|d.cancel_admission(account,key)).await?;
                                }
                                (id, output, sender, brain) = enter_admitted(&app, profile, character, destination, None, admission.clone()).await?;
                                admission = None;
                                tracing::info!(player=id,account,character=current_character,destination,?portal,portal_kind,"autonomous durable transfer completed");
                            }
                        }
                    }
                }
                let view = app.world.directory(move |d| d.player_view(id)).await?;
                brain.mind.reconcile(&view)?;
                brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?;
                server::depart(&app, id).await
            }.await;
        if let Some(key) = admission
            && let Err(error) = app
                .world
                .directory(move |d| d.cancel_admission(account, key))
                .await
        {
            tracing::error!(account,%error,"failed controller admission cancellation failed");
        }
        app.society
            .lock()
            .expect("adventure registry poisoned")
            .unregister(account);
        dialogue::cancel(&app, &mut pending).await;
        dialogue::end_agreement(&app, &mut brain, "controller stopped").await;
        if let Err(error) = result {
            tracing::error!(player=id,name=profile.name,%error,"autonomous controller stopped; account retained for diagnosis");
            let reason = error.to_string();
            if let Err(error) = app
                .world
                .for_player(id)
                .call(move |w| w.poison(id, reason))
                .await
            {
                tracing::error!(player=id,%error,"autonomous quarantine failed");
            }
            app.quarantined
                .lock()
                .expect("quarantine registry poisoned")
                .push(owner);
        }
    });
    Ok(())
}

#[cfg(test)]
mod community_crowd_tests;
#[cfg(test)]
mod community_events_tests;
#[cfg(test)]
mod community_market_tests;
#[cfg(test)]
mod community_resources_tests;
#[cfg(test)]
mod community_social_tests;
#[cfg(test)]
mod decision_timing_tests;
#[cfg(test)]
mod market_consent_tests;
