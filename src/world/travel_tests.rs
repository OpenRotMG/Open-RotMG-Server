use super::*;

#[test]
fn travel_denials_bindings_expiry_and_once_only_admission() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let mut d = Directory::test_with_seed(std::path::Path::new("data"), content, 2, 791).unwrap();
    d.clock.set_paused(true);
    let dungeon = d
        .instance("snake-pit", None, Some(DungeonReturn::Realm(-3)))
        .unwrap();
    let mut ch = Character::fresh(d.content.get(0x310).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let id = d
        .worlds
        .get_mut(&dungeon)
        .unwrap()
        .enter(ch.clone(), None)
        .unwrap();
    let exit = d.worlds[&dungeon]
        .entities
        .values()
        .find(|e| e.kind == 0x703)
        .unwrap()
        .id;
    let pos = d.worlds[&dungeon].entities[&exit].pos;
    d.worlds
        .get_mut(&dungeon)
        .unwrap()
        .players
        .get_mut(&id)
        .unwrap()
        .pos = pos;
    assert_eq!(d.destination(id, Some(exit)).unwrap(), (1, 1, -3));
    let vault = d.instance("vault", Some(2), None).unwrap();
    assert!(
        d.issue(1, 1, vault)
            .unwrap_err()
            .to_string()
            .contains("private destination")
    );
    assert!(
        d.issue(1, 1, i32::MAX)
            .unwrap_err()
            .to_string()
            .contains("destination missing")
    );
    d.worlds.get_mut(&-3).unwrap().max_players = 1;
    let mut occupant = ch.clone();
    occupant.account = 2;
    occupant.id = 2;
    let other = d
        .worlds
        .get_mut(&-3)
        .unwrap()
        .enter(occupant, None)
        .unwrap();
    assert!(
        d.issue(1, 1, -3)
            .unwrap_err()
            .to_string()
            .contains("destination full")
    );
    d.worlds.get_mut(&-3).unwrap().admission_open = false;
    assert!(
        d.destination(id, Some(exit))
            .unwrap_err()
            .to_string()
            .contains("has closed. Use Nexus")
    );
    assert!(
        d.issue(1, 1, -3)
            .unwrap_err()
            .to_string()
            .contains("Realm is closed")
    );
    assert_eq!(d.destination(id, None).unwrap(), (1, 1, -2));
    let source = &d.worlds[&dungeon].players[&id].character;
    assert_eq!(
        (
            source.account,
            source.id,
            source.alive,
            source.hp,
            source.items.clone(),
            source.revision
        ),
        (
            ch.account,
            ch.id,
            ch.alive,
            ch.hp,
            ch.items.clone(),
            ch.revision
        )
    );
    assert!(d.grants.is_empty());
    assert!(d.worlds[&-3].admissions.is_empty());
    d.worlds.get_mut(&-3).unwrap().admission_open = true;
    d.worlds
        .get_mut(&-3)
        .unwrap()
        .players
        .remove(&other)
        .unwrap();
    let (time, key, _) = d.issue(1, 1, -3).unwrap();
    for (account, destination, stamp) in [(2, -3, time), (1, -2, time), (1, -3, time + 1)] {
        assert!(
            d.consume(account, destination, stamp, key.clone())
                .unwrap_err()
                .to_string()
                .contains("binding mismatch")
        );
    }
    assert!(
        d.consume(1, -3, time, key.clone())
            .unwrap_err()
            .to_string()
            .contains("origin still owns")
    );
    d.tick += 2400;
    assert!(
        d.consume(1, -3, time, key.clone())
            .unwrap_err()
            .to_string()
            .contains("grant expired")
    );
    d.retire_transfer_grants();
    assert!(d.grants.is_empty());
    assert!(d.worlds[&-3].admissions.is_empty());
    let (time, key, _) = d.issue(1, 1, -3).unwrap();
    d.worlds
        .get_mut(&dungeon)
        .unwrap()
        .players
        .remove(&id)
        .unwrap();
    let (character, arrival) = d.consume(1, -3, time, key.clone()).unwrap();
    assert_eq!(character, ch.id);
    d.admit_transfer(arrival, 1, 1, -3).unwrap();
    assert!(
        d.consume(1, -3, time, key)
            .unwrap_err()
            .to_string()
            .contains("already used")
    );
}
