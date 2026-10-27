use super::*;
use crate::bots::composer::Source;

async fn step(app: &App, id: i32) -> PlayerView {
    app.world
        .for_player(id)
        .call(move |w| {
            w.step();
            Ok(())
        })
        .await
        .unwrap();
    app.world.player_view(id).await.unwrap()
}
async fn typed(app: &App, brain: &mut Brain, source: Source, text: &str) -> PlayerView {
    let mut view = app.world.player_view(brain.session.1).await.unwrap();
    assert!(!brain.compose(app, &view, source, text).await.unwrap());
    for _ in 0..140 {
        view = step(app, view.session.player).await;
        brain.advance_composition(app, &view).await.unwrap();
        if brain.compose(app, &view, source, text).await.unwrap() {
            return view;
        }
    }
    panic!("draft did not reach Enter");
}
fn text_sent(output: &mut mpsc::Receiver<Packet>, id: i32) -> bool {
    let mut sent = false;
    while let Ok(packet) = output.try_recv() {
        if packet.id == wire::TEXT {
            let mut r = Reader::new(&packet.body);
            r.utf().unwrap();
            sent |= r.i32().unwrap() == id;
        }
    }
    sent
}
fn swap(view: &PlayerView) -> Action {
    let from = if view.character.items[4] >= 0 { 4 } else { 5 };
    let slot = |slot| Slot {
        object: view.session.player,
        slot,
        kind: view.character.items[slot],
    };
    Action::Swap {
        from: slot(from),
        to: slot(9 - from),
    }
}
fn decision(goal: Goal, chat: Option<&'static str>) -> Decision {
    Decision {
        goal,
        chat,
        movement: None,
        aim: None,
        item: None,
        journey: None,
    }
}

