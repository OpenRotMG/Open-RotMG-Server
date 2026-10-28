use super::*;

#[test]
fn pentaract_streams_follow_owned_corpses_visibility_and_completion() {
    let content = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let map = Arc::new(Map::load(std::path::Path::new("data/tutorial.wmap"), &content).unwrap());
    let mut w = World::new(content, map).unwrap();
    let positions: Vec<_> = (0..w.map.height)
        .flat_map(|y| {
            (0..w.map.width).map(move |x| Pos {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            })
        })
        .filter(|pos| w.walkable(*pos))
        .take(6)
        .collect();
    assert_eq!(positions.len(), 6);
    let root = w.spawn_entity(0xd5f, positions[5], vec![]).unwrap();
    let towers: Vec<_> = positions[..5]
        .iter()
        .map(|pos| w.spawn_entity(0xd5e, *pos, vec![]).unwrap())
        .collect();
    for id in std::iter::once(root).chain(towers.iter().copied()) {
        w.entities
            .get_mut(&id)
            .unwrap()
            .quest
            .set(Some(root), id, &mut w.quest_ids);
    }
    let stray = w.spawn_entity(0xd5e, positions[5], vec![]).unwrap();
    let mut ch = Character::fresh(w.content.get(0x30e).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    let (packets, mut inbox) = mpsc::channel(64);
    let (lost, _) = watch::channel(None);
    let player = w.enter(ch, Some(Output { packets, lost })).unwrap();
    while inbox.try_recv().is_ok() {}
    let check = |inbox: &mut mpsc::Receiver<Packet>, count, color| {
        let frames: Vec<_> = std::iter::from_fn(|| inbox.try_recv().ok())
            .filter(|p| p.id == wire::SHOW_EFFECT && p.body[0] == 3)
            .collect();
        assert_eq!(frames.len(), count);
        assert_eq!(
            frames
                .iter()
                .map(|p| p.body[5..21].to_vec())
                .collect::<BTreeSet<_>>()
                .len(),
            count
        );
        for p in frames {
            assert_eq!(p.body.len(), 25);
            assert_eq!(
                u32::from_be_bytes(p.body[21..25].try_into().unwrap()),
                color
            );
        }
    };
    w.players
        .get_mut(&player)
        .unwrap()
        .known
        .extend(towers.iter().copied());
    w.event_step();
    check(&mut inbox, 10, 0xffff0000);
    w.now = 50;
    w.event_step();
    check(&mut inbox, 0, 0);
    w.players.get_mut(&player).unwrap().known.clear();
    w.now = 250;
    w.event_step();
    check(&mut inbox, 0, 0);
    w.players.get_mut(&player).unwrap().known.insert(towers[4]);
    w.now = 500;
    w.event_step();
    check(&mut inbox, 4, 0xffff0000);
    let defeated = w.remove_entity(towers[0]);
    w.event_tower_death(&defeated).unwrap();
    let corpses = w.quest_members(root, &[0xd60]);
    assert_eq!(corpses.len(), 1);
    assert_eq!(w.entities[&corpses[0]].pos.x, defeated.pos.x);
    assert_eq!(w.entities[&corpses[0]].pos.y, defeated.pos.y);
    let members = w.quest_members(root, &[0xd5e, 0xd60]);
    w.players.get_mut(&player).unwrap().known.extend(members);
    w.now = 750;
    w.event_step();
    check(&mut inbox, 10, 0xffffff00);
    w.now = 15_500;
    w.event_step();
    check(&mut inbox, 10, 0xffffff00);
    assert!(w.quest_members(root, &[0xd60]).is_empty());
    let revived = w.quest_members(root, &[0xd5e]);
    assert_eq!(revived.len(), 5);
    w.players
        .get_mut(&player)
        .unwrap()
        .known
        .extend(revived.iter().copied());
    w.now = 15_750;
    w.event_step();
    check(&mut inbox, 10, 0xffff0000);
    let eye = w.spawn_entity(0xd5d, positions[0], vec![]).unwrap();
    w.entities
        .get_mut(&eye)
        .unwrap()
        .quest
        .set(Some(root), eye, &mut w.quest_ids);
    for id in revived {
        let defeated = w.remove_entity(id);
        w.event_tower_death(&defeated).unwrap();
    }
    w.now = 16_000;
    w.event_step();
    check(&mut inbox, 0, 0);
    assert!(!w.entities.contains_key(&root));
    assert!(!w.entities.contains_key(&eye));
    assert!(w.entities.contains_key(&stray));
    w.event_step();
    check(&mut inbox, 0, 0);
}
