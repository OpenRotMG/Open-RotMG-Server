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

// A live native window may hold movement, never the controller's survival evaluation.
async fn decide_before_social(
    app: &App,
    brain: &mut Brain,
    view: &PlayerView,
) -> Result<Option<Decision>> {
    let deciding = std::time::Instant::now();
    let before = brain.mind.clone();
    social::recover_market(app, brain).await?;
    let prior_mind = brain.mind.clone();
    let measure = crate::performance::Measure::start();
    let sync_deciding = std::time::Instant::now();
    let decision = brain.decide(view, &app.content);
    let decision = {
        let mut timings = brain.performance.lock().expect("bot timings poisoned");
        timings.completed("decision_sync_work", measure.finish());
        record_decision_phase_elapsed(
            &mut timings,
            "decide_sync_elapsed",
            sync_deciding.elapsed(),
            decision,
        )?
    };
    app.cctv.decided(
        brain.profile.name,
        view,
        &decision,
        cctv::reason(brain.activity, &decision, view, &app.content),
        &brain.intent,
        &app.content,
    );
    let surviving = brain.activity == Activity::Survive;
    if surviving {
        brain.close_composition(app, false).await?;
    }
    if surviving
        && (brain.mind.social.gift_active()
            || brain.mind.market.trade.is_some()
            || brain.mind.market.incoming.is_some()
            || brain
                .mind
                .market
                .bargain
                .as_ref()
                .is_some_and(|b| b.resolution.is_none()))
    {
        brain.mind.market.revoke_trade = true;
    }
    let revoking = std::time::Instant::now();
    let revoked = social::revoke_market(app, brain).await;
    let revoked = record_decision_phase_elapsed(
        &mut brain.performance.lock().expect("bot timings poisoned"),
        "market_revoke_await_elapsed",
        revoking.elapsed(),
        revoked,
    )?;
    let mut held = if !revoked {
        brain.mind = prior_mind;
        true
    } else if surviving {
        let pending = social::interrupt_gift(app, brain, view).await?;
        if !pending && brain.mind.market.incoming.is_some() {
            brain.mind.market.end_incoming(view);
        }
        pending
    } else {
        social::life(app, brain, view).await? || social::market(app, brain, view).await?
    };
    if !held {
        let current = app.world.player_view(view.session.player).await?;
        ensure!(
            (
                current.session.world,
                current.session.player,
                current.session.character
            ) == brain.session,
            "controller changed while coordinating social actions"
        );
        // A committed trade may have published after the decision's inventory snapshot.
        held = current.lifecycle.busy
            || current.character.inventory_revision != view.character.inventory_revision;
    }
    if brain.mind != before {
        let saving = std::time::Instant::now();
        let revision = app.store.save_bot_mind(brain.mind.clone()).await;
        brain.mind.revision = record_decision_phase_elapsed(
            &mut brain.performance.lock().expect("bot timings poisoned"),
            "mind_store_await_elapsed",
            saving.elapsed(),
            revision,
        )?;
    }
    let mut timings = brain.performance.lock().expect("bot timings poisoned");
    timings.record("decision", deciding.elapsed());
    if let Some(report) = timings.report() {
        tracing::info!(name=brain.profile.name,world=%view.map.name,world_id=view.session.world,player=view.session.player,
            known_tiles=view.tiles.len(),cover_tiles=brain.cover.len(),visited_tiles=brain.visits.len(),
            blocked_routes=brain.blocked.len(),tracks=brain.tracks.len(),crowd_contacts=brain.crowd.len(),
            navigation_rebuilds=brain.navigation_rebuilds,navigation_reuses=brain.navigation_reuses,
            navigation=%serde_json::json!(brain.navigation.as_ref().map(|g|g.metrics())),timings=%report,"bot performance");
    }
    Ok((!held).then_some(decision))
}

fn held_movement(grid: &Grid, from: Pos, goal: Pos, distance: f32) -> Option<Pos> {
    let length = from.distance(goal).min(distance);
    // Short key holds fit corners where no full reaction-length octant fits.
    (0..4)
        .flat_map(|scale| {
            (0..8).map(move |index| {
                let heading = index as f32 * std::f32::consts::FRAC_PI_4;
                let length = length / (1 << scale) as f32;
                Pos {
                    x: from.x + heading.cos() * length,
                    y: from.y + heading.sin() * length,
                }
            })
        })
        .filter(|point| {
            from.distance(*point) > 0.1
                && point.distance(goal) < from.distance(goal)
                && grid.segment(from, *point)
        })
        .min_by(|a, b| a.distance(goal).total_cmp(&b.distance(goal)))
}

