use super::*;

fn fixture() -> (Content, PlayerView, Brain, Request) {
    let (content, mut view) = super::super::quest_tests::fixture();
    let peer = &mut view.objects[0];
    peer.kind = crate::content::WIZARD;
    peer.name = "Peer".into();
    peer.account = Some(2);
    peer.position = view.position;
    peer.equipped = vec![0xa97];
    peer.level = Some(14);
    let brain = Brain::new(PROFILES[0], &view);
    let request = brain
        .adventure_request(
            adventures::Plan::Invite {
                peer: adventures::Member {
                    account: 2,
                    name: "Peer".into(),
                },
                purpose: adventures::Purpose::Equipment,
            },
            &view,
            &content,
        )
        .unwrap();
    (content, view, brain, request)
}

#[test]
fn publication_deadline_is_stable_length_sensitive_and_staggers_speakers() {
    let now = Instant::now();
    let deadlines: Vec<_> = PROFILES
        .iter()
        .map(|p| publication_at(now, *p, "hi", "hi"))
        .collect();
    for (i, at) in deadlines.iter().enumerate() {
        assert_eq!(*at, publication_at(now, PROFILES[i], "hi", "hi"));
        assert!(*at > now + Duration::from_millis(750));
        assert!(*at < now + Duration::from_secs(4));
    }
    assert!(
        deadlines
            .windows(2)
            .all(|w| w[1] >= w[0] + Duration::from_millis(50))
    );
    let longer = publication_at(
        now,
        PROFILES[0],
        "can we level together?",
        "Peer, want to look for gear together?",
    );
    assert!(longer > deadlines[0]);
    // Fast generation waits; a five-second generation never adds another typing delay.
    assert!(longer.saturating_duration_since(now + Duration::from_millis(300)) > Duration::ZERO);
    assert_eq!(
        longer.saturating_duration_since(now + Duration::from_secs(5)),
        Duration::ZERO
    );
}

#[test]
fn optional_history_is_room_scoped_bounded_and_not_a_permanent_silence() {
    let mut speech = PublicSpeech::default();
    speech.record(-2, 1000, "want to level up together?");
    assert!(speech.repeated(-2, 1000, " WANT TO LEVEL UP TOGETHER? "));
    assert!(!speech.repeated(-3, 1000, "want to level up together?"));
    assert!(speech.repeated(-2, 90_999, "want to level up together?"));
    assert!(!speech.repeated(-2, 91_000, "want to level up together?"));
    for i in 0..5 {
        speech.record(-2, 1000, &format!("offer {i}"));
    }
    assert!(speech.repeated(-2, 1000, "want to level up together?"));
    for i in 0..40 {
        speech.record(-2, 1000, &format!("offer {i}"));
    }
    assert_eq!(speech.recent.len(), 32);
    assert!(!speech.repeated(-2, 1000, "want to level up together?"));
}

#[test]
fn delayed_dialogue_revalidates_world_peer_lifecycle_intent_and_expiry() {
    let (_, mut view, brain, mut request) = fixture();
    let invite = adventures::policy_reply(&request, Intent::Invite).unwrap();
    assert_eq!(invite.reply, "Peer, want to look for better gear together?");
    assert!(request.payload.get("recent_public_utterances").is_none());
    assert!(brain.validate_dialogue(&request, &invite, &view).is_ok());
    view.session.world -= 1;
    assert!(brain.validate_dialogue(&request, &invite, &view).is_err());
    view.session.world += 1;
    let peer = view.objects.remove(0);
    assert!(brain.validate_dialogue(&request, &invite, &view).is_err());
    view.objects.push(peer);
    view.objects[0].name = "SomeoneElse".into();
    assert!(brain.validate_dialogue(&request, &invite, &view).is_err());
    view.objects[0].name = request.name.clone();
    view.lifecycle.busy = true;
    assert!(brain.validate_dialogue(&request, &invite, &view).is_err());
    view.lifecycle.busy = false;
    let unrequested = Reply {
        intent: Intent::Join,
        reply: "yes".into(),
    };
    assert!(
        brain
            .validate_dialogue(&request, &unrequested, &view)
            .is_err()
    );
    request.created = Instant::now() - Duration::from_secs(11);
    assert!(
        brain
            .validate_dialogue(&request, &invite, &view)
            .unwrap_err()
            .to_string()
            .contains("expired")
    );
    assert!(brain.agreement.is_none());
    assert_eq!(brain.invitation_until, 0);
}

#[test]
fn pending_speech_does_not_hold_up_critical_recovery() {
    let (content, mut view, mut brain, _) = fixture();
    brain.proposing = true;
    view.character.hp = 100;
    assert_eq!(brain.decide(&view, &content).unwrap().goal, Goal::Recover);
}

#[tokio::test]
async fn explicitly_disabled_policy_speech_waits_once_and_can_be_cancelled() {
    let (_, _, _, request) = fixture();
    let mut pending = Pending::start(None, request, PROFILES[0]);
    assert!(!pending.task.is_finished());
    let reply = tokio::time::timeout(Duration::from_secs(5), &mut pending.task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(reply.intent, Intent::Invite);
    let (_, _, _, request) = fixture();
    let mut pending = Pending::start(None, request, PROFILES[0]);
    pending.task.abort();
    assert!((&mut pending.task).await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn provider_failure_remains_observable_and_never_becomes_policy_speech() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/",
                axum::routing::post(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
            ),
        )
        .await
        .unwrap();
    });
    let service =
        Arc::new(Service::new("local-test-only", &endpoint, Duration::from_secs(2)).unwrap());
    let (_, _, _, request) = fixture();
    let mut pending = Pending::start(Some(service.clone()), request, PROFILES[0]);
    let failure = (&mut pending.task).await.unwrap().unwrap_err();
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    assert_eq!(failure.0, "provider_rejected");
    let health = service.health();
    assert_eq!(health["model"], MODEL);
    assert_eq!(health["successes"], 0);
    assert_eq!(health["failures"], 1);
    assert_eq!(health["last_error"], "provider_rejected");
}
