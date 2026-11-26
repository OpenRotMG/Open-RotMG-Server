use super::*;

fn ship_world() -> (World, i32, i32, i32) {
    let content = Arc::new(crate::world::test_content());
    let map = Arc::new(Map {
        elevation: None,
        width: 128,
        height: 128,
        cells: vec![
            crate::content::Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            128 * 128
        ],
        spawns: vec![(64.5, 64.5)],
    });
    let mut w = World::in_domain(content, map, WorldDomain::NonRealm).unwrap();
    w.name = "Realm".into();
    w.encounter = true;
    let mut ch = Character::fresh(w.content.get(crate::content::WIZARD).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let player = w.enter(ch, None).unwrap();
    let ship = w
        .spawn_entity(0xe37, Pos { x: 64.5, y: 64.5 }, vec![])
        .unwrap();
    let anchor = w
        .spawn_entity(0xe38, Pos { x: 64.5, y: 64.5 }, vec![])
        .unwrap();
    w.bind_ghost_ship(ship, anchor).unwrap();
    (w, ship, anchor, player)
}

#[test]
fn anchor_make_ship_keeps_the_reserved_ship_as_its_only_ghost_ship() {
    // GhostAnchor MakeShip (actors.xml:11753) emits on entry and owns at most one Ghost Ship; native VPS
    // f210c074 published a second ship (parent=anchor) beside the reserved one.
    let (mut w, ship, anchor, _) = ship_world();
    for tick in 0..80 {
        w.now = tick * 50;
        w.xml_step().unwrap();
        crate::world::xml::tests::drain_events(&mut w).unwrap();
    }
    let ships: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.kind == 0xe37)
        .map(|e| e.id)
        .collect();
    assert_eq!(ships, [ship]);
    assert_eq!(*w.entities[&ship].parent, Some(anchor));
}

#[test]
fn vengeful_spirit_defeats_commit_without_xp() {
    // XML Vengeful Spirits carry XpMult 0 (ghostShip.xml:576); each defeat commits through the journal without XP.
    let (mut w, ship, _, player) = ship_world();
    w.players.get_mut(&player).unwrap().character.name = "Ship".into();
    let xp = w.players[&player].character.exp;
    let mut previous = None;
    for _ in 0..2 {
        let pos = w.entities[&ship].pos;
        let child = w.spawn_entity(0xe39, pos, vec![]).unwrap();
        assert_ne!(previous, Some(child));
        w.entities.get_mut(&child).unwrap().hp = 1;
        w.damage_enemy(player, child, 1000, true, &[], None)
            .unwrap();
        for _ in 0..4 {
            crate::world::xml::tests::drain_events(&mut w).unwrap();
        }
        assert!(!w.entities.contains_key(&child));
        assert_eq!(w.players[&player].character.exp, xp);
        previous = Some(child);
    }
}
