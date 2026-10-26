use super::*;

async fn habitat(seed: u64) -> World {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Map {
        width: 128,
        height: 128,
        cells: vec![
            crate::content::Cell {
                ground: 0x61,
                object: None,
                config: String::new(),
                terrain: 1,
                region: 0,
            };
            128 * 128
        ],
        spawns: vec![(1.5, 1.5)],
    };
    let store = crate::store::Store::open(std::path::Path::new(":memory:"), &content).unwrap();
    let account = store.list("god-reproduction".into()).await.unwrap();
    let character = store
        .create(account.id, content.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    let mut w = World::new(content, Arc::new(map)).unwrap();
    w.encounter = true;
    w.rng = rand::rngs::StdRng::seed_from_u64(seed);
    w.configure_population().unwrap();
    // Reproduction must work without the regional low-water replacement trigger.
    w.population_regions[1].target = 0;
    w.population_regions[1].limit = 64;
    let id = w.enter(character, None).unwrap();
    w.players.get_mut(&id).unwrap().pos = Pos { x: 80.5, y: 64.5 };
    w
}

fn parent(w: &mut World, kind: u16, pos: Pos) -> i32 {
    let entity = w.spawn_entity(kind, pos, vec![]).unwrap();
    let slot = w.population.len();
    w.population.push(Spawn {
        kind,
        pos,
        entity: Some(entity),
        camp: None,
    });
    w.mark_population(entity, slot).unwrap();
    w.entities.get_mut(&entity).unwrap().condition |=
        crate::condition::bit(crate::condition::PARALYZED)
            | crate::condition::bit(crate::condition::STUNNED);
    entity
}

#[tokio::test]
async fn ordinary_god_reproduction_uses_active_minute_not_regional_refill() {
    let mut w = habitat(12351).await;
    let center = Pos { x: 64.5, y: 64.5 };
    let id = parent(&mut w, 0x651, center);
    w.now += 60000;
    w.npc_step();
    assert_eq!(w.population_count(1), 1);
    w.now += 50;
    w.npc_step();
    assert_eq!(w.population_count(1), 2);
    let child = w.actors().find(|e| e.id != id).unwrap();
    assert_eq!(child.kind, 0x651);
    assert!((child.pos.distance(center) - 5.).abs() < 0.0001);
    assert!(child.parent.is_none());
    assert!(child.population_slot.is_some());
    assert!(child.damage.is_empty() && child.quest.is_none());
    assert_eq!(w.population_regions[1].target, 0);
}
