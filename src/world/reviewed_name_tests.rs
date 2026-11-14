use super::*;
use crate::{content::Review, store::Store};

#[tokio::test]
async fn reviewed_accounts_look_like_unnamed_guests_to_other_players_only() {
    let mut content = Content::load(std::path::Path::new("data")).unwrap();
    let store = Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let marked = store.list("reviewed-name-marked".into()).await.unwrap().id;
    let plain = store.list("reviewed-name-plain".into()).await.unwrap().id;
    content.reviewed_accounts = vec![Review {
        account: marked,
        reviewed_on: "2026-11-05".into(),
    }];
    let content = Arc::new(content);
    let mut directory =
        Directory::test_with_seed(std::path::Path::new("data"), content.clone(), 16, 1098).unwrap();
    let mut ids = Vec::new();
    for (account, name) in [(marked, "MarkedDemo"), (plain, "PlainDemo")] {
        let mut ch = store
            .create(account, content.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        ch.name = name.into();
        let presentation = store.presentation(account).await.unwrap();
        let world = directory.worlds.get_mut(&-2).unwrap();
        ids.push(world.enter_at(ch, None, None, presentation, false).unwrap());
    }
    let name_chosen = |id, recipient| {
        super::legends_glow_tests::fields(&directory.worlds[&-2].stats(id, recipient).unwrap())[&56]
    };
    assert_eq!(
        [
            name_chosen(ids[0], ids[1]),
            name_chosen(ids[0], ids[0]),
            name_chosen(ids[1], ids[0]),
        ],
        [0, 1, 1]
    );
}
