use super::*;

#[test]
fn lair_traps_retire_after_their_first_volley_between_auxiliary_checks() {
    for kind in [0xda2, 0xda3] {
        let mut w = tests::arena(0);
        let player = w.enter(tests::ch(&w), None).unwrap();
        let home = Pos { x: 10.5, y: 10.5 };
        let trap = w.spawn_entity(kind, home, vec![]).unwrap();
        let outside = Pos { x: 15.5, y: 10.5 };
        let inside = Pos { x: 13., y: 10.5 };
        w.players.get_mut(&player).unwrap().pos = outside;
        w.now = 50;
        assert!(w.npc_step().is_empty());
        assert_eq!(w.entities[&trap].next_aux, 1050);
        w.players.get_mut(&player).unwrap().pos = inside;
        w.now = 100;
        assert!(!w.npc_step().is_empty());
        assert_eq!(w.bullets.iter().filter(|b| b.owner == trap).count(), 12);
        w.players.get_mut(&player).unwrap().pos = outside;
        w.step();
        for time in [1100, 2100, 3100] {
            w.now = time;
            w.npc_step();
            w.players.get_mut(&player).unwrap().pos = inside;
            w.now += 50;
            w.npc_step();
            w.players.get_mut(&player).unwrap().pos = outside;
        }
        assert_eq!(
            w.bullets.iter().filter(|b| b.owner == trap).count(),
            12,
            "trap {kind:#x} fired again after its first volley"
        );
        assert!(
            !w.entities.contains_key(&trap),
            "trap {kind:#x} was not retired"
        );
        assert!(!w.actor_ids.contains(&trap));
        assert!(!w.timed_ids.contains(&trap));
    }
}
