use super::*;

#[tokio::test]
async fn community_lifecycle_death_committed_during_departure_rebuilds_in_nexus_without_resurrection()
 {
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
