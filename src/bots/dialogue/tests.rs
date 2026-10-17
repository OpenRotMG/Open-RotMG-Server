use super::*;
use crate::bots::tests::{content, object, view};
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Stub {
    url: String,
    calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn stub(status: StatusCode, body: String, delay: Duration) -> Stub {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let app = Router::new().route(
        "/",
        post(
            move |headers: axum::http::HeaderMap, Json(body_in): Json<Value>| {
                let count = count.clone();
                let body = body.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer test-key");
                    assert_eq!(body_in["model"], MODEL);
                    assert_eq!(
                        body_in["providerOptions"],
                        json!({"gateway":{"only":["nvidia"]}})
                    );
                    assert_eq!(body_in["max_tokens"], 300);
                    assert_eq!(body_in["tools"][0]["function"]["strict"], true);
                    assert_eq!(
                        body_in["tool_choice"]["function"]["name"],
                        "reply_to_player"
                    );
                    assert_eq!(body_in["parallel_tool_calls"], false);
                    assert!(
                        body_in["messages"][1]["content"]
                            .as_str()
                            .unwrap()
                            .contains("\n\nPlayer message:\n")
                    );
                    count.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    (status, [("retry-after", "10")], body).into_response()
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Stub { url, calls, task }
}
fn envelope(reply: Value) -> String {
    json!({"id":"test-upstream","model":MODEL,"choices":[{"finish_reason":"tool_calls","message":{"content":null,
        "provider_metadata":{"gateway":{"routing":{"finalProvider":"nvidia","canonicalSlug":MODEL},"cost":"0","generationId":"test-generation"}},
        "tool_calls":[{"type":"function","function":{"name":"reply_to_player","arguments":reply.to_string()}}]}}]}).to_string()
}
#[tokio::test]
async fn autonomous_inference_failure_does_not_publish_an_offer_or_use_policy_speech() {
    let s = stub(
        StatusCode::TOO_MANY_REQUESTS,
        "limited".into(),
        Duration::ZERO,
    )
    .await;
    let service = Service::new("test-key", &s.url, DEADLINE).unwrap();
    let result=service.ask(&json!({"allowed_intents":["invite"],"message":"Invite the visible player to hunt for equipment"}),"autonomous-failure").await;
    assert!(matches!(result, Err(Failure("rate_limited"))));
    assert!(!service.automatic_available());
    let mut society = adventures::Society::default();
    assert!(society.outgoing(1, 0).is_none());
    assert!(society.group(1, 0).is_none());
    assert_eq!(s.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn declarative_invitation_is_a_visible_failure_not_an_offer() {
    let stub = stub(
        StatusCode::OK,
        envelope(json!({"intent":"invite","reply":"Ready, let us go."})),
        Duration::ZERO,
    )
    .await;
    let service = Service::new("test-key", &stub.url, DEADLINE).unwrap();
    assert!(matches!(
        service
            .ask(
                &json!({"speaker":{"name":"Aster"},"allowed_intents":["invite"],"message":"Initiate an equipment hunt"}),
                "not-an-invitation"
            )
            .await,
        Err(Failure("invalid_reply"))
    ));
}
#[test]
fn invitation_names_are_present_once_and_questions_are_required() {
    assert!(invitation("Aster, want to hunt gear?", "Aster"));
    for text in [
        "Want to hunt gear?",
        "Aster, Aster, want to hunt gear?",
        "Aster, ready, let us go.",
        "Biceps, want to hunt gear?",
    ] {
        assert!(!invitation(text, "Aster"));
    }
}
#[test]
fn an_outgoing_invitation_excludes_other_commitments_and_a_group_can_part() {
    let c = content();
    let mut v = view(&c);
    peer(&mut v);
    let mut b = Brain::new(PROFILES[1], &v);
    let own = adventures::Member {
        account: v.session.account,
        name: "Biceps".into(),
    };
    let partner = adventures::Member {
        account: 77,
        name: "Aster".into(),
    };
    b.offered = Some(adventures::Offer {
        from: own.clone(),
        to: partner.clone(),
        purpose: adventures::Purpose::Equipment,
        until: v.elapsed_ms + 60000,
        text: "Aster, want to hunt gear?".into(),
        answer_started: false,
    });
    assert!(
        !b.dialogue_allowed(&v, "SomeoneElse")
            .contains(&Intent::Follow)
    );
    b.offered = None;
    b.group = Some(adventures::Adventure {
        members: vec![own, partner],
        purpose: adventures::Purpose::Equipment,
        until: v.elapsed_ms + 600000,
        finished: false,
    });
    assert!(b.dialogue_allowed(&v, "Aster").contains(&Intent::Leave));
    assert!(
        !b.dialogue_allowed(&v, "SomeoneElse")
            .contains(&Intent::Leave)
    );
}
#[test]
fn autonomous_allowance_reserves_capacity_for_interactive_requests() {
    let service = Service::new("test-key", "http://127.0.0.1:1", DEADLINE).unwrap();
    assert!(service.automatic_available());
    service.budget.lock().unwrap().daily = 800;
    assert!(!service.automatic_available());
    assert_eq!(service.health()["automatic_remaining"], 0);
    assert!(service.budget.lock().unwrap().daily < 1000);
}
#[tokio::test]
async fn pinned_provider_uses_one_strict_request_and_real_json() {
    let stub = stub(
        StatusCode::OK,
        envelope(json!({"intent":"follow","reply":"Sure, let's go."})),
        Duration::ZERO,
    )
    .await;
    let service = Service::new("test-key", &stub.url, DEADLINE).unwrap();
    let reply = service
        .ask(&json!({"message":"BattleCrus, come along?","allowed_intents":["follow","wait","leave","decline","talk"]}), "test")
        .await
        .unwrap();
    assert_eq!(reply.intent, Intent::Follow);
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    assert!(Service::configured(false).unwrap().is_none());
    assert!(Service::new("", &stub.url, DEADLINE).is_err());
    assert!(Service::new("bad\nheader", &stub.url, DEADLINE).is_err());
}
#[tokio::test]
async fn rate_limit_cools_down_without_retry_or_fallback() {
    let stub = stub(
        StatusCode::TOO_MANY_REQUESTS,
        "provider secret text".into(),
        Duration::ZERO,
    )
    .await;
    let service = Service::new("test-key", &stub.url, DEADLINE).unwrap();
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .0,
        "rate_limited"
    );
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .0,
        "rate_limited"
    );
    assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    assert!(service.budget.lock().unwrap().blocked_until > Instant::now() + Duration::from_secs(9));
}
#[tokio::test]
async fn timeout_and_provider_errors_are_not_invented_replies() {
    let slow = stub(
        StatusCode::OK,
        envelope(json!({"intent":"talk","reply":"Too late"})),
        Duration::from_millis(200),
    )
    .await;
    let service = Service::new("test-key", &slow.url, Duration::from_millis(30)).unwrap();
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .0,
        "timeout"
    );
    assert_eq!(slow.calls.load(Ordering::SeqCst), 1);
    let denied = stub(
        StatusCode::UNAUTHORIZED,
        "do not expose provider body".into(),
        Duration::ZERO,
    )
    .await;
    let service = Service::new("test-key", &denied.url, DEADLINE).unwrap();
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .to_string(),
        "provider_rejected"
    );
    assert_eq!(denied.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn malformed_and_command_replies_fail_closed() {
    let mut invalid = vec![
        envelope(json!({"intent":"spawn_item","reply":"Gift"})),
        envelope(json!({"reply":"Missing intention"})),
        envelope(json!({"intent":"talk","reply":"Hi","secret":"extra"})),
        envelope(json!({"intent":"talk","reply":"/tell someone private"})),
        envelope(json!({"intent":"talk","reply":"a".repeat(91)})),
        envelope(json!({"intent":"talk","reply":"bad\nline"})),
        "not json".into(),
        "x".repeat(65537),
    ];
    let mut truncated: Value =
        serde_json::from_str(&envelope(json!({"intent":"follow","reply":"Hi"}))).unwrap();
    truncated["choices"][0]["finish_reason"] = json!("length");
    invalid.push(truncated.to_string());
    for calls in [
        json!([]),
        json!([{"type":"function","function":{"name":"wrong","arguments":"{}"}}]),
        json!([{}, {}]),
    ] {
        let mut body: Value =
            serde_json::from_str(&envelope(json!({"intent":"talk","reply":"Hi"}))).unwrap();
        body["choices"][0]["message"]["tool_calls"] = calls;
        invalid.push(body.to_string());
    }
    for body in invalid {
        let stub = stub(StatusCode::OK, body, Duration::ZERO).await;
        let service = Service::new("test-key", &stub.url, DEADLINE).unwrap();
        assert!(
            service
                .ask(
                    &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                    "test"
                )
                .await
                .is_err()
        );
        assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn changed_provider_model_or_cost_latches_explicit_failure() {
    for (field, value) in [
        ("provider", "other"),
        ("model", "other/model"),
        ("canonical_model", "other/model"),
        ("cost", "0.01"),
    ] {
        let mut body: Value =
            serde_json::from_str(&envelope(json!({"intent":"talk","reply":"Hi"}))).unwrap();
        match field {
            "provider" => {
                body["choices"][0]["message"]["provider_metadata"]["gateway"]["routing"]["finalProvider"] =
                    json!(value)
            }
            "cost" => {
                body["choices"][0]["message"]["provider_metadata"]["gateway"]["cost"] = json!(value)
            }
            "model" => body["model"] = json!(value),
            "canonical_model" => {
                body["choices"][0]["message"]["provider_metadata"]["gateway"]["routing"]["canonicalSlug"] =
                    json!(value)
            }
            _ => unreachable!(),
        }
        let stub = stub(StatusCode::OK, body.to_string(), Duration::ZERO).await;
        let service = Service::new("test-key", &stub.url, DEADLINE).unwrap();
        for _ in 0..2 {
            assert_eq!(
                service
                    .ask(&json!({"allowed_intents":["talk"]}), "test")
                    .await
                    .unwrap_err()
                    .0,
                "provider_contract_changed"
            );
        }
        assert_eq!(stub.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn concurrency_and_request_budgets_do_not_queue_stale_calls() {
    let stub = stub(
        StatusCode::OK,
        envelope(json!({"intent":"talk","reply":"Hi"})),
        Duration::ZERO,
    )
    .await;
    let service = Service::new("test-key", &stub.url, DEADLINE).unwrap();
    let a = service.slots.clone().acquire_owned().await.unwrap();
    let b = service.slots.clone().acquire_owned().await.unwrap();
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .0,
        "busy"
    );
    drop((a, b));
    service.budget.lock().unwrap().calls = 30;
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .0,
        "request_budget_exhausted"
    );
    {
        let mut b = service.budget.lock().unwrap();
        b.calls = 0;
        b.daily = 1000;
    }
    assert_eq!(
        service
            .ask(
                &json!({"allowed_intents":["follow","wait","leave","decline","talk"]}),
                "test"
            )
            .await
            .unwrap_err()
            .0,
        "request_budget_exhausted"
    );
    assert_eq!(stub.calls.load(Ordering::SeqCst), 0);
}
fn packet(name: &str, id: i32, text: &str, recipient: &str) -> Packet {
    let mut w = crate::wire::Writer::new();
    w.utf(name)
        .i32(id)
        .i32(0)
        .u8(5)
        .utf(recipient)
        .utf(text)
        .utf("");
    Packet {
        id: wire::TEXT,
        body: w.0,
    }
}
fn peer(v: &mut PlayerView) {
    let mut p = object(77, crate::content::WIZARD, v.position.x + 1., v.position.y);
    p.name = "Aster".into();
    p.equipped = vec![2711, 2606, -1, -1];
    v.objects.push(p);
}
#[test]
fn enabled_dialogue_never_falls_back_to_legacy_follow() {
    let c = content();
    let mut v = view(&c);
    peer(&mut v);
    v.objects[0].position.x = v.position.x + 15.;
    let mut brain = Brain::new(PROFILES[2], &v);
    let p = packet("Aster", 77, "BattleCrus follow me", "");
    assert!(brain.dialogue_request(&p, &v, &c).unwrap().is_none());
    assert!(!brain.receive(p, &v, false).unwrap());
    assert!(brain.companion.is_none());
}
#[test]
fn only_visible_addressed_public_human_context_is_sent() {
    let c = content();
    let mut v = view(&c);
    peer(&mut v);
    let brain = Brain::new(PROFILES[2], &v);
    let p = packet("Aster", 77, "BattleCrus, fancy adventuring?", "");
    let request = brain.dialogue_request(&p, &v, &c).unwrap().unwrap();
    let serialized = request.payload.to_string();
    for forbidden in [
        "account",
        "session",
        "tiles",
        "projectiles",
        "seed",
        "inventory_revision",
    ] {
        assert!(!serialized.contains(forbidden));
    }
    assert!(
        brain
            .dialogue_request(&packet("Aster", 77, "Someone else, come?", ""), &v, &c)
            .unwrap()
            .is_none()
    );
    assert!(
        brain
            .dialogue_request(
                &packet("Aster", 77, "BattleCrus, secret", "BattleCrus"),
                &v,
                &c
            )
            .unwrap()
            .is_none()
    );
    assert!(
        brain
            .dialogue_request(&packet("Biceps", 77, "BattleCrus, follow?", ""), &v, &c)
            .unwrap()
            .is_none()
    );
    v.objects[0].position.x += 30.;
    assert!(brain.dialogue_request(&p, &v, &c).unwrap().is_none());
}
#[test]
fn agreements_follow_wait_resume_leave_and_do_not_claim_hidden_positions() {
    let c = content();
    let mut v = view(&c);
    peer(&mut v);
    let mut brain = Brain::new(PROFILES[2], &v);
    let request = brain
        .dialogue_request(&packet("Aster", 77, "BattleCrus, join me?", ""), &v, &c)
        .unwrap()
        .unwrap();
    let mut reply = Reply {
        intent: Intent::Follow,
        reply: "Sure".into(),
    };
    brain.validate_dialogue(&request, &reply, &v).unwrap();
    brain.apply_dialogue(&request, &reply, &v);
    assert_eq!(brain.decide(&v, &c).unwrap().goal, Goal::Follow(77));
    v.tick += 6;
    v.time_ms += 300;
    v.objects[0].position.x += 5.;
    reply.intent = Intent::Wait;
    brain.validate_dialogue(&request, &reply, &v).unwrap();
    brain.apply_dialogue(&request, &reply, &v);
    let wait = brain.decide(&v, &c).unwrap();
    assert_eq!(wait.goal, Goal::Gather(v.session.player));
    assert!(wait.movement.is_none());
    v.tick += 6;
    v.time_ms += 300;
    reply.intent = Intent::Follow;
    brain.apply_dialogue(&request, &reply, &v);
    assert_eq!(brain.decide(&v, &c).unwrap().goal, Goal::Follow(77));
    v.tick += 6;
    v.time_ms += 300;
    v.objects.clear();
    assert!(brain.validate_dialogue(&request, &reply, &v).is_err());
    assert!(!matches!(
        brain.decide(&v, &c).unwrap().journey,
        Some(Journey::Regroup(_))
    ));
    peer(&mut v);
    reply.intent = Intent::Leave;
    brain.validate_dialogue(&request, &reply, &v).unwrap();
    brain.apply_dialogue(&request, &reply, &v);
    assert!(brain.agreement.is_none());
}
#[test]
fn accepted_nexus_agreement_overrides_optional_departure_and_crew_wait() {
    let c = content();
    let mut v = view(&c);
    v.map.name = "Nexus".into();
    peer(&mut v);
    let mut brain = Brain::new(PROFILES[1], &v);
    let request = brain
        .dialogue_request(&packet("Aster", 77, "Biceps, join me?", ""), &v, &c)
        .unwrap()
        .unwrap();
    let reply = Reply {
        intent: Intent::Follow,
        reply: "Sure".into(),
    };
    brain.validate_dialogue(&request, &reply, &v).unwrap();
    brain.apply_dialogue(&request, &reply, &v);
    let d = brain.decide(&v, &c).unwrap();
    assert_eq!(d.goal, Goal::Follow(77));
    assert!(d.journey.is_none());
}
#[test]
fn stale_sessions_conflicting_partners_recovery_and_invalid_intents_reject_acceptance() {
    let c = content();
    let mut v = view(&c);
    peer(&mut v);
    let mut brain = Brain::new(PROFILES[2], &v);
    let mut request = brain
        .dialogue_request(&packet("Aster", 77, "BattleCrus, follow?", ""), &v, &c)
        .unwrap()
        .unwrap();
    let follow = Reply {
        intent: Intent::Follow,
        reply: "Sure".into(),
    };
    brain.apply_dialogue(&request, &follow, &v);
    request.name = "SomeoneElse".into();
    assert!(
        !brain
            .dialogue_allowed(&v, &request.name)
            .contains(&Intent::Follow)
    );
    request.name = "Aster".into();
    v.character.hp = 1;
    assert!(brain.validate_dialogue(&request, &follow, &v).is_err());
    v.character.hp = v.character.stats[0];
    v.session.world += 1;
    assert!(brain.validate_dialogue(&request, &follow, &v).is_err());
    v.session.world -= 1;
    request.created = Instant::now() - Duration::from_secs(11);
    assert!(brain.validate_dialogue(&request, &follow, &v).is_err());
    let wait = Reply {
        intent: Intent::Wait,
        reply: "Waiting".into(),
    };
    request.created = Instant::now();
    brain.apply_dialogue(&request, &wait, &v);
    v.character.hp = 1;
    let action = brain.decide(&v, &c).unwrap();
    assert!(matches!(action.goal, Goal::Recover | Goal::Dodge));
    v.tick += 6;
    v.time_ms = brain.invitation_until;
    brain.decide(&v, &c).unwrap();
    assert!(brain.agreement.is_none());
}