fn execute_inputs(
    w: &mut crate::world::World,
    session: (i32, i32, i32),
    movement: Option<Pos>,
    aim: Option<Pos>,
    item: Option<Action>,
    reaction: u64,
) -> Result<Option<crate::world::InventoryPlan>> {
    w.bot_inputs(session, movement, aim, reaction)?;
    let item = item.filter(|_| {
        w.players[&session.0]
            .inputs
            .as_ref()
            .is_none_or(|i| i.composition.phase == 0)
    });
    let item = if let Some(Action::Use { slot: 1, x, y }) = item {
        use crate::content::Ability;
        let p = &w.players[&session.0];
        let ability = w.content.get(p.character.items[1] as u16)?;
        let aimed = ability.abilities.iter().any(|a| {
            !matches!(
                a,
                Ability::Heal { .. }
                    | Ability::Condition { .. }
                    | Ability::Clear { .. }
                    | Ability::Cleanse { .. }
                    | Ability::SpiritBomb
            )
        });
        let point = if aimed {
            let p = w
                .players
                .get_mut(&session.0)
                .expect("virtual ability player missing");
            ensure!(
                x.is_finite() && y.is_finite() && p.pos.distance(Pos { x, y }).is_finite(),
                "nonfinite virtual ability point"
            );
            p.inputs
                .as_mut()
                .expect("virtual inputs admitted")
                .aim(p.pos, Pos { x, y })
        } else {
            Pos { x, y }
        };
        Some(Action::Use {
            slot: 1,
            x: point.x,
            y: point.y,
        })
    } else {
        item
    };
    item.map(|action| w.autonomous_action(session.0, action))
        .transpose()
        .map(Option::flatten)
}

async fn public_call(
    app: &App,
    brain: &mut Brain,
    view: &PlayerView,
    decision: &Decision,
) -> Result<()> {
    let id = view.session.player;
    let item_goal = brain.mind.intentions.as_ref().and_then(|i| i.item_goal());
    let idle = brain.activity == Activity::Event
        && !brain.committed(view)
        && decision.goal == Goal::Wait
        && decision.item.is_none()
        && decision.journey.is_none()
        && decision.movement.is_none();
    if brain.aftermath.as_ref().is_some_and(|_| !idle)
        || if let Some(speech) = &brain.aftermath {
            !speech.valid(
                &brain.mind.events,
                view,
                &app.content,
                brain.profile,
                item_goal,
            )?
        } else {
            false
        }
    {
        tracing::info!(
            player = id,
            "bot aftermath composition canceled: context changed"
        );
        brain.close_composition(app, false).await?;
    }
    if brain.draft.is_none() {
        let speech = if idle {
            brain
                .mind
                .events
                .speech(view, &app.content, brain.profile, item_goal, decision.chat)?
        } else {
            None
        };
        let text = if let Some(speech) = speech {
            if !speech.valid(
                &brain.mind.events,
                view,
                &app.content,
                brain.profile,
                item_goal,
            )? {
                return Ok(());
            }
            let text = speech.text.clone();
            brain.aftermath = Some(speech);
            Some(text)
        } else {
            decision
                .chat
                .filter(|text| !events::loot_report(text))
                .map(str::to_owned)
        };
        if let Some(text) = text {
            let _ = brain
                .compose(app, view, composer::Source::Call, &text)
                .await?;
            brain.call_goal = matches!(decision.goal, Goal::Portal(_) | Goal::Follow(_))
                .then(|| decision.goal.clone());
        }
    }
    if brain.composing(composer::Source::Call).is_some()
        && brain
            .call_goal
            .as_ref()
            .is_some_and(|goal| *goal != decision.goal)
    {
        tracing::info!(
            player = id,
            "bot public call canceled: current intent changed"
        );
        brain.close_composition(app, false).await?;
    }
    if let Some(text) = brain.composing(composer::Source::Call).map(str::to_owned)
        && brain
            .compose(app, view, composer::Source::Call, &text)
            .await?
    {
        let received = app.world.clock.now();
        let session = brain.session;
        let companion = brain.companion;
        let call_goal = brain.call_goal.clone();
        let until = brain.invitation_until;
        let text_log = text.clone();
        let aftermath = brain.aftermath.clone();
        let events = brain.mind.events.clone();
        let profile = brain.profile;
        let content = app.content.clone();
        let sent = app
            .world
            .directory(move |d| {
                let current = d.player_view(id)?;
                ensure!(
                    (current.session.world, id, current.session.character) == session,
                    "public call session changed"
                );
                if let Some(speech) = aftermath {
                    ensure!(
                        speech.valid(&events, &current, &content, profile, item_goal)?,
                        "aftermath speech context changed before Enter"
                    );
                }
                if text == "omw" {
                    ensure!(
                        current.time_ms < until
                            && companion.is_some_and(|peer| current
                                .objects
                                .iter()
                                .any(|p| p.id == peer && p.hp > 0)),
                        "acknowledgement contact expired"
                    );
                }
                if let Some(Goal::Portal(portal)) = call_goal {
                    ensure!(
                        current.objects.iter().any(|p| p.id == portal),
                        "public call target left view"
                    );
                }
                if let Some(Goal::Follow(peer)) = call_goal {
                    ensure!(
                        current.objects.iter().any(|p| p.id == peer && p.hp > 0),
                        "public call companion left view"
                    );
                }
                if text.starts_with("hexed;") {
                    ensure!(
                        current.map.name == "Mad Lab"
                            && current.character.conditions
                                & crate::condition::bit(crate::condition::HEXED)
                                != 0,
                        "public call status ended"
                    );
                }
                if text == "blue water clears the green pool" {
                    ensure!(current.map.name == "Mad Lab", "public call context changed");
                }
                let portal = match text.as_str() {
                    "pc here" => Some(0x717),
                    "snake here" => Some(0x718),
                    "lab here" => Some(0x890),
                    _ => None,
                };
                ensure!(
                    portal.is_none_or(|kind| current.map.name == "Realm"
                        && current.objects.iter().any(|p| p.kind == kind)),
                    "public call portal left view"
                );
                d.submit_bot_chat(id, text, received)
            })
            .await;
        brain.close_composition(app, sent.is_ok()).await?;
        match sent {
            Ok(()) => tracing::info!(
                player = id,
                text = text_log,
                "autonomous public call delivered"
            ),
            Err(error) => {
                tracing::warn!(player=id,text=text_log,%error,"autonomous public call rejected")
            }
        }
    }
    Ok(())
}

