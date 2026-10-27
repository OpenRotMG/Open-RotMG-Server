use super::*;
use rand::SeedableRng;

#[test]
fn living_fame_carries_unboosted_xp_through_boost_expiry() {
    let content = Content::load(Path::new("data")).unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(778);
    let mut ch = Character::fresh(content.get(WIZARD).unwrap()).unwrap();
    // This tests the selected reconstruction, not the original server's XP curve: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.Leveling.cs#L165-L170
    for (award, fame) in [(999, 0), (1, 1), (999, 1), (1, 2)] {
        ch.gain_experience(&content, award, &mut rng).unwrap();
        assert_eq!(ch.fame, fame);
    }
    ch.gain_experience(&content, level_start(19).unwrap() - ch.exp, &mut rng)
        .unwrap();
    ch.xp_boost.active = true;
    ch.gain_experience(&content, 925, &mut rng).unwrap();
    assert_eq!((ch.level, ch.exp, ch.fame), (20, 18050, 17));
    assert_eq!(
        ch.xp_boost,
        XpBoost {
            active: false,
            bonus_exp: 925
        }
    );
    ch.gain_experience(&content, 874, &mut rng).unwrap();
    assert_eq!(ch.fame, 17);
    ch.gain_experience(&content, 1, &mut rng).unwrap();
    assert_eq!(ch.fame, 18);
    let before = serde_json::to_value(&ch).unwrap();
    assert!(ch.gain_experience(&content, -1, &mut rng).is_err());
    assert_eq!(serde_json::to_value(&ch).unwrap(), before);
}

#[test]
fn living_fame_goal_uses_the_class_record_and_has_a_terminal_sentinel() {
    let mut presentation = AccountPresentation {
        guild: None,
        credits: 0,
        fame: 9000,
        best_fame: BTreeMap::new(),
    };
    for (fame, next, stars) in [
        (0, 20, 0),
        (19, 20, 0),
        (20, 150, 1),
        (149, 150, 1),
        (150, 400, 2),
        (399, 400, 2),
        (400, 800, 3),
        (799, 800, 3),
        (800, 2000, 4),
        (1999, 2000, 4),
        (2000, -1, 5),
        (2100, -1, 5),
    ] {
        assert_eq!(presentation.next_quest(WIZARD, fame), next);
        presentation.best_fame.insert(WIZARD, fame);
        assert_eq!(presentation.stars(), stars);
        assert_eq!(presentation.next_quest(WIZARD, 0), next);
    }
    assert_eq!(presentation.next_quest(0x300, 0), 20);
}

#[tokio::test]
async fn living_fame_records_follow_commits_without_paying_the_death_wallet() {
    let content = Content::load(Path::new("data")).unwrap();
    let store = Store::open(Path::new(":memory:"), &content).unwrap();
    let account = store.list("living-fame-fixture".into()).await.unwrap().id;
    let mut ch = store
        .create(account, content.get(WIZARD).unwrap())
        .await
        .unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(778);
    ch.gain_experience(&content, 20000, &mut rng).unwrap();
    assert_eq!(store.presentation(account).await.unwrap().stars(), 0);
    store.progress(ch.clone()).await.unwrap();
    let loaded = store.load(account, ch.id).await.unwrap();
    assert_eq!((loaded.exp, loaded.fame), (20000, 20));
    let presentation = store.presentation(account).await.unwrap();
    assert_eq!(
        (
            presentation.fame,
            presentation.stars(),
            presentation.next_quest(WIZARD, 0)
        ),
        (0, 1, 150)
    );
    let mut stale = ch.clone();
    stale.exp = 19000;
    stale.fame = 19;
    assert!(store.progress(stale).await.is_err());
    assert_eq!(store.load(account, ch.id).await.unwrap().fame, 20);
    ch.alive = false;
    ch.hp = 0;
    store
        .death(ch.clone(), "fame fixture".into())
        .await
        .unwrap();
    let paid = store.presentation(account).await.unwrap();
    assert!(paid.fame >= 20);
    assert!(store.death(ch, "fame fixture".into()).await.is_err());
    assert_eq!(store.presentation(account).await.unwrap(), paid);
}
