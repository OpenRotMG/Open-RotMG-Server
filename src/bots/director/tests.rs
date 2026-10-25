use super::*;

#[test]
fn session_end_waits_for_pending_work_and_promised_company() {
    let (_, mut view) = crate::bots::quest_tests::fixture();
    let mut brain = Brain::new(ARCHETYPES[0], &view);
    assert!(may_finish(&brain, &view));
    view.lifecycle.busy = true;
    assert!(!may_finish(&brain, &view));
    view.lifecycle.busy = false;
    brain.agreement = Some(dialogue::Agreement {
        partner: "Peer".into(),
        player: 42,
        waiting: None,
    });
    assert!(!may_finish(&brain, &view));
    brain.agreement = None;
    brain.group = Some(adventures::Adventure {
        members: vec![],
        purpose: adventures::Purpose::Equipment,
        until: 9999,
        finished: false,
    });
    assert!(!may_finish(&brain, &view));
    brain.group.as_mut().unwrap().finished = true;
    assert!(may_finish(&brain, &view));
}

#[tokio::test]
async fn retiring_a_preset_preserves_old_progress_and_creates_a_distinct_next_session() {
    use crate::store::{Character, FameLifetime, Store};
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let dir = std::env::temp_dir().join(format!("persona-retire-{:016x}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("game.db");
    let store = Store::open(&path, &content).unwrap();
    let mut snapshot = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    snapshot.lifetime = FameLifetime::known_only();
    let identity = "persona:v1:Session";
    let old = store
        .persona_character(
            identity.into(),
            vec![snapshot.clone()],
            vec![[0xa1f; 8]],
            0,
            None,
        )
        .await
        .unwrap();
    assert!(
        store
            .retire_persona(identity.into(), old.account + 1)
            .await
            .is_err()
    );
    store
        .retire_persona(identity.into(), old.account)
        .await
        .unwrap();
    let next = store
        .persona_character(identity.into(), vec![snapshot], vec![[0xa34; 8]], 0, None)
        .await
        .unwrap();
    assert_ne!(old.account, next.account);
    assert_eq!(old.name, next.name);
    assert_eq!(
        serde_json::to_value(store.load(old.account, old.id).await.unwrap()).unwrap(),
        serde_json::to_value(&old).unwrap()
    );
    assert_eq!(
        store.vaults(old.account).await.unwrap()[0].items,
        vec![0xa1f; 8]
    );
    assert_eq!(
        store.vaults(next.account).await.unwrap()[0].items,
        vec![0xa34; 8]
    );
    drop(store);
    Store::open(&path, &content).unwrap();
}
