use super::*;
use crate::bots::composer::Source;

#[tokio::test]
async fn composer_types_before_public_chat_v2_is_atomic_and_owned_ads_really_paste() {
    let (dir, app) = server::test_app();
    app.world.clock.set_paused(true);
    let (_owner, id, mut output, mut brain) =
        tests::actor(&app, "composer", "Trader", [-1; 8]).await;
    let mut view = app.world.player_view(id).await.unwrap();
    let text = "wts def é😀";
    assert!(
        !brain
            .compose(&app, &view, Source::Call, text)
            .await
            .unwrap()
    );
    brain.mind.social.outgoing = Some("Realm is full, waiting here".into());
    assert!(
        !social::life(&app, &mut brain, &view).await.unwrap(),
        "another source cannot starve the current composer"
    );
    brain.mind.social.outgoing = None;
    let received = app.world.clock.now();
    assert!(
        app.world
            .directory(move |d| d.submit_bot_chat(id, text.into(), received))
            .await
            .is_err()
    );
    let initial = app.world.player_view(id).await.unwrap().inputs.unwrap();
    assert_eq!(
        (initial.composition.phase, initial.composition.text.as_str()),
        (1, "")
    );
    let mut observed = Vec::new();
    for _ in 0..32 {
        app.world
            .for_player(id)
            .call(move |w| {
                w.step();
                Ok(())
            })
            .await
            .unwrap();
        view = app.world.player_view(id).await.unwrap();
        brain.advance_composition(&app, &view).await.unwrap();
        let current = app.world.player_view(id).await.unwrap();
        let input = current.inputs.as_ref().unwrap();
        assert!(input.keys == [0, 0] && !input.fire);
        observed.push(input.composition.text.clone());
        let packets = app
            .world
            .spectator_inputs(
                view.session.world,
                id,
                view.session.account,
                view.session.character,
                2,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!((packets[0].id, packets[0].body.len()), (250, 29));
        let mut cursor = Reader::new(&packets[0].body);
        assert_eq!(cursor.i32().unwrap(), id);
        assert_eq!(cursor.i32().unwrap(), view.session.character);
        assert_eq!(cursor.i32().unwrap() as u32, view.tick as u32);
        assert_eq!(cursor.f32().unwrap(), input.cursor.unwrap().x);
        assert_eq!(cursor.f32().unwrap(), input.cursor.unwrap().y);
        assert_eq!(cursor.u8().unwrap(), 0);
        assert_eq!(cursor.f32().unwrap(), current.position.x);
        assert_eq!(cursor.f32().unwrap(), current.position.y);
        cursor.done().unwrap();
        let mut draft = Reader::new(&packets[1].body);
        assert_eq!(packets[1].id, 251);
        assert_eq!(draft.i32().unwrap(), id);
        assert_eq!(draft.i32().unwrap(), view.session.character);
        assert_eq!(draft.i32().unwrap() as u32, view.tick as u32);
        assert_eq!(draft.u8().unwrap(), 1);
        assert_eq!(draft.utf().unwrap(), input.composition.text);
        draft.done().unwrap();
        while let Ok(packet) = output.try_recv() {
            assert_ne!(packet.id, wire::TEXT, "typing cannot publish chat");
        }
        if brain
            .compose(&app, &view, Source::Call, text)
            .await
            .unwrap()
        {
            break;
        }
    }
    assert!(observed.iter().any(|s| s == "w"));
    assert_eq!(observed.last().unwrap(), text);
    let received = app.world.clock.now();
    app.world
        .directory(move |d| d.submit_bot_chat(id, text.into(), received))
        .await
        .unwrap();
    brain.close_composition(&app, true).await.unwrap();
    assert!(std::iter::from_fn(|| output.try_recv().ok()).any(|p| p.id == wire::TEXT));
    let closed = app
        .world
        .spectator_inputs(
            view.session.world,
            id,
            view.session.account,
            view.session.character,
            2,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(closed[1].body[12], 0);
    assert_eq!(&closed[1].body[13..], &[0, 0]);
    assert_eq!(
        app.world
            .spectator_inputs(
                view.session.world,
                id,
                view.session.account,
                view.session.character,
                1
            )
            .await
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    assert!(
        brain
            .compose(&app, &view, Source::Call, "/tell Secret hidden")
            .await
            .is_err()
    );
    assert!(
        brain
            .compose(&app, &view, Source::Call, " /g private")
            .await
            .is_err()
    );
    let message = pursuits::market::Message {
        text: text.into(),
        channel: pursuits::market::Channel::Local,
    };
    brain.mind.market.outgoing = Some(message.clone());
    brain.market_words = Some((message.clone(), text.into()));
    assert!(
        !brain
            .compose(&app, &view, Source::Market, text)
            .await
            .unwrap()
    );
    assert_eq!(
        app.world
            .player_view(id)
            .await
            .unwrap()
            .inputs
            .unwrap()
            .composition
            .phase,
        1
    );
    // Completing a genuinely typed owned ad populates only this bot's clipboard.
    for _ in 0..32 {
        app.world
            .for_player(id)
            .call(move |w| {
                w.step();
                Ok(())
            })
            .await
            .unwrap();
        view = app.world.player_view(id).await.unwrap();
        brain.advance_composition(&app, &view).await.unwrap();
        if brain
            .compose(&app, &view, Source::Market, text)
            .await
            .unwrap()
        {
            break;
        }
    }
    let received = app.world.clock.now();
    app.world
        .directory(move |d| d.submit_bot_chat(id, text.into(), received))
        .await
        .unwrap();
    brain.close_composition(&app, true).await.unwrap();
    brain.market_words = Some((message, text.into()));
    assert!(
        !brain
            .compose(&app, &view, Source::Market, text)
            .await
            .unwrap()
    );
    for _ in 0..2 {
        app.world
            .for_player(id)
            .call(move |w| {
                w.step();
                Ok(())
            })
            .await
            .unwrap();
    }
    view = app.world.player_view(id).await.unwrap();
    brain.advance_composition(&app, &view).await.unwrap();
    let pasted = app
        .world
        .player_view(id)
        .await
        .unwrap()
        .inputs
        .unwrap()
        .composition;
    assert_eq!((pasted.phase, pasted.text.as_str()), (2, text));
    assert!(
        !brain
            .compose(&app, &view, Source::Market, text)
            .await
            .unwrap(),
        "paste must be observable before Enter"
    );
    app.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().character.revision += 1;
            Ok(())
        })
        .await
        .unwrap();
    view = app.world.player_view(id).await.unwrap();
    brain.advance_composition(&app, &view).await.unwrap();
    assert!(brain.draft.is_none() && brain.mind.market.outgoing.is_none());
    assert_eq!(
        app.world
            .player_view(id)
            .await
            .unwrap()
            .inputs
            .unwrap()
            .composition
            .phase,
        0
    );
    app.world
        .for_player(id)
        .call(move |w| {
            w.players.get_mut(&id).unwrap().character.revision -= 1;
            Ok(())
        })
        .await
        .unwrap();
    lifecycle::finalize(&app, id, CloseReason::Controller)
        .unwrap()
        .wait()
        .await
        .unwrap();
    drop(app);
    std::fs::remove_dir_all(dir).unwrap();
}