#[tokio::test]
async fn review_enter_revision_race_cancels_call_and_dialogue_before_text_or_society() {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, mut output, mut brain) = tests::actor(
        &app,
        "enter-race",
        "Trader",
        [0xa22, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    let (_peer_owner, peer, _peer_output, _) =
        tests::actor(&app, "enter-race-peer", "Peer", [-1; 8]).await;
    step(&app, id).await;
    brain.companion = Some(peer);
    brain.call_goal = Some(Goal::Follow(peer));
    let ready = typed(&app, &mut brain, Source::Call, "omw").await;
    brain.invitation_until = ready.time_ms + 60000;
    let action = swap(&ready);
    assert!(
        server::inventory(&app, id, move |w| w.autonomous_action(id, action))
            .await
            .unwrap()
            .unwrap()
    );
    let committed = app.world.player_view(id).await.unwrap();
    assert_eq!(
        committed.character.inventory_revision,
        ready.character.inventory_revision + 1
    );
    assert_eq!(committed.inputs.unwrap().composition.text, "omw");
    public_call(
        &app,
        &mut brain,
        &ready,
        &decision(Goal::Follow(peer), None),
    )
    .await
    .unwrap();
    let call_sent = text_sent(&mut output, id);

    let view = app.world.player_view(id).await.unwrap();
    let peer_account = app.world.player_view(peer).await.unwrap().session.account;
    let request = brain
        .adventure_request(
            adventures::Plan::Invite {
                peer: adventures::Member {
                    account: peer_account,
                    name: "Peer".into(),
                },
                purpose: adventures::Purpose::Equipment,
            },
            &view,
            &app.content,
        )
        .unwrap();
    let reply = dialogue::Reply {
        intent: dialogue::Intent::Invite,
        reply: "Peer, pc?".into(),
    };
    let ready = typed(&app, &mut brain, Source::Dialogue, &reply.reply).await;
    let action = swap(&ready);
    assert!(
        server::inventory(&app, id, move |w| w.autonomous_action(id, action))
            .await
            .unwrap()
            .unwrap()
    );
    let result = dialogue::publish(&app, &mut brain, &request, &reply, &ready).await;
    let canceled = matches!(&result,Err(error) if error.is::<dialogue::Obsolete>());
    let dialogue_sent = text_sent(&mut output, id);
    let (offered, recorded) = {
        let mut society = app.society.lock().unwrap();
        (
            society
                .outgoing(view.session.account, view.elapsed_ms)
                .is_some(),
            society
                .speech
                .repeated(view.session.world, view.elapsed_ms, "pc?"),
        )
    };
    brain.close_composition(&app, false).await.unwrap();
    for player in [id, peer] {
        lifecycle::finalize(&app, player, CloseReason::Controller)
            .unwrap()
            .wait()
            .await
            .unwrap();
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(
        (call_sent, canceled, dialogue_sent, offered, recorded),
        (false, true, false, false, false)
    );
}

#[tokio::test]
async fn review_informational_event_calls_survive_planning_changes_and_reach_enter() {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, mut output, mut brain) =
        tests::actor(&app, "info-call", "Trader", [-1; 8]).await;
    let (_peer_owner, peer, _peer_output, _) =
        tests::actor(&app, "info-peer", "Peer", [-1; 8]).await;
    app.world
        .for_player(id)
        .call(move |w| {
            w.name = "Mad Lab".into();
            let p = w.players.get_mut(&id).unwrap();
            p.character.level = 20;
            p.character.exp = crate::store::level_start(20)?;
            p.character.base[3] = 10;
            p.character.items[0] = 0xa9e;
            Ok(())
        })
        .await
        .unwrap();
    for advice in [true, false] {
        brain.mind.events.bind();
        brain.mind.events.knows_blue = advice;
        if !advice {
            app.world
                .for_player(id)
                .call(move |w| w.apply_condition(id, crate::condition::HEXED, 10000))
                .await
                .unwrap();
        }
        let view = step(&app, id).await;
        brain.mind.events.observe(&view);
        if advice {
            let mut text = crate::wire::Writer::new();
            text.utf("Peer")
                .i32(peer)
                .i32(0)
                .u8(5)
                .utf("")
                .utf("hexed; need help")
                .utf("");
            brain
                .mind
                .events
                .receive(
                    &Packet {
                        id: wire::TEXT,
                        body: text.0,
                    },
                    &view,
                    "Trader",
                )
                .unwrap();
        }
        let initial = brain
            .mind
            .events
            .decide(&view, &app.content, &Grid::terrain(&view), brain.profile)
            .unwrap()
            .unwrap();
        let text = if advice {
            "blue water clears the green pool"
        } else {
            "hexed; need help"
        };
        assert_eq!(initial.chat, Some(text));
        assert_eq!(initial.goal, Goal::Wait);
        public_call(&app, &mut brain, &view, &initial)
            .await
            .unwrap();
        let mut changed = false;
        let mut sent = false;
        for _ in 0..80 {
            let view = step(&app, id).await;
            brain.advance_composition(&app, &view).await.unwrap();
            if view.tick >= brain.next_decision {
                let planned = brain.decide(&view, &app.content).unwrap();
                changed |= planned.goal != Goal::Wait;
                public_call(&app, &mut brain, &view, &planned)
                    .await
                    .unwrap();
            }
            sent |= text_sent(&mut output, id);
            if sent || brain.draft.is_none() {
                break;
            }
        }
        assert!(
            changed,
            "ordinary planning must change the placeholder goal"
        );
        assert!(
            sent,
            "still-valid one-shot information must reach Enter: {text}"
        );
    }
    brain.close_composition(&app, false).await.unwrap();
    for player in [id, peer] {
        lifecycle::finalize(&app, player, CloseReason::Controller)
            .unwrap()
            .wait()
            .await
            .unwrap();
    }
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn review_long_inventory_pending_keeps_sampled_cursor_without_gameplay_controls() {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, _output, _) = tests::actor(
        &app,
        "busy-cursor",
        "Trader",
        [0xa22, -1, -1, -1, -1, -1, -1, -1],
    )
    .await;
    let view = app.world.player_view(id).await.unwrap();
    let session = (id, view.session.account, view.session.character);
    let target = Pos {
        x: view.position.x + 3.,
        y: view.position.y,
    };
    app.world
        .for_player(id)
        .call(move |w| {
            w.bot_inputs(session, Some(target), Some(target), 4)?;
            w.step();
            Ok(())
        })
        .await
        .unwrap();
    let view = app.world.player_view(id).await.unwrap();
    let point = view.inputs.as_ref().unwrap().cursor.unwrap();
    let action = swap(&view);
    lifecycle::account(&app, vec![session.1], "inventory", move |app| async move {
        let plan = app
            .world
            .for_player(id)
            .call(move |w| w.autonomous_action(id, action))
            .await?
            .unwrap();
        app.world
            .for_player(id)
            .call(move |w| {
                assert!(w.bot_inputs(session, None, None, 4).is_err());
                Ok(())
            })
            .await?;
        let began = std::time::Instant::now();
        for _ in 0..26 {
            let packets = app
                .world
                .spectator_inputs(view.session.world, id, session.1, session.2, 2)
                .await?
                .expect("ordinary inventory busy must not hide retained actual cursor");
            assert_eq!(
                f32::from_be_bytes(packets[0].body[12..16].try_into().unwrap()),
                point.x
            );
            assert_eq!(
                f32::from_be_bytes(packets[0].body[16..20].try_into().unwrap()),
                point.y
            );
            assert_eq!(
                packets[0].body[20], 0,
                "busy sampling cannot show held fire"
            );
            assert_eq!(&packets[1].body[12..], &[0, 0, 0]);
            step(&app, id).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(began.elapsed() >= Duration::from_millis(1250));
        let revision = app
            .store
            .exchange(
                vec![crate::store::InventoryWrite {
                    id: plan.character,
                    account: plan.account,
                    revision: plan.revision,
                    items: plan.items.clone(),
                }],
                vec![],
            )
            .await?
            .remove(0);
        app.world
            .directory(move |d| d.finish_inventory(plan, Ok(revision)))
            .await?;
        let after = app.world.player_view(id).await?;
        assert!(
            !after.lifecycle.busy
                && after.character.inventory_revision == view.character.inventory_revision + 1
        );
        assert!(!after.inputs.unwrap().fire);
        Ok(())
    })
    .await
    .unwrap();
    for (world, account, character) in [
        (view.session.world + 1, session.1, session.2),
        (view.session.world, session.1 + 1, session.2),
        (view.session.world, session.1, session.2 + 1),
    ] {
        assert!(
            app.world
                .spectator_inputs(world, id, account, character, 2)
                .await
                .unwrap()
                .is_none()
        );
    }
    lifecycle::finalize(&app, id, CloseReason::Controller)
        .unwrap()
        .wait()
        .await
        .unwrap();
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn aftermath_request_composition_publishes_only_current_missing_own_goal() {
    for acquire in [None, Some(true), Some(false)] {
        let (dir, app) = server::test_app();
        app.world.clock.set_paused(true);
        let (_owner, id, mut output, mut brain) = tests::actor(
            &app,
            "aftermath-request",
            "Trader",
            [0xa9e, -1, -1, -1, -1, -1, -1, -1],
        )
        .await;
        let (_peer_owner, peer, _peer_output, _) =
            tests::actor(&app, "aftermath-audience", "Peer", [-1; 8]).await;
        app.world
            .for_player(id)
            .call(move |w| {
                w.name = "Wine Cellar".into();
                let p = w.players.get_mut(&id).unwrap();
                p.character.level = 20;
                p.character.items[0] = 0xa9e;
                p.character.base[3] = 10;
                Ok(())
            })
            .await
            .unwrap();
        brain.profile.identity = "Cedar";
        brain.profile.companions = 8.;
        brain.activity = Activity::Event;
        let mut view = step(&app, id).await;
        brain
            .mind
            .intentions
            .as_mut()
            .unwrap()
            .observe(&view, &app.content, "Mossy")
            .unwrap();
        let goal = brain.mind.intentions.as_ref().unwrap().item_goal().unwrap();
        // Only the event observation is prepared; publication still reads the actual world.
        let mut cue = crate::bots::quest_tests::fixture().1.objects.remove(0);
        cue.kind = crate::endgame::ORYX2;
        cue.position = view.position;
        view.objects.push(cue);
        brain.mind.events.observe(&view);
        let cue = view.objects.last_mut().unwrap();
        cue.kind = 0x71d;
        cue.hp = 0;
        cue.usable = true;
        brain.mind.events.observe(&view);
        for _ in 0..20 {
            view = step(&app, id).await;
        }
        let speech = brain
            .mind
            .events
            .speech(&view, &app.content, brain.profile, Some(goal), None)
            .unwrap()
            .unwrap();
        let text = speech.text.clone();
        assert!(
            !brain
                .compose(&app, &view, Source::Call, "snake here")
                .await
                .unwrap()
        );
        public_call(&app, &mut brain, &view, &decision(Goal::Wait, None))
            .await
            .unwrap();
        assert_eq!(brain.composing(Source::Call), Some("snake here"));
        brain.close_composition(&app, false).await.unwrap();
        brain.aftermath = Some(speech);
        let ready = typed(&app, &mut brain, Source::Call, &text).await;
        assert!(
            brain
                .aftermath
                .as_ref()
                .unwrap()
                .valid(
                    &brain.mind.events,
                    &ready,
                    &app.content,
                    brain.profile,
                    Some(goal)
                )
                .unwrap()
        );
        if acquire == Some(true) {
            app.world
                .for_player(id)
                .call(move |w| {
                    // Exercise current ownership independently of the composer's revision guard.
                    w.players.get_mut(&id).unwrap().character.items[5] = goal.1;
                    Ok(())
                })
                .await
                .unwrap();
        } else if acquire == Some(false) {
            brain.mind.intentions = Some(Default::default());
        }
        public_call(&app, &mut brain, &ready, &decision(Goal::Wait, None))
            .await
            .unwrap();
        assert_eq!(text_sent(&mut output, id), acquire.is_none());
        assert!(brain.draft.is_none());
        assert!(brain.aftermath.is_none());
        for player in [id, peer] {
            lifecycle::finalize(&app, player, CloseReason::Controller)
                .unwrap()
                .wait()
                .await
                .unwrap();
        }
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
