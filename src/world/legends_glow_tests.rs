use super::*;
use crate::store::Store;

fn fields(status: &[u8]) -> BTreeMap<u8, i32> {
    let mut r = Reader::new(status);
    r.i32().unwrap();
    r.f32().unwrap();
    r.f32().unwrap();
    let count = r.i16().unwrap();
    let mut fields = BTreeMap::new();
    for _ in 0..count {
        let key = r.u8().unwrap();
        if matches!(key, 31 | 62) {
            r.utf().unwrap();
        } else {
            assert!(fields.insert(key, r.i32().unwrap()).is_none());
        }
    }
    r.done().unwrap();
    fields
}

#[tokio::test]
async fn legends_glow_initial_wire_and_guild_refresh_preserve_account_membership() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let store = Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let legend = store.list("glow-legend".into()).await.unwrap().id;
    let ordinary = store.list("glow-ordinary".into()).await.unwrap().id;
    let mut archived = store
        .create(legend, content.get(0x30e).unwrap())
        .await
        .unwrap();
    archived.alive = false;
    archived.hp = 0;
    store.death(archived, "fixture".into()).await.unwrap();
    let mut directory =
        Directory::test_with_seed(std::path::Path::new("data"), content.clone(), 16, 1098).unwrap();
    let mut players = Vec::new();
    for account in [legend, ordinary] {
        let ch = store
            .create(account, content.get(0x30e).unwrap())
            .await
            .unwrap();
        let legendary = store.is_legend(account).await.unwrap();
        let id = directory
            .worlds
            .get_mut(&-2)
            .unwrap()
            .enter_at(
                ch,
                None,
                None,
                store.presentation(account).await.unwrap(),
                legendary,
            )
            .unwrap();
        players.push(id);
    }
    let world = &directory.worlds[&-2];
    for (id, expected) in [(players[0], 1), (players[1], -1)] {
        for recipient in &players {
            assert_eq!(fields(&world.stats(id, *recipient).unwrap())[&59], expected);
        }
        let mut update = Writer::new();
        update.i16(0).i16(1).i16(0x30e);
        update.0.extend(world.stats(id, id).unwrap());
        update.i16(0);
        let projected = spectator::Projection::new(players[1])
            .packet(
                Packet {
                    id: wire::UPDATE,
                    body: update.0,
                },
                &content,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            fields(&projected.body[6..projected.body.len() - 2])[&59],
            expected
        );
    }
    let presentations = store
        .create_prepared_guild(vec![legend, ordinary], "Glow Fixture".into())
        .await
        .unwrap();
    for (account, presentation) in presentations {
        directory.sync_guild(account, presentation, false).unwrap();
    }
    directory
        .sync_guild(legend, store.presentation(legend).await.unwrap(), true)
        .unwrap();
    let world = &directory.worlds[&-2];
    assert!(world.players[&players[0]].legendary);
    assert!(!world.players[&players[1]].legendary);
    assert_eq!(
        fields(&world.stats(players[0], players[1]).unwrap())[&59],
        1
    );
    assert_eq!(
        fields(&world.stats(players[1], players[0]).unwrap())[&59],
        -1
    );
}
