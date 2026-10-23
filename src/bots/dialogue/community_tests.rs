use super::*;

#[test]
fn community_market_language_cannot_change_policy_terms_or_supply_consent_and_failure_is_observable()
 {
    let terms = "Biceps, agree 2a 2593x2/2636x1 @-2,78,74 !1999999999 b1";
    let good = Reply {
        intent: Intent::MarketAgree,
        reply: terms.into(),
    };
    assert!(market_reply(good, Intent::MarketAgree, terms).is_ok());
    for reply in [
        Reply {
            intent: Intent::Join,
            reply: terms.into(),
        },
        Reply {
            intent: Intent::MarketAgree,
            reply: terms.replace("2636x1", "2636x2"),
        },
    ] {
        let rejected = market_reply(reply, Intent::MarketAgree, terms);
        assert_eq!(rejected.as_ref().unwrap_err().0, "unsupported_market_terms");
        let offline =
            Service::new("offline-test-fixture", "http://127.0.0.1:1/", DEADLINE).unwrap();
        offline.completed(&rejected);
        assert_eq!(offline.health()["failures"], 1);
        assert_eq!(offline.health()["successes"], 0);
        assert_eq!(offline.health()["last_error"], "unsupported_market_terms");
    }
    assert!(Service::configured(false).unwrap().is_none());
}
