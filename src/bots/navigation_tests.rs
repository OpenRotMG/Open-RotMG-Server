use super::*;
use crate::{
    navigation::{Geometry, Tiles},
    world::view::{Lifecycle, MapInfo, OwnCharacter, Session},
};
use rand::RngCore;

#[test]
fn unseen_terrain_cannot_change_controller_decisions_or_randomness() {
    let content = Content::load(std::path::Path::new("data")).unwrap();
    let speeds = BTreeMap::from([(1, 1.), (2, 0.)]);
    let one = Geometry::new((130, 130), vec![1; 130 * 130], &speeds);
    let mut hidden = vec![2; 130 * 130];
    for x in 0..66 {
        for y in 0..65 {
            hidden[y * 130 + x] = 1;
        }
    }
    let two = Geometry::new((130, 130), hidden, &speeds);
    let observed: Vec<_> = (0..65).flat_map(|x| (0..65).map(move |y| (x, y))).collect();
    let view = |geometry, account| PlayerView {
        session: Session {
            world: -3,
            player: 1,
            account,
            character: 1,
        },
        tick: 0,
        time_ms: 0,
        elapsed_ms: 0,
        position: Pos { x: 32.5, y: 32.5 },
        quest: None,
        map: MapInfo {
            name: "Realm".into(),
            width: 130,
            height: 130,
        },
        character: OwnCharacter {
            conditions: 0,
            class: crate::content::WIZARD,
            level: 1,
            experience: 0,
            hp: 100,
            mp: 100,
            stats: content.classes[&crate::content::WIZARD].base,
            base_stats: content.classes[&crate::content::WIZARD].base,
            items: content.objects[&crate::content::WIZARD].equipment.clone(),
            inventory_revision: 0,
            pirate_caves: 0,
            snake_pits: 0,
            ability_ready_ms: 0,
        },
        lifecycle: Lifecycle {
            alive: true,
            death_pending: false,
            departing: false,
            busy: false,
            quarantined: false,
            output: OutputState::Live,
        },
        tiles: Arc::new(Tiles::new(geometry, observed.clone())),
        objects: vec![],
        projectiles: vec![],
    };
    let describe = |d: &Decision| {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            d.goal,
            d.movement.map(|p| (p.x.to_bits(), p.y.to_bits())),
            d.aim.map(f32::to_bits),
            d.item,
            d.journey,
            d.chat
        )
    };
    for (n, profile) in PROFILES.into_iter().enumerate() {
        let (mut a, mut b) = (
            view(one.clone(), n as i32 + 1),
            view(two.clone(), n as i32 + 1),
        );
        let (mut left, mut right) = (Brain::new(profile, &a), Brain::new(profile, &b));
        for tick in (12..144).step_by(6) {
            for v in [&mut a, &mut b] {
                v.tick = tick;
                v.time_ms = tick * 50;
                v.elapsed_ms = tick * 50;
            }
            if tick == 72 {
                for (v, g) in [(&mut a, &one), (&mut b, &two)] {
                    Arc::make_mut(&mut v.tiles).discover(g, (0..65).map(|y| (65, y)));
                }
            }
            let x = left.decide(&a, &content).unwrap();
            let y = right.decide(&b, &content).unwrap();
            assert_eq!(describe(&x), describe(&y));
            assert_eq!(left.rng.clone().next_u64(), right.rng.clone().next_u64());
            left.goal = x.goal;
            left.destination = x.movement;
            right.goal = y.goal;
            right.destination = y.movement;
        }
    }
}