async fn stop_inputs(app: &App, id: i32) -> Result<()> {
    app.world
        .for_player(id)
        .call(move |w| {
            let p = w
                .players
                .get_mut(&id)
                .context("stopping virtual inputs player missing")?;
            if let Some(input) = &mut p.inputs {
                input.release();
            }
            p.bot_goal = None;
            p.bot_aim = None;
            Ok(())
        })
        .await
}

async fn enter(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Option<Pos>,
) -> Result<(i32, mpsc::Receiver<Packet>, mpsc::Sender<Packet>, Brain)> {
    enter_admitted(app, profile, character, world, position, None, None).await
}
async fn enter_admitted(
    app: &App,
    profile: Profile,
    character: Character,
    world: i32,
    position: Option<Pos>,
    arrival: Option<crate::world::TransferArrival>,
    admission: Option<Vec<u8>>,
) -> Result<(i32, mpsc::Receiver<Packet>, mpsc::Sender<Packet>, Brain)> {
    let world = if world == -3 && position.is_none() && admission.is_none() {
        let account = character.account;
        app.world
            .directory(move |d| {
                let choices = d.available_realms();
                ensure!(
                    !choices.is_empty(),
                    anyhow::anyhow!(CloseReason::Controller).context("no open Realm has capacity")
                );
                realms::choose(account, &choices, &mut rand::thread_rng())
            })
            .await?
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
    let (lost, _) = watch::channel(None);
    let presentation = app.store.presentation(character.account).await?;
    let own_name = character.name.clone();
    let bind = server::lifecycle::binding(app, character.account)?;
    let id = app
        .world
        .directory(move |d| {
            let entry_key = arrival
                .as_ref()
                .map(|_| {
                    admission
                        .clone()
                        .context("autonomous arrival admission key missing")
                })
                .transpose()?;
            if let Some(arrival) = arrival {
                d.admit_transfer(arrival, character.account, character.id, world)?;
            }
            let w = d
                .worlds
                .get_mut(&world)
                .context("autonomous entry world missing")?;
            if let Some(vault) = vault {
                w.load_vaults(vault)?;
            }
            let id = w.enter_admitted(
                character,
                Some(Output { packets, lost }),
                position,
                presentation,
                admission,
            )?;
            bind(id)?;
            w.players
                .get_mut(&id)
                .context("new autonomous player missing")?
                .controller = crate::world::Controller::Autonomous;
            if let Some(key) = entry_key {
                d.complete_dungeon_entry(&key, id)?;
            }
            Ok(id)
        })
        .await?;
    let view = app.world.player_view(id).await?;
    app.cctv
        .entered(profile.name, view.session.player, view.session.account);
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
    brain.mind.market.trader = profile.role == persona::Role::Trader;
    brain.mind.market.observe(&view);
    if let Some(b) = &brain.mind.market.bargain {
        let receipts = app
            .store
            .market_receipts(view.session.account, b.terms.version)
            .await?;
        brain.mind.market.recover(receipts)?;
    }
    brain.mind.future_class = Some(view.character.class);
    let mut generation = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&app.started, &mut generation);
    brain
        .mind
        .bind(&view, std::hash::Hasher::finish(&generation))?;
    brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?;
    Ok((id, output, sender, brain))
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
    entry: lifecycle::drain::Entry,
    request: Reentry,
) -> Result<Entry> {
    // Drain must await already accepted work, not reject it mid-transfer: https://tokio.rs/tokio/topics/shutdown
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
                let mut admission = None;
                let (character, destination, arrival) = match request {
                    Reentry::Collective {
                        destination,
                        time,
                        key,
                        character,
                    } => {
                        admission = Some(key.clone());
                        let (consumed, arrival) = app
                            .world
                            .directory(move |d| d.consume(account, destination, time, key))
                            .await?;
                        ensure!(
                            consumed == character,
                            "collective transfer character binding changed"
                        );
                        server::lifecycle::stage(server::lifecycle::Stage::Store);
                        (
                            app.store.load(account, consumed).await?,
                            destination,
                            Some(arrival),
                        )
                    }
                    Reentry::Portal {
                        destination,
                        time,
                        key,
                        session,
                        selected,
                    } => {
                        let ticket = key.clone();
                        let (consumed, arrival) = app
                            .world
                            .directory(move |d| d.consume(account, destination, time, key))
                            .await?;
                        ensure!(
                            consumed == session.character,
                            "transfer character binding changed"
                        );
                        server::lifecycle::stage(server::lifecycle::Stage::Store);
                        if let Some(selected) = selected {
                            ensure!(
                                selected == consumed,
                                "selected transfer character binding changed"
                            );
                            admission = Some(ticket);
                            (
                                continue_selected(&app.store, account, selected).await?,
                                destination,
                                Some(arrival),
                            )
                        } else if app.store.source_character_alive(account, consumed).await? {
                            let (character, destination) =
                                continue_character(&app, profile, &session, destination).await?;
                            ensure!(
                                character.id == consumed,
                                "live source character changed during autonomous continuation"
                            );
                            admission = Some(ticket);
                            (character, destination, Some(arrival))
                        } else {
                            app.world
                                .directory(move |d| {
                                    d.retire_replacement(arrival, account, consumed, destination)?;
                                    d.cancel_admission(account, ticket)
                                })
                                .await?;
                            let (character, destination) =
                                continue_character(&app, profile, &session, destination).await?;
                            ensure!(
                                character.id != consumed && destination == -2,
                                "committed death did not select explicit replacement"
                            );
                            (character, destination, None)
                        }
                    }
                    Reentry::Replacement { previous } => {
                        server::lifecycle::stage(server::lifecycle::Stage::Store);
                        let character = persona_character(&app, profile).await?;
                        ensure!(
                            character.account == account && character.id != previous,
                            "replacement did not preserve account and replace the dead character"
                        );
                        // A fresh life cannot take the Realm slot already promised to a waiter.
                        tracing::info!(
                            account,
                            dead_character = previous,
                            character = character.id,
                            destination = -2,
                            "autonomous replacement rebuilding arrival selected"
                        );
                        (character, -2, None)
                    }
                };
                server::lifecycle::stage(server::lifecycle::Stage::Publication);
                enter_admitted(
                    &app,
                    profile,
                    character,
                    destination,
                    None,
                    arrival,
                    admission,
                )
                .await
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

pub(super) async fn persona_character(app: &App, profile: Profile) -> Result<Character> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(persona::seed(profile.identity));
    let (roster, vault) = persona::life(&profile, &app.content, &mut rng)?;
    app.store
        .persona_character(
            profile.identity.into(),
            roster,
            vault,
            if profile.role == persona::Role::Trader {
                0
            } else {
                rand::random()
            },
            persona::starting_stock(&profile, &app.content)?,
        )
        .await
}
async fn continue_character(
    app: &App,
    profile: Profile,
    session: &crate::world::view::Session,
    destination: i32,
) -> Result<(Character, i32)> {
    // Departure can commit a death after the transfer was requested.
    let character = if app
        .store
        .source_character_alive(session.account, session.character)
        .await?
    {
        app.store.load(session.account, session.character).await?
    } else {
        persona_character(app, profile).await?
    };
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

pub async fn start(app: &App) -> Result<()> {
    director::start(app).await
}
pub(super) async fn start_test_persona(
    app: &App,
    profile: Profile,
    world: i32,
    position: Pos,
) -> Result<Character> {
    ensure!(
        app.testing.is_some() && app.world.clock.paused(),
        "persona fixture requires a paused prepared test session"
    );
    ensure!(
        app.store
            .shore_account(profile.identity.into())
            .await?
            .is_none(),
        "persona fixture already exists; no reset or implicit replacement"
    );
    let character = persona_character(app, profile).await?;
    let owner = app.claim(character.account)?;
    launch(
        app,
        profile,
        character.clone(),
        (world, Some(position)),
        None,
        owner,
        None,
    )
    .await?;
    Ok(character)
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
        (world, Some(position)),
        Some(selected),
        owner,
        None,
    )
    .await
}
pub(super) async fn launch(
    app: &App,
    profile: Profile,
    character: Character,
    location: (i32, Option<Pos>),
    selected: Option<i32>,
    mut owner: server::Owner,
    done: Option<mpsc::Sender<director::Departure>>,
) -> Result<()> {
    let _names = app.name_claims.lock().await;
    app.store
        .persona_name_available(character.account, character.name.clone())
        .await?;
    let (world, position) = location;
    owner.admit_controller(app)?;
    let mut drain = lifecycle::drain::stopping(app);
    let entry = lifecycle::drain::entry(app)?;
    let (mut cancel, completion) = owner.control(true)?;
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
        let mut ready_reply: Option<(dialogue::Request, dialogue::Reply, String)> = None;
        let mut deferred_market: Option<(Packet, Arc<PlayerView>)> = None;
        let mut admission: Option<Vec<u8>> = None;
        let mut last_dialogue: Option<std::time::Instant> = None;
        let mut phase = adventures::Phase::Seeking;
        let until = done.as_ref().map(|_| {
            app.world.clock.now()
                + Duration::from_secs(
                    rand::thread_rng()
                        .gen_range(app.bot_session_seconds[0]..=app.bot_session_seconds[1]),
                )
        });
        let mut retired = false;
        let mut result: Result<()> = async {
                loop {
                    if *cancel.borrow() || *shutdown.borrow() || drain.has_changed()? || !lifecycle::drain::open(&app) { break; }
                    tokio::select! {
                        _=tokio::time::sleep(Duration::from_millis(50))=>{},
                        _=shutdown.changed()=>break,
                        _=drain.changed()=>break,
                        _=cancel.changed()=>break,
                    }
                    if app.world.clock.paused() { continue; }
                    let _entry = match lifecycle::drain::entry(&app) {
                        Ok(entry) => entry,
                        Err(error) if error.is::<lifecycle::drain::UnadmittedEntry>() => {
                            tracing::info!(player=id,name=profile.name,"autonomous controller stopped before admission");
                            break;
                        }
                        Err(error) => return Err(error),
                    };
                    if let Some(transfer) = server::automatic_transfer(&app, id).await? {
                        dialogue::cancel(&mut pending);
                        ready_reply = None;
                        dialogue::end_agreement(&app, &mut brain, "world changed").await;
                        ensure!(transfer.player == id && transfer.output.packets.same_channel(&sender), "collective transfer does not belong to autonomous output");
                        while output.try_recv().is_ok() {}
                        deferred_market = None;
                        let Some(destination) = transfer.destination else { return Ok(()); };
                        admission = Some(transfer.key.clone());
                        let character_id = brain.session.2;
                        (id, output, sender, brain) = reenter(&app, profile, account, &mut owner, _entry, Reentry::Collective { destination, time: transfer.key_time, key: transfer.key, character: character_id }).await?;
                        admission = None;
                        tracing::info!(player=id,account,character=character_id,destination,"autonomous collective transfer completed");
                        continue;
                    }
                    #[cfg(test)]
                    let audit_due = (selected.is_some(), brain.next_decision);
                    let view=Arc::new(app.world.player_view(id).await?);
                    #[cfg(test)]
                    if audit_due.0 && view.tick>=audit_due.1 {
                        let audit_view=view.clone();
                        app.world.directory(move|d| {
                            if let Some(audit)=d.prepared_postclear_key_audit(Some(&audit_view))? { tracing::info!(audit=%audit,"prepared postclear evaluator key audit"); }
                            Ok(())
                        }).await?;
                    }
                    ensure!(view.session.account == account, "autonomous account binding changed");
                    let composing = brain.draft.is_some();
                    brain.advance_composition(&app, &view).await?;
                    if composing && brain.draft.is_none() { ready_reply = None; }
                    app.cctv.inputs(profile.name, &view);
                    if let Some(blocked) = view.inputs.as_ref().and_then(|input| input.blocked) {
                        let tile = tile(blocked);
                        if brain.blocked.get(&tile).is_none_or(|until| *until <= view.tick) {
                            brain.blocked.insert(tile, view.tick + 100);
                            tracing::warn!(player=id,tick=view.tick,x=blocked.x,y=blocked.y,"virtual held movement blocked; route excluded");
                        }
                    }
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
                        dialogue::cancel(&mut pending);
                        ready_reply = None;
                        dialogue::end_agreement(&app, &mut brain, "character died").await;
                        brain.mind.end(&view, "character_died_steps_invalidated");
                        brain.mind.market.finish("character_died_remaining_balance_unenforced");
                        brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?;
                        lifecycle::finalize(&app, id, CloseReason::Controller)?.wait().await?;
                        if selected.is_some() { return Ok(()); }
                        if until.is_some_and(|deadline| app.world.clock.now() >= deadline) {
                            retired = true;
                            return Ok(());
                        }
                        if *cancel.borrow() { return Ok(()); }
                        tokio::select! {
                            _=tokio::time::sleep(Duration::from_secs(3))=>{},
                            _=shutdown.changed()=>return Ok(()),
                            _=cancel.changed()=>return Ok(()),
                        }
                        (id, output, sender, brain) = reenter(&app, profile, account, &mut owner, _entry, Reentry::Replacement { previous: view.session.character }).await?;
                        deferred_market = None;
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
                    brain.crowd.observe(&view, &app.content);
                    while let Some((packet, received_view)) = deferred_market.take().or_else(||output.try_recv().ok().map(|p|(p,view.clone()))) {
                        if packet.id == crate::wire::GOTO {
                            app.world.for_player(id).call(move |w| w.acknowledge_autonomous_goto(id)).await?;
                            continue;
                        }
                        app.cctv.heard(profile.name, id, &packet)?;
                        let settled_view = if matches!(packet.id,12|23|67) { Some(app.world.player_view(id).await?) } else { None };
                        let social_view = settled_view.as_ref().unwrap_or(&received_view);
                        brain.crowd.receive(&packet, social_view, &app.content)?;
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
                            // A rate-limited correction must still invalidate the older promise.
                            dialogue::supersede(&mut pending, &request);
                            if let Some((old, _, _)) = &ready_reply
                                && old.speaker == request.speaker {
                                ready_reply = None;
                                if brain.composing(composer::Source::Dialogue).is_some() { brain.close_composition(&app, false).await?; }
                            }
                            if pending.is_some() || ready_reply.is_some() || last_dialogue.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
                                dialogue::skipped(&request, None, "interaction_busy");
                            } else {
                                last_dialogue = Some(std::time::Instant::now());
                                pending = Some(dialogue::Pending::start(Some(service.clone()), request, profile));
                            }
                            continue;
                        }
                        if brain.receive(packet, &view, app.dialogue.is_none(), &app.content)? {
                            if brain.draft.is_none() {
                                let _ = brain.compose(&app, &view, composer::Source::Call, "omw").await?;
                                brain.call_goal = brain.companion.map(Goal::Follow);
                            }
                            tracing::info!(player=id, companion=brain.companion, "visible invitation accepted");
                        }
                    }
                    if deferred_market.is_some() {
                        stop_inputs(&app, id).await?;
                        continue;
                    }
                    if pending.is_none() && ready_reply.is_none() && brain.mind.market.outgoing.is_none() && brain.mind.market.bargain.as_ref().is_none_or(|b|b.resolution.is_some()) && app.dialogue.as_ref().is_none_or(|service|service.automatic_available()) {
                        let plan=app.society.lock().expect("adventure registry poisoned").next(&brain,&view,&app.content,app.dialogue.is_some());
                        if let Some(plan)=plan {
                            let request=brain.adventure_request(plan,&view,&app.content)?;
                            pending=Some(dialogue::Pending::start(app.dialogue.clone(),request,profile));
                        }
                    }
                    if pending.as_ref().is_some_and(|p| p.task.is_finished()) {
                        let mut completed = pending.take().expect("finished dialogue missing");
                        let result = match (&mut completed.task).await {
                            Ok(result) => result,
                            Err(error) => {
                                tracing::error!(request=%completed.id,%error,"bot dialogue worker failed");
                                Err(dialogue::Failure::Error("worker_failed"))
                            }
                        };
                        match result {
                            Ok(reply) => {
                                ready_reply = Some((completed.request.clone(), reply, completed.id.clone()));
                            }
                            Err(dialogue::Failure::LocalCapacity) => dialogue::skipped(&completed.request, Some(&completed.id), "local_capacity"),
                            Err(dialogue::Failure::Error(reason)) => dialogue::failed(&app, &completed.request, &completed.id, reason).await,
                        }
                    }
                    brain.proposing=pending.as_ref().is_some_and(|p|matches!(p.request.plan,Some(adventures::Plan::Invite{..})))
                        || ready_reply.as_ref().is_some_and(|(r,_,_)|matches!(r.plan,Some(adventures::Plan::Invite{..})));
                    let ending = until.is_some_and(|deadline| app.world.clock.now() >= deadline)
                        && pending.is_none() && ready_reply.is_none() && brain.draft.is_none() && director::may_finish(&app, &mut brain, &view);
                    if ending && view.map.name == "Nexus" {
                        retired = true;
                        break;
                    }
                    if view.lifecycle.busy { continue; }
                    if view.tick < brain.next_decision { continue; }
                    // World clocks start at different times; banking delay belongs to this account session.
                    let now = app.world.clock.now();
                    brain.next_bank = view.time_ms + u64::try_from(next_bank.saturating_duration_since(now).as_millis())?;
                    let Some(mut decision) = decide_before_social(&app, &mut brain, &view).await? else {
                        if brain.activity == Activity::Survive { ready_reply = None; }
                        stop_inputs(&app, id).await?;
                        continue;
                    };
                    if brain.activity == Activity::Survive { ready_reply = None; decision.chat = None; }
                    if let Some((request, reply, request_id)) = ready_reply.take() {
                        let validation = brain.validate_dialogue(&request, &reply, &view);
                        if validation.is_ok() && !brain.compose(&app, &view, composer::Source::Dialogue, &reply.reply).await? {
                            if brain.draft.is_some() { ready_reply = Some((request, reply, request_id)); }
                        } else {
                            let result = match validation {
                                Ok(()) => dialogue::publish(&app, &mut brain, &request, &reply, &view).await,
                                Err(error) => Err(error),
                            };
                            if brain.composing(composer::Source::Dialogue).is_some() {
                                brain.close_composition(&app, matches!(result, Ok(true))).await?;
                            }
                            match result {
                                Ok(true) => tracing::info!(request=%request_id,player=id,intent=?reply.intent,"bot dialogue intention applied"),
                                Ok(false) => {},
                                Err(error) if error.is::<dialogue::Obsolete>() => tracing::info!(request=%request_id,player=id,%error,"bot dialogue canceled; pending intention not published"),
                                Err(error) => {
                                    tracing::error!(request=%request_id,player=id,%error,"bot dialogue intention rejected");
                                    dialogue::failed(&app, &request, &request_id, "rejected").await;
                                }
                            }
                        }
                    }
                    if ending && director::may_finish(&app, &mut brain, &view) && decision.journey.is_none() {
                        decision.journey = Some(Journey::Nexus);
                        decision.chat = None;
                        tracing::info!(account,name=profile.name,"session finished; returning to Nexus before logout");
                        app.cctv.decided(profile.name, &view, &decision, "session finished; returning to Nexus before logout".into(), &serde_json::json!({"goal":null,"plan":"End this session","step":"Return to Nexus before logout","mode":"necessary","reason":"session deadline reached","resume":null}), &app.content);
                    }
                    if events::shared(&view.map.name) {
                        tracing::info!(account,character=view.session.character,world=%view.map.name,tick=view.tick,time_ms=view.time_ms,
                            hp=view.character.hp,max_hp=view.character.stats[0],mp=view.character.mp,recovery=profile.recovery,
                            held_health=view.character.items.iter().skip(4).filter(|k|**k==0xa22).count(),
                            goal=?decision.goal,journey=?decision.journey,movement=?decision.movement,
                            "autonomous event action selected");
                    }
                    #[cfg(test)]
                    let postclear_choice_sha = if selected.is_some() && brain.mind.events.postclear_observed(&view) {
                        use sha2::Digest;
                        let raw = serde_json::to_string(&*view)?;
                        let hash = format!("{:x}", sha2::Sha256::digest(raw.as_bytes()));
                        tracing::info!(account,character=view.session.character,normal_view_sha256=%hash,session=%serde_json::to_string(&view.session)?,tick=view.tick,time_ms=view.time_ms,goal=?decision.goal,journey=?decision.journey,item=?decision.item,movement=?decision.movement,"prepared postclear final selected decision");
                        Some(hash)
                    } else { None };
                    next_bank = now.checked_add(Duration::from_millis(brain.next_bank.saturating_sub(view.time_ms))).context("autonomous bank cooldown overflow")?;
                    public_call(&app, &mut brain, &view, &decision).await?;
                    if brain.draft.is_some() {
                        decision.movement = None; decision.aim = None; decision.item = None; decision.journey = None;
                    }
                    if !brain.decision_logged || decision.goal != brain.goal {
                        tracing::info!(account,character=view.session.character,world=view.session.world,player=id,name=profile.name,tick=view.tick,hp=view.character.hp,
                            experience=view.character.experience,x=view.position.x,y=view.position.y,
                            aim=?decision.aim,movement=?decision.movement,
                            goal=?decision.goal,"autonomous decision");
                        brain.decision_logged = true;
                    }
                    brain.goal = decision.goal;
                    brain.destination = decision.movement;
                    let diagnostic = format!("movement={:?} cursor={:?} item={:?}", decision.movement, decision.aim, decision.item);
                    let input_session = (id, account, view.session.character);
                    let input_grid = brain.navigation.clone().context("virtual input navigation missing")?;
                    let held_distance = (combat::speed(&view) * profile.reaction as f32 * 0.05).max(0.5);
                    let movement = decision.journey.is_none().then_some(decision.movement).flatten();
                    let aim = decision.journey.is_none().then_some(decision.aim).flatten();
                    let item = decision.journey.is_none().then_some(decision.item).flatten();
                    match server::inventory(&app, id, move |w| {
                        let movement = movement.and_then(|goal| held_movement(&input_grid, w.players[&id].pos, goal, held_distance));
                        execute_inputs(w, input_session, movement, aim, item, profile.reaction)
                    }).await? {
                        Ok(true) => {
                            let after = app.world.player_view(id).await?;
                            let prior_events = brain.mind.events.clone();
                            brain.mind.events.committed(&after)?;
                            if brain.mind.events != prior_events { brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?; }
                            tracing::info!(account, character=after.session.character, world=after.session.world,
                                before_revision=view.character.inventory_revision, revision=after.character.inventory_revision,
                                items=?after.character.items, actions=diagnostic, "autonomous inventory outcome");
                        },
                        Ok(false) => { brain.mind.events.rejected(); },
                        Err(error) => {
                            brain.mind.events.rejected();
                            stop_inputs(&app, id).await?;
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
                                    d.bind_exit(id,portal,&key)?;
                                    let reservation=d.reserve_transfer(key.clone())?;
                                    Ok((destination, time, key,reservation))
                                }).await;
                                let (destination, time, key,reservation) = match request {
                                    Ok(request) => request,
                                    Err(error) => {
                                        brain.portal = None;
                                        tracing::warn!(player=id,?portal,%error,"autonomous transfer rejected before departure");
                                        continue;
                                    }
                                };
                                admission = Some(key.clone());
                                dialogue::cancel(&mut pending);
                                ready_reply = None;
                                dialogue::end_agreement(&app, &mut brain, "world changed").await;
                                lifecycle::finalize_transfer(&app, id,reservation)?.wait().await?;
                                let previous = view.session.character;
                                (id, output, sender, brain) = reenter(&app, profile, account, &mut owner, _entry, Reentry::Portal { destination, time, key, session: crate::world::view::Session { world: view.session.world, player: view.session.player, account: view.session.account, character: view.session.character }, selected }).await?;
                                let current_character = brain.session.2;
                                let destination = if current_character != previous { -2 } else { destination };
                                if current_character != previous {
                                    tracing::info!(account,character=previous,replacement=current_character,"autonomous transfer interrupted by permanent death");
                                }
                                admission = None;
                                tracing::info!(player=id,account,character=current_character,destination,?portal,portal_kind,"autonomous durable transfer completed");
                                #[cfg(test)]
                                if let Some(hash) = postclear_choice_sha {
                                    tracing::info!(normal_view_sha256=%hash,origin_world=view.session.world,origin_player=view.session.player,account,character=current_character,destination,player=id,tick=view.tick,time_ms=view.time_ms,?portal,portal_kind,"prepared postclear durable transfer ACK");
                                }
                            }
                        }
                    }
                }
                let view = app.world.player_view(id).await?;
                brain.mind.reconcile(&view)?;
                brain.mind.revision = app.store.save_bot_mind(brain.mind.clone()).await?;
                Ok(())
            }.await;
        if let Some(key) = admission
            && let Err(error) = app
                .world
                .directory(move |d| d.cancel_admission(account, key))
                .await
        {
            let message = format!("controller admission cancellation failed: {error:#}");
            result = Err(match result {
                Ok(()) => error.context(message),
                Err(prior) => prior.context(message),
            });
        }
        if *cancel.borrow()
            && let Err(error) = &result
            && let Some(owner) = &owner
        {
            owner.failed(format!("persona name relinquishment failed: {error:#}"));
        }
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
        let mut finalized = true;
        if let Some(closing) = closing {
            let saved = match closing {
                Ok(Some(completion)) => completion.wait().await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            if let Err(error) = saved {
                finalized = false;
                tracing::error!(player=id,account,%error,"autonomous finalization failed; account remains fenced");
                result = Err(error);
            }
        }
        #[cfg(test)]
        if finalized
            && let Err(error) =
                crate::server::name_claims_tests::departure_gate(&app, account).await
        {
            finalized = false;
            result = Err(error);
        }
        if finalized {
            app.cctv.departed(profile.name, id, account);
        }
        dialogue::cancel(&mut pending);
        dialogue::end_agreement(&app, &mut brain, "controller stopped").await;
        let retire = retired
            && profile.identity.starts_with("guest:v1:")
            && finalized
            && result.is_ok()
            && !*shutdown.borrow();
        if let Err(error) = &result {
            tracing::error!(player=id,name=profile.name,%error,"autonomous controller stopped");
        }
        if let Some(done) = done
            && !*shutdown.borrow()
            && done
                .send(director::Departure {
                    profile,
                    account,
                    retire,
                })
                .await
                .is_err()
        {
            tracing::error!(account, "population director stopped receiving departures");
        }
        completion.complete(result);
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

#[cfg(test)]
mod composer_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
pub(super) mod tests;
